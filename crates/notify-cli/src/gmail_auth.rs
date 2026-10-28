//! Bounded separate credential preflight. This module cannot submit email.
use crate::{
    email_profile::{Profile, Provider},
    email_result::Outcome,
    gmail_credentials::Grant,
};
use std::{
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use zeroize::Zeroizing;
fn identity(profile: &Profile, grant: &Grant, client: &ureq::Agent) -> Result<(), Outcome> {
    let authorization = Zeroizing::new(format!("Bearer {}", grant.access.as_str()));
    let response = match client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .set("Authorization", &authorization)
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::Status(401, _)) => return Err(Outcome::Authentication),
        Err(ureq::Error::Status(403, _)) => return Err(Outcome::Policy),
        Err(ureq::Error::Status(status, response)) => {
            let retry = response.header("Retry-After").map(str::to_owned);
            let bytes = crate::email_http::bounded(response).map_err(|_| Outcome::NotAttempted)?;
            let now = crate::provider_outcome::unix_time().ok_or(Outcome::NotAttempted)?;
            let result = crate::email_result::classify(
                Provider::Gmail,
                status,
                &bytes,
                retry.as_deref(),
                now,
            );
            return Err(if matches!(result, Outcome::Retry(_) | Outcome::Policy) {
                result
            } else {
                Outcome::NotAttempted
            });
        }
        _ => return Err(Outcome::NotAttempted),
    };
    let bytes = crate::email_http::bounded(response).map_err(|_| Outcome::NotAttempted)?;
    let value = crate::config_decode::decode(&bytes).map_err(|_| Outcome::NotAttempted)?;
    use crate::config_validate::{get, object, text};
    let fields = object(&value).map_err(|_| Outcome::NotAttempted)?;
    let field = |key| get(fields, key).ok().and_then(|v| text(v).ok());
    if field("sub") != Some(profile.account.as_str())
        || field("email") != Some(profile.sender.as_str())
        || !matches!(
            get(fields, "email_verified"),
            Ok(crate::config_decode::Value::Bool(true))
        )
    {
        return Err(Outcome::Policy);
    }
    Ok(())
}
fn refresh(grant: &mut Grant, client: &ureq::Agent, now: u64) -> Result<(), Outcome> {
    let body = Zeroizing::new(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "refresh_token"),
                ("client_id", grant.client_id.as_str()),
                ("client_secret", grant.client_secret.as_str()),
                ("refresh_token", grant.refresh.as_str()),
            ])
            .finish(),
    );
    let response = match client
        .post("https://oauth2.googleapis.com/token")
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_bytes(body.as_bytes())
    {
        Ok(r) => r,
        Err(ureq::Error::Status(status, r)) => {
            let retry = r.header("Retry-After").map(str::to_owned);
            let bytes = crate::email_http::bounded(r).map_err(|_| Outcome::NotAttempted)?;
            // Only categorical OAuth rejection is retained, never the response body.
            let value = crate::config_decode::decode(&bytes).ok();
            let permanent = value
                .as_ref()
                .and_then(|v| crate::config_validate::object(v).ok())
                .and_then(|p| crate::config_validate::get(p, "error").ok())
                .and_then(|v| crate::config_validate::text(v).ok())
                .is_some_and(|s| {
                    matches!(
                        s,
                        "invalid_grant"
                            | "invalid_client"
                            | "unauthorized_client"
                            | "invalid_scope"
                            | "access_denied"
                    )
                });
            return Err(if permanent || matches!(status, 401 | 403) {
                Outcome::Authentication
            } else {
                let result = crate::email_result::classify(
                    Provider::Gmail,
                    status,
                    &bytes,
                    retry.as_deref(),
                    now as i64,
                );
                if matches!(result, Outcome::Retry(_) | Outcome::Policy) {
                    result
                } else {
                    Outcome::NotAttempted
                }
            });
        }
        _ => return Err(Outcome::NotAttempted),
    };
    let bytes = crate::email_http::bounded(response).map_err(|_| Outcome::NotAttempted)?;
    grant.update(&bytes, now).map_err(|_| Outcome::Policy)
}
// Fixture injection exists only here; the production caller constructs fixed TLS endpoints.
pub(super) fn ensure(
    profile: &Profile,
    lease: &crate::gmail_store::Lease,
    client: &ureq::Agent,
    now: u64,
) -> Outcome {
    let mut original = match lease.read() {
        Ok(b) => b,
        Err(()) => return Outcome::NotAttempted,
    };
    let mut grant = match Grant::parse(&original) {
        Ok(g) if g.matches(profile) => g,
        _ => return Outcome::Policy,
    };
    let publish = |grant: &Grant, original: &mut Zeroizing<Vec<u8>>| -> Result<(), Outcome> {
        let bytes = grant.encode().map_err(|_| Outcome::Policy)?;
        lease
            .replace(original, &bytes)
            .map_err(|_| Outcome::NotAttempted)?;
        *original = bytes;
        Ok(())
    };
    let mut changed = false;
    if !grant.usable(now, profile.timeout.as_secs()) {
        if let Err(outcome) = refresh(&mut grant, client, now) {
            return outcome;
        }
        if let Err(outcome) = publish(&grant, &mut original) {
            return outcome;
        }
        changed = true;
    }
    match identity(profile, &grant, client) {
        Err(Outcome::Authentication) if !changed => {
            if let Err(outcome) = refresh(&mut grant, client, now) {
                return outcome;
            }
            if let Err(outcome) = publish(&grant, &mut original) {
                return outcome;
            }
            if let Err(outcome) = identity(profile, &grant, client) {
                return outcome;
            }
        }
        Err(outcome) => return outcome,
        Ok(()) => {}
    }
    if !grant.usable(now, profile.timeout.as_secs()) {
        return Outcome::NotAttempted;
    }
    Outcome::Accepted
}
pub(super) fn local(path: &Path, logout: bool) -> Result<String, ()> {
    let bytes = crate::config_snapshot::read_limit(path, 16384).map_err(|_| ())?;
    let profile = Profile::parse(bytes.bytes())?;
    if profile.provider != Provider::Gmail || profile.client_id.is_empty() {
        return Err(());
    }
    let lease = if logout {
        Some(
            crate::gmail_store::Lease::acquire(Path::new(profile.credential.as_str()))
                .map_err(|_| ())?,
        )
    } else {
        None
    };
    let bytes = if let Some(lease) = &lease {
        lease.read()?
    } else {
        Zeroizing::new(
            crate::config_snapshot::read_limit(Path::new(profile.credential.as_str()), 16384)
                .map_err(|_| ())?
                .bytes()
                .to_vec(),
        )
    };
    let grant = Grant::parse(&bytes)?;
    if !grant.matches(&profile) {
        return Err(());
    }
    if let Some(lease) = lease {
        lease.remove(&bytes)?;
        Ok("Local Gmail grant removed. Google consent was not revoked; revoke it separately in your Google account. No email was sent.\n".into())
    } else {
        let now = crate::provider_outcome::unix_time().ok_or(())? as u64;
        Ok(format!("Gmail credential snapshot: bound=true expiring={} live_identity=not_checked. No provider request was made.\n",!grant.usable(now,profile.timeout.as_secs())))
    }
}

