//! Private local resource composition; no permission grant, delivery or activation.
use crate::{
    credential_resolution,
    smtp_provider_delivery::Resolved,
    smtp_provider_revision::{Descriptor, Profile},
    trust_snapshot,
};
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Profile,
    Trust(trust_snapshot::Error),
    TrustMismatch,
    Credentials(credential_resolution::Error),
}
pub(super) fn resolve(
    bound: &Descriptor,
    profile: &Profile<'_>,
    trust_paths: &[PathBuf],
    username_reference: &str,
    password_reference: &str,
    lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Resolved, Error> {
    if !bound.matches(profile).map_err(|_| Error::Profile)? {
        return Err(Error::Profile);
    }
    let trust = trust_snapshot::load(trust_paths).map_err(Error::Trust)?;
    if trust.fingerprint() != profile.trust_fingerprint {
        return Err(Error::TrustMismatch);
    }
    let credentials = credential_resolution::resolve(
        profile.authentication_identity,
        username_reference,
        password_reference,
        lookup,
    )
    .map_err(Error::Credentials)?;
    // The owned constructor rechecks both binding and credential identity.
    Resolved::from_owned(bound, profile, &trust.roots(), credentials).map_err(|_| Error::Profile)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use crate::{
        smtp_auth::Credentials,
        smtp_provider_revision::{self, TlsMode},
    };
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::OpenOptionsExt,
    };
    struct Fixture {
        _directory: crate::storage::tests::Directory,
        path: PathBuf,
        root: Vec<u8>,
        bound: Descriptor,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = crate::storage::tests::Directory::new();
            let path = directory
                .path()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join("root.der");
            let root = certificate("first");
            let hash = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
            let bound = Descriptor::new(&Profile {
                destination: "smtp",
                relay: "relay.fixture.invalid",
                port: 587,
                tls: TlsMode::RequiredStartTls,
                server_name: "smtp.fixture.invalid",
                trust_reference: "fixture-trust",
                trust_fingerprint: &hash,
                authentication_identity: "fixture-user",
                credential_reference: "fixture-secret",
            })
            .unwrap();
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap()
                .write_all(&root)
                .unwrap();
            Self {
                _directory: directory,
                path,
                root,
                bound,
            }
        }
        fn resolve(&self, password: &[u8]) -> Result<Resolved, Error> {
            resolve(
                &self.bound,
                &self.bound.profile(),
                std::slice::from_ref(&self.path),
                "USER",
                "PASSWORD",
                |name, _| {
                    Ok(Zeroizing::new(if name == "USER" {
                        b"fixture-user".to_vec()
                    } else {
                        password.to_vec()
                    }))
                },
            )
        }
    }
    fn certificate(name: &str) -> Vec<u8> {
        rcgen::generate_simple_self_signed(vec![format!("{name}.fixture.invalid")])
            .unwrap()
            .serialize_der()
            .unwrap()
    }
    #[test]
    fn composed_resources_survive_rotation_and_removal_with_same_binding() {
        let fixture = Fixture::new();
        let first = fixture.resolve(b"first-fixture").unwrap();
        let second = fixture.resolve(b"rotated-fixture").unwrap();
        assert_eq!(first.provider().token(), fixture.bound.token());
        assert_eq!(second.provider().token(), fixture.bound.token());
        fs::remove_file(&fixture.path).unwrap();
        let (first_binding, first_roots, first_credentials) = first.into_parts();
        let (second_binding, second_roots, second_credentials) = second.into_parts();
        assert_eq!(first_binding.token(), second_binding.token());
        assert!(first_roots == vec![fixture.root.clone()] && second_roots == first_roots);
        assert!(first_credentials.into_plain().as_slice() == b"\0fixture-user\0first-fixture");
        assert!(second_credentials.into_plain().as_slice() == b"\0fixture-user\0rotated-fixture");
    }
    #[test]
    fn profile_changes_block_before_files_or_secret_lookup() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.path).unwrap();
        for field in 0..5 {
            let mut profile = fixture.bound.profile();
            match field {
                0 => profile.relay = "changed.fixture.invalid",
                1 => profile.authentication_identity = "other-user",
                2 => profile.credential_reference = "other-secret",
                3 => profile.trust_reference = "other-trust",
                _ => profile.port = 0,
            }
            assert!(matches!(
                resolve(
                    &fixture.bound,
                    &profile,
                    std::slice::from_ref(&fixture.path),
                    "USER",
                    "PASSWORD",
                    |_, _| { panic!("changed binding must not resolve credentials") }
                ),
                Err(Error::Profile)
            ));
        }
    }
    #[test]
    fn changed_or_missing_trust_blocks_before_secret_lookup() {
        let fixture = Fixture::new();
        fs::write(&fixture.path, certificate("rotated")).unwrap();
        assert!(matches!(
            resolve(
                &fixture.bound,
                &fixture.bound.profile(),
                std::slice::from_ref(&fixture.path),
                "USER",
                "PASSWORD",
                |_, _| { panic!("changed trust must not resolve credentials") }
            ),
            Err(Error::TrustMismatch)
        ));
        fs::remove_file(&fixture.path).unwrap();
        assert!(matches!(
            fixture.resolve(b"fixture-password"),
            Err(Error::Trust(_))
        ));
    }
    #[test]
    fn credential_failure_does_not_publish_a_resolved_provider() {
        let fixture = Fixture::new();
        assert!(matches!(
            fixture.resolve(b""),
            Err(Error::Credentials(
                credential_resolution::Error::Credentials
            ))
        ));
        assert!(matches!(
            resolve(
                &fixture.bound,
                &fixture.bound.profile(),
                std::slice::from_ref(&fixture.path),
                "USER",
                "PASSWORD",
                |_, _| Err(())
            ),
            Err(Error::Credentials(credential_resolution::Error::Lookup))
        ));
    }
    #[test]
    fn owned_constructor_rechecks_identity_trust_and_profile() {
        let fixture = Fixture::new();
        let load = || trust_snapshot::load(std::slice::from_ref(&fixture.path)).unwrap();
        let credentials = |user| Credentials::new(Some(user), Some("fixture-password")).unwrap();
        assert!(matches!(
            Resolved::from_owned(
                &fixture.bound,
                &fixture.bound.profile(),
                &load().roots(),
                credentials("other-user")
            ),
            Err(crate::smtp_provider_delivery::Error::Profile)
        ));
        let mut profile = fixture.bound.profile();
        profile.port = 465;
        assert!(matches!(
            Resolved::from_owned(
                &fixture.bound,
                &profile,
                &load().roots(),
                credentials("fixture-user")
            ),
            Err(crate::smtp_provider_delivery::Error::Profile)
        ));
        fs::write(&fixture.path, certificate("other")).unwrap();
        assert!(matches!(
            Resolved::from_owned(
                &fixture.bound,
                &fixture.bound.profile(),
                &load().roots(),
                credentials("fixture-user")
            ),
            Err(crate::smtp_provider_delivery::Error::Trust)
        ));
    }
}
