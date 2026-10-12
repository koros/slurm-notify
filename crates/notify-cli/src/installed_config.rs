//! Installed finite webhook configuration validation; no scheduler, state or sends.
use crate::config_validate::{get, object, text};
use std::path::Path;
use zeroize::Zeroizing;

pub(super) enum Error {
    Invalid,
    UnsupportedSmtp,
}

pub(super) fn validate(path: &Path) -> Result<String, Error> {
    let snapshot = crate::config_snapshot::read(path).map_err(|_| Error::Invalid)?;
    let draft = crate::config_validate::validate(snapshot.bytes()).map_err(|_| Error::Invalid)?;
    let root = object(&draft.value).map_err(|_| Error::Invalid)?;
    let providers =
        object(get(root, "providers").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?;
    // Reject unsupported scope before reading any environment resource.
    for (_, value) in providers {
        let provider = object(value).map_err(|_| Error::Invalid)?;
        if text(get(provider, "type").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?
            == "smtp"
        {
            return Err(Error::UnsupportedSmtp);
        }
    }
    for (_, value) in providers {
        let provider = object(value).map_err(|_| Error::Invalid)?;
        let name = text(get(provider, "url_env").map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        let endpoint = read_env(name)?;
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
            let value = read_env(text(auth).map_err(|_| Error::Invalid)?)?;
            if !value.iter().all(|byte| (32..=126).contains(byte)) {
                return Err(Error::Invalid);
            }
        }
    }
    let monitor =
        object(get(root, "monitor").map_err(|_| Error::Invalid)?).map_err(|_| Error::Invalid)?;
    let cluster = text(get(monitor, "cluster_id").map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    Ok(format!("Configuration valid: schema_version=1 cluster={cluster} destinations={} smtp_permissions=not_applicable\n", providers.len()))
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
