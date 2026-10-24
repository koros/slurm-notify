//! Installed finite webhook configuration validation; no scheduler, state or sends.
use crate::config_validate::{get, object, text};
use std::path::Path;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Invalid,
    Permission,
}

pub(super) fn validate(path: &Path) -> Result<String, Error> {
    validate_with(path, read_env)
}
fn validate_with(
    path: &Path,
    mut lookup: impl FnMut(&str) -> Result<Zeroizing<Vec<u8>>, Error>,
) -> Result<String, Error> {
    let snapshot = crate::config_snapshot::read(path).map_err(|_| Error::Invalid)?;
    let draft = crate::config_validate::validate(snapshot.bytes()).map_err(|_| Error::Invalid)?;
    let root = object(&draft.value).map_err(|_| Error::Invalid)?;
    let providers =
        object(get(root, "providers").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?;
    let monitor =
        object(get(root, "monitor").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?;
    let cluster = text(get(monitor, "cluster_id").map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    let delivery = get(root, "delivery").map_err(|_| Error::Invalid)?;
    let mut smtp = false;
    for (name, value) in providers {
        let provider = object(value).map_err(|_| Error::Invalid)?;
        if text(get(provider, "type").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?
            == "smtp"
        {
            crate::installed_smtp::validate(name, value, cluster, delivery, &mut lookup)?;
            smtp = true;
            continue;
        }
        if let Ok(roots) = get(provider, "trust_der_files") {
            let paths = crate::config_validate::list(roots, 1, 32)
                .map_err(|_| Error::Invalid)?
                .iter()
                .map(|value| text(value).map(std::path::PathBuf::from))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| Error::Invalid)?;
            crate::trust_snapshot::load(&paths).map_err(|_| Error::Invalid)?;
        }
        let name = text(get(provider, "url_env").map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        let endpoint = lookup(name)?;
        if !endpoint.iter().all(|byte| (33..=126).contains(byte)) {
            return Err(Error::Invalid);
        }
        let endpoint = std::str::from_utf8(&endpoint).map_err(|_| Error::Invalid)?;
        let url = url::Url::parse(endpoint).map_err(|_| Error::Invalid)?;
        if url.scheme() != "https"
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Invalid);
        }
        if let Ok(auth) = get(provider, "authorization_env") {
            let value = lookup(text(auth).map_err(|_| Error::Invalid)?)?;
            if !value.iter().all(|byte| (32..=126).contains(byte)) {
                return Err(Error::Invalid);
            }
        }
    }
    let monitor =
        object(get(root, "monitor").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?;
    let cluster = text(get(monitor, "cluster_id").map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    Ok(format!("Configuration valid: schema_version=1 cluster={cluster} destinations={} smtp_permissions={}\n", providers.len(), if smtp { "approved" } else { "not_applicable" }))
}
fn read_env(name: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
    let value = std::env::var_os(name).ok_or(Error::Invalid)?;
    // The OS lookup allocates before this accepted-byte cap; no hard lookup deadline claim.
    let bytes = Zeroizing::new(value.into_encoded_bytes());
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(Error::Invalid);
    }
    std::str::from_utf8(&bytes).map_err(|_| Error::Invalid)?;
    Ok(bytes)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn smtp_validation_matches_private_revisions_and_checks_permission_before_credentials() {
        let directory = crate::storage::tests::Directory::new();
        let root = directory.path().parent().unwrap().canonicalize().unwrap();
        let trust_path = root.join("root.der");
        let certificate =
            rcgen::generate_simple_self_signed(vec!["smtp.example.invalid".into()]).unwrap();
        std::fs::write(&trust_path, certificate.cert.der()).unwrap();
        std::fs::set_permissions(&trust_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let path = root.join("config.json");
        let mut source: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap();
        source["providers"]["research_email"]["trust_der_files"] = serde_json::json!([trust_path]);
        let plan = crate::mixed_config_composition::prepare(
            &serde_json::to_vec(&source).unwrap(),
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            |name, _| {
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    b"https://fixture.invalid/hook".to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap();
        let revisions = plan.revisions();
        let (_, rendering, provider) = revisions[0];
        source["delivery"]["smtp_permissions"] = serde_json::json!([{
            "cluster_id":"research-cluster", "destination":"research_email",
            "rendering_revision":rendering, "provider_revision":provider
        }]);
        let write = |source: &serde_json::Value| {
            std::fs::write(&path, serde_json::to_vec(source).unwrap()).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        };
        let lookup = |name: &str| -> Result<Zeroizing<Vec<u8>>, Error> {
            Ok(Zeroizing::new(match name {
                "SLURM_NOTIFY_SMTP_USERNAME" => b"researcher".to_vec(),
                "SLURM_NOTIFY_SMTP_PASSWORD" => b"fixture-password".to_vec(),
                _ if name.ends_with("_URL") => b"https://fixture.invalid/hook".to_vec(),
                _ => b"Bearer fixture".to_vec(),
            }))
        };
        write(&source);
        let result = validate_with(&path, lookup).unwrap();
        assert_eq!(result, "Configuration valid: schema_version=1 cluster=research-cluster destinations=2 smtp_permissions=approved\n");
        assert!(!result.contains("researcher@") && !result.contains("smtp.example"));
        for case in 0..4 {
            let mut changed = source.clone();
            match case {
                0 => changed["delivery"]["smtp_permissions"] = serde_json::json!([]),
                1 => {
                    changed["providers"]["research_email"]["relay"] =
                        serde_json::json!("changed.example.invalid")
                }
                2 => {
                    changed["providers"]["research_email"]["recipient"] =
                        serde_json::json!("changed@example.invalid")
                }
                _ => {
                    changed["providers"]["research_email"]["visibility"] =
                        serde_json::json!("included")
                }
            }
            write(&changed);
            assert_eq!(
                validate_with(&path, |name| {
                    assert!(!name.contains("SMTP"), "denied credential lookup");
                    lookup(name)
                }),
                Err(Error::Permission)
            );
        }
        write(&source);
        for bad in [None, Some("wrong-identity"), Some("researcher\n")] {
            assert_eq!(
                validate_with(&path, |name| if name == "SLURM_NOTIFY_SMTP_USERNAME" {
                    bad.map(|value| Zeroizing::new(value.as_bytes().to_vec()))
                        .ok_or(Error::Invalid)
                } else {
                    lookup(name)
                }),
                Err(Error::Invalid)
            );
        }
        for bad in ["", "private\n", &"x".repeat(129)] {
            assert_eq!(
                validate_with(&path, |name| if name == "SLURM_NOTIFY_SMTP_PASSWORD" {
                    Ok(Zeroizing::new(bad.as_bytes().to_vec()))
                } else {
                    lookup(name)
                }),
                Err(Error::Invalid)
            );
        }
        std::fs::write(&trust_path, b"not a certificate").unwrap();
        assert_eq!(
            validate_with(&path, |name| {
                assert!(!name.contains("SMTP"));
                lookup(name)
            }),
            Err(Error::Invalid)
        );
    }
}
