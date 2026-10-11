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
    #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
    use crate::{
        smtp_auth::Credentials,
        smtp_provider_revision::{self, TlsMode},
    };
    #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
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

    fn seeded_store(fixture: &Fixture) -> crate::storage::Store {
        use crate::{
            lifecycle::Reconciler,
            scheduler::PollRequest,
            smtp_render::JobVisibility,
            smtp_routes::{Metadata, Plan, ProviderPlan, Route},
        };
        let mut store = reopen(fixture);
        let routes = [Route {
            name: "smtp",
            visibility: JobVisibility::Hidden,
            metadata: Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let rendering = Plan::new(&["smtp"], &routes, 4).unwrap();
        let request = PollRequest {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: std::time::Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        ProviderPlan::new(&rendering, &[fixture.bound.profile()])
            .unwrap()
            .poll_with(
                &request,
                &mut Reconciler::with_capacity(10),
                &mut store,
                10,
                |command: &mut std::process::Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        ""
                    } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0"
                    }
                    .into())
                },
            )
            .unwrap();
        store
    }
    fn reopen(fixture: &Fixture) -> crate::storage::Store {
        crate::storage::Store::open(
            &fixture._directory.path(),
            std::time::Duration::from_millis(50),
        )
        .unwrap()
    }
    fn policy() -> crate::delivery::RetryPolicy {
        crate::delivery::RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        }
    }
    fn delivery_error(error: Error) -> crate::smtp_provider_delivery::Error {
        match error {
            Error::Profile => crate::smtp_provider_delivery::Error::Profile,
            Error::Trust(_) | Error::TrustMismatch => crate::smtp_provider_delivery::Error::Trust,
            Error::Credentials(_) => crate::smtp_provider_delivery::Error::Credentials,
        }
    }
    #[test]
    fn permission_and_cancellation_gates_skip_composition_and_preserve_pending_work() {
        use crate::{
            delivery::FailureKind,
            smtp_provider_delivery::{self, Hooks, Permission, Permissions},
            supervised_provider::Diagnostic,
        };
        let fixture = Fixture::new();
        let mut store = seeded_store(&fixture);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let rendering = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: rendering.token(),
            provider: fixture.bound.token(),
        }];
        let granted = Permissions::new(&entries).unwrap();
        let denied = Permissions::new(&[]).unwrap();
        fs::remove_file(&fixture.path).unwrap();
        for cancelled in [false, true] {
            let failure = smtp_provider_delivery::deliver_due(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                Hooks {
                    permissions: if cancelled { &granted } else { &denied },
                    resolve: |_: &Descriptor| -> Result<Resolved, smtp_provider_delivery::Error> {
                        panic!("gate must precede composed resource lookup")
                    },
                    send: |_: &crate::storage::DueDelivery,
                           _: &crate::smtp_revision::Descriptor,
                           _: &crate::storage::FrozenMessage,
                           _: Resolved| panic!("gated send"),
                    clock: || panic!("gated clock"),
                    cancelled: || cancelled,
                },
            )
            .unwrap_err();
            if cancelled {
                assert_eq!(failure.kind, FailureKind::Cancelled);
            } else {
                assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpProviderDenied));
            }
            drop(store);
            store = reopen(&fixture);
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&item.event_id, "smtp")
                    .unwrap()
                    .unwrap()
                    .token(),
                fixture.bound.token()
            );
            assert!(store.frozen_smtp(&item.event_id, "smtp").unwrap().is_some());
        }
    }
    #[test]
    fn composed_resource_blocks_preserve_retry_and_frozen_bytes_until_restart_recovery() {
        use crate::{
            delivery::ProviderOutcome,
            smtp_provider_delivery::{self, Hooks, Permission, Permissions},
            supervised_provider::Diagnostic,
        };
        let fixture = Fixture::new();
        let mut store = seeded_store(&fixture);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let rendering = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let frozen = store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap();
        let checkpoint = store.restore("cluster", 10).unwrap().1;
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            rendering: rendering.token(),
            provider: fixture.bound.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let report = smtp_provider_delivery::deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |bound: &Descriptor| {
                    resolve(
                        bound,
                        &fixture.bound.profile(),
                        std::slice::from_ref(&fixture.path),
                        "USER",
                        "PASSWORD",
                        |name, _| {
                            Ok(Zeroizing::new(if name == "USER" {
                                b"fixture-user".to_vec()
                            } else {
                                b"first-fixture".to_vec()
                            }))
                        },
                    )
                    .map_err(delivery_error)
                },
                send: |_: &crate::storage::DueDelivery,
                       _: &crate::smtp_revision::Descriptor,
                       _: &crate::storage::FrozenMessage,
                       _: Resolved| Ok(ProviderOutcome::Retry),
                clock: || Some(12),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.retrying, 1);
        for mode in 0..4 {
            drop(store);
            store = reopen(&fixture);
            if mode == 0 {
                fs::remove_file(&fixture.path).unwrap();
            } else {
                let bytes = if mode == 1 {
                    certificate("changed")
                } else {
                    fixture.root.clone()
                };
                if fixture.path.exists() {
                    fs::write(&fixture.path, bytes).unwrap();
                } else {
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&fixture.path)
                        .unwrap()
                        .write_all(&bytes)
                        .unwrap();
                }
            }
            let failure = smtp_provider_delivery::deliver_due(
                &mut store,
                "cluster",
                17,
                10,
                policy(),
                Hooks {
                    permissions: &permissions,
                    resolve: |bound: &Descriptor| {
                        resolve(
                            bound,
                            &fixture.bound.profile(),
                            std::slice::from_ref(&fixture.path),
                            "USER",
                            "PASSWORD",
                            |name, _| {
                                assert!(mode >= 2, "trust failure must precede secret lookup");
                                Ok(Zeroizing::new(if name == "USER" {
                                    if mode == 3 {
                                        b"other-user".to_vec()
                                    } else {
                                        b"fixture-user".to_vec()
                                    }
                                } else {
                                    vec![]
                                }))
                            },
                        )
                        .map_err(delivery_error)
                    },
                    send: |_: &crate::storage::DueDelivery,
                           _: &crate::smtp_revision::Descriptor,
                           _: &crate::storage::FrozenMessage,
                           _: Resolved| panic!("blocked send"),
                    clock: || panic!("blocked clock"),
                    cancelled: || false,
                },
            )
            .unwrap_err();
            let expected = if mode < 2 {
                smtp_provider_delivery::Error::Trust
            } else {
                smtp_provider_delivery::Error::Credentials
            };
            assert_eq!(
                failure.diagnostic,
                Some(Diagnostic::SmtpProviderResolution(expected))
            );
            assert_eq!(
                store.due_deliveries("cluster", 17, 10).unwrap()[0].attempt_number,
                2
            );
            assert_eq!(
                store
                    .frozen_smtp(&item.event_id, "smtp")
                    .unwrap()
                    .unwrap()
                    .data(),
                frozen.data()
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&item.event_id, "smtp")
                    .unwrap()
                    .unwrap()
                    .canonical(),
                fixture.bound.canonical()
            );
            assert_eq!(store.restore("cluster", 10).unwrap().1, checkpoint);
        }
        drop(store);
        store = reopen(&fixture);
        let report = smtp_provider_delivery::deliver_due(
            &mut store,
            "cluster",
            17,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                resolve: |bound: &Descriptor| {
                    resolve(
                        bound,
                        &fixture.bound.profile(),
                        std::slice::from_ref(&fixture.path),
                        "USER",
                        "PASSWORD",
                        |name, _| {
                            Ok(Zeroizing::new(if name == "USER" {
                                b"fixture-user".to_vec()
                            } else {
                                b"rotated-fixture".to_vec()
                            }))
                        },
                    )
                    .map_err(delivery_error)
                },
                send: |due: &crate::storage::DueDelivery,
                       actual_render: &crate::smtp_revision::Descriptor,
                       actual_frozen: &crate::storage::FrozenMessage,
                       resolved: Resolved| {
                    assert_eq!(due.attempt_number, 2);
                    assert_eq!(actual_render.token(), rendering.token());
                    assert_eq!(actual_frozen.data(), frozen.data());
                    let (provider, roots, credentials) = resolved.into_parts();
                    assert_eq!(provider.token(), fixture.bound.token());
                    assert!(roots == vec![fixture.root.clone()]);
                    assert!(
                        credentials.into_plain().as_slice() == b"\0fixture-user\0rotated-fixture"
                    );
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(18),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        drop(store);
        store = reopen(&fixture);
        assert!(store.due_deliveries("cluster", 18, 10).unwrap().is_empty());
        assert_eq!(store.restore("cluster", 10).unwrap().1, checkpoint);
    }
}