const LIMIT: usize = 83000;
pub(super) fn serve(
    input: impl Read,
    mut output: impl Write,
) -> Result<(), crate::provider_frame::Error> {
    use crate::provider_frame::Error;
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Io)?;
    if bytes.len() > LIMIT {
        return Err(Error::InvalidInput);
    }
    let mut remaining = bytes.as_slice();
    let mut fields = Vec::new();
    for _ in 0..2 {
        let n = u32::from_be_bytes(
            remaining
                .get(..4)
                .ok_or(Error::InvalidInput)?
                .try_into()
                .map_err(|_| Error::InvalidInput)?,
        ) as usize;
        remaining = &remaining[4..];
        fields.push(remaining.get(..n).ok_or(Error::InvalidInput)?);
        remaining = &remaining[n..];
    }
    if !remaining.is_empty() || fields[0].len() > 16384 || fields[1].len() > 65664 {
        return Err(Error::InvalidInput);
    }
    let profile = Profile::parse(fields[0]).map_err(|_| Error::InvalidInput)?;
    if profile.provider != Provider::Gmail || profile.client_id.is_empty() {
        return Err(Error::InvalidInput);
    }
    let mut roots = Vec::new();
    let mut remaining = fields[1];
    while !remaining.is_empty() {
        let n = u32::from_be_bytes(
            remaining
                .get(..4)
                .ok_or(Error::InvalidInput)?
                .try_into()
                .map_err(|_| Error::InvalidInput)?,
        ) as usize;
        remaining = &remaining[4..];
        roots.push(remaining.get(..n).ok_or(Error::InvalidInput)?);
        remaining = &remaining[n..];
        if roots.len() > 32 {
            return Err(Error::InvalidInput);
        }
    }
    if roots.is_empty() != profile.roots.is_empty() {
        return Err(Error::InvalidInput);
    }
    let result = match crate::gmail_store::Lease::acquire(Path::new(profile.credential.as_str())) {
        Ok(lease) => {
            let client = crate::email_http::agent(profile.timeout, &roots)
                .map_err(|_| Error::InvalidInput)?;
            ensure(
                &profile,
                &lease,
                &client,
                crate::provider_outcome::unix_time().ok_or(Error::InvalidInput)? as u64,
            )
        }
        Err(_) => Outcome::NotAttempted,
    };
    output
        .write_all(result.encode().as_bytes())
        .map_err(|_| Error::Io)?;
    output.flush().map_err(|_| Error::Io)
}
pub(super) fn run(path: &Path) -> Result<Outcome, ()> {
    let bytes = crate::config_snapshot::read_limit(path, 16384).map_err(|_| ())?;
    let profile = Profile::parse(bytes.bytes())?;
    if profile.provider != Provider::Gmail || profile.client_id.is_empty() {
        return Err(());
    }
    let trust = if profile.roots.is_empty() {
        None
    } else {
        Some(crate::trust_snapshot::load(&profile.roots).map_err(|_| ())?)
    };
    run_profile(&profile, trust.as_ref())
}
pub(super) fn run_profile(
    profile: &Profile,
    trust: Option<&crate::trust_snapshot::Trust>,
) -> Result<Outcome, ()> {
    let bytes = profile.encode()?;
    let roots = trust
        .map(|t| crate::provider_frame::frame(&t.roots(), 65664))
        .transpose()
        .map_err(|_| ())?
        .unwrap_or_default();
    let input = Zeroizing::new(
        crate::provider_frame::frame(&[bytes.as_slice(), &roots], LIMIT).map_err(|_| ())?,
    );
    let executable = std::env::current_exe().map_err(|_| ())?;
    let mut command = std::process::Command::new(executable);
    command.arg("--internal-gmail-auth-v1");
    crate::transport::supervise_with(
        &mut command,
        input,
        Duration::from_secs(profile.timeout.as_secs()),
        LIMIT,
        64,
        Outcome::decode,
    )
    .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    fn fixture(
        expires: u64,
    ) -> (
        crate::storage::tests::Directory,
        crate::email_profile::Profile,
        crate::gmail_store::Lease,
    ) {
        use std::os::unix::fs::PermissionsExt;
        let d = crate::storage::tests::Directory::new();
        let path = d
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("gmail.json");
        let v = serde_json::json!({"schema_version":1,"client_id":"desktop-fixture","client_secret":"fake-secret","subject":"fixture-account","email":"s@fixture.invalid","refresh_token":"old-refresh","access_token":"old-access","expires_at":expires,"scopes":[crate::gmail_credentials::SEND,"openid","email"]});
        std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut v = crate::email_profile::tests::fixture("gmail_api");
        v["oauth_client_id"] = serde_json::json!("desktop-fixture");
        v["credential_file"] = serde_json::json!(path);
        let p = crate::email_profile::Profile::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
        let lease = crate::gmail_store::Lease::acquire(std::path::Path::new(p.credential.as_str()))
            .unwrap();
        (d, p, lease)
    }
    fn identity() -> Vec<u8> {
        br#"{"sub":"fixture-account","email":"s@fixture.invalid","email_verified":true}"#.to_vec()
    }
    #[test]
    fn controlled_refresh_rotation_is_persisted_and_never_submits_email() {
        for rotate in [false, true] {
            let (_d, p, lease) = fixture(0);
            let mut response = serde_json::json!({"token_type":"Bearer","access_token":"new-access","expires_in":3600});
            if rotate {
                response["refresh_token"] = serde_json::json!("new-refresh");
            }
            let (agent, server) = crate::email_http::acceptance::fixture(vec![
                (200, serde_json::to_vec(&response).unwrap(), ""),
                (200, identity(), ""),
            ]);
            assert_eq!(
                super::ensure(&p, &lease, &agent, 100),
                crate::email_result::Outcome::Accepted
            );
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[0].0.starts_with("POST /token "));
            assert!(String::from_utf8_lossy(&requests[0].1).contains("grant_type=refresh_token"));
            assert!(requests[1].0.starts_with("GET /v1/userinfo "));
            assert!(!requests.iter().any(|(h, _)| h.contains("messages/send")));
            let saved = crate::gmail_credentials::Grant::parse(&lease.read().unwrap()).unwrap();
            assert_eq!(
                saved.refresh.as_str(),
                if rotate { "new-refresh" } else { "old-refresh" }
            );
            assert_eq!(saved.expires, 3700);
            let (agent, server) =
                crate::email_http::acceptance::fixture(vec![(200, identity(), "")]);
            assert_eq!(
                super::ensure(&p, &lease, &agent, 101),
                crate::email_result::Outcome::Accepted
            );
            assert_eq!(server.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn revoked_grants_identity_mismatch_and_fresh_token_rejection_are_bounded() {
        let (_d, p, lease) = fixture(0);
        let original = lease.read().unwrap();
        let (agent, server) = crate::email_http::acceptance::fixture(vec![(
            400,
            br#"{"error":"invalid_grant","error_description":"private fixture"}"#.to_vec(),
            "",
        )]);
        assert_eq!(
            super::ensure(&p, &lease, &agent, 100),
            crate::email_result::Outcome::Authentication
        );
        assert_eq!(server.join().unwrap().len(), 1);
        assert_eq!(original.as_slice(), lease.read().unwrap().as_slice());
        let (_d, p, lease) = fixture(3700);
        let (agent, server) = crate::email_http::acceptance::fixture(vec![(
            200,
            br#"{"sub":"another-account","email":"s@fixture.invalid","email_verified":true}"#
                .to_vec(),
            "",
        )]);
        assert_eq!(
            super::ensure(&p, &lease, &agent, 100),
            crate::email_result::Outcome::Policy
        );
        assert_eq!(server.join().unwrap().len(), 1);
        let (agent,server)=crate::email_http::acceptance::fixture(vec![(401,b"".to_vec(),""),(200,br#"{"token_type":"Bearer","access_token":"fresh","refresh_token":"rotated","expires_in":3600}"#.to_vec(),""),(401,b"".to_vec(),"")]);
        assert_eq!(
            super::ensure(&p, &lease, &agent, 100),
            crate::email_result::Outcome::Authentication
        );
        assert_eq!(server.join().unwrap().len(), 3);
        assert_eq!(
            crate::gmail_credentials::Grant::parse(&lease.read().unwrap())
                .unwrap()
                .refresh
                .as_str(),
            "rotated"
        );
    }
}
