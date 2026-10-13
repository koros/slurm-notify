//! Finite SMTP profile validation and exact permission checks; no transport.
use crate::config_decode::Value;
use crate::{
    config_validate::{get, list, object, text},
    installed_config::Error,
};
use ring::digest::{digest, SHA256};
use std::path::PathBuf;

pub(super) fn validate(
    name: &str,
    provider: &Value,
    cluster: &str,
    delivery: &Value,
    lookup: &mut impl FnMut(&str) -> Result<zeroize::Zeroizing<Vec<u8>>, Error>,
) -> Result<(), Error> {
    let p = object(provider).map_err(|_| Error::Invalid)?;
    let field = |key| get(p, key).and_then(text).map_err(|_| Error::Invalid);
    let auth = object(get(p, "authentication").map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    let auth_field = |key| get(auth, key).and_then(text).map_err(|_| Error::Invalid);
    crate::smtp_address::address(field("sender")?).map_err(|_| Error::Invalid)?;
    crate::smtp_address::address(field("recipient")?).map_err(|_| Error::Invalid)?;
    let disclosure = if field("visibility")? == "hidden" {
        "hidden"
    } else {
        "identifier"
    };
    let canonical = crate::rendering_fingerprint::encode(&[
        ("destination", name),
        ("sender", field("sender")?),
        ("recipient", field("recipient")?),
        ("namespace", field("identity_namespace")?),
        ("disclosure", disclosure),
        ("renderer", "allowlist-v1"),
        ("identity", "sha256-event-route-revision-v2"),
        ("domain", "fixture.invalid"),
    ])
    .map_err(|_| Error::Invalid)?;
    let rendering: String = digest(&SHA256, &canonical)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let roots = list(
        get(p, "trust_der_files").map_err(|_| Error::Invalid)?,
        1,
        32,
    )
    .map_err(|_| Error::Invalid)?;
    let roots: Vec<_> = roots
        .iter()
        .map(text)
        .collect::<Result<_, _>>()
        .map_err(|_| Error::Invalid)?;
    let trust_reference = crate::config_reference::reference(b"trust\0", &roots);
    let username_ref = auth_field("username_env")?;
    let password_ref = auth_field("password_env")?;
    let credential_reference =
        crate::config_reference::reference(b"credentials\0", &[username_ref, password_ref]);
    let port = match get(p, "port").map_err(|_| Error::Invalid)? {
        Value::Integer(value) => u16::try_from(*value).map_err(|_| Error::Invalid)?,
        _ => return Err(Error::Invalid),
    };
    let profile = |fingerprint: &str| {
        crate::smtp_provider_revision::Descriptor::new(&crate::smtp_provider_revision::Profile {
            destination: name,
            relay: field("relay")?,
            port,
            tls: crate::smtp_provider_revision::TlsMode::RequiredStartTls,
            server_name: field("server_name")?,
            trust_reference: &trust_reference,
            trust_fingerprint: fingerprint,
            authentication_identity: auth_field("identity")?,
            credential_reference: &credential_reference,
        })
        .map_err(|_| Error::Invalid)
    };
    profile(&"0".repeat(64))?; // Validate host/profile policy before protected trust reads.
    let trust = crate::trust_snapshot::load(&roots.iter().map(PathBuf::from).collect::<Vec<_>>())
        .map_err(|_| Error::Invalid)?;
    let descriptor = profile(trust.fingerprint())?;
    let delivery = object(delivery).map_err(|_| Error::Invalid)?;
    let permissions = list(
        get(delivery, "smtp_permissions").map_err(|_| Error::Invalid)?,
        0,
        64,
    )
    .map_err(|_| Error::Invalid)?;
    let mut permitted = false;
    for value in permissions {
        let permission = object(value).map_err(|_| Error::Invalid)?;
        let value = |key| {
            get(permission, key)
                .and_then(text)
                .map_err(|_| Error::Invalid)
        };
        permitted |= value("cluster_id")? == cluster
            && value("destination")? == name
            && value("rendering_revision")? == rendering
            && value("provider_revision")? == descriptor.token();
    }
    if !permitted {
        return Err(Error::Permission);
    }
    // Denied/mismatched pairs cannot cause credential environment access.
    let username = lookup(username_ref)?;
    let username = std::str::from_utf8(&username).map_err(|_| Error::Invalid)?;
    if username != auth_field("identity")?
        || crate::credential_policy::valid(Some(username), 64).is_none()
    {
        return Err(Error::Invalid);
    }
    let password = lookup(password_ref)?;
    let password = std::str::from_utf8(&password).map_err(|_| Error::Invalid)?;
    if crate::credential_policy::valid(Some(password), 128).is_none() {
        return Err(Error::Invalid);
    }
    Ok(())
}
