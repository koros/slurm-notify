//! Fixed-endpoint verified HTTPS email submission, called only in a supervised child.
use crate::{
    config_decode::Value,
    config_validate::{get, object, text},
    email_profile::{Profile, Provider},
    email_result::{self, Outcome},
    email_wire::Request,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{io::Read, time::Duration};
use zeroize::Zeroizing;
pub(super) fn agent(timeout: Duration, roots: &[&[u8]]) -> Result<ureq::Agent, ()> {
    let builder = ureq::AgentBuilder::new()
        .timeout(timeout)
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .timeout_write(timeout)
        .redirects(0)
        .try_proxy_from_env(false);
    let builder = if roots.is_empty() {
        builder
    } else {
        crate::smtp_provider_revision::trust_fingerprint(roots).map_err(|_| ())?;
        let mut store = rustls::RootCertStore::empty();
        for root in roots {
            store
                .add(rustls::pki_types::CertificateDer::from(root.to_vec()))
                .map_err(|_| ())?;
        }
        builder.tls_config(std::sync::Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(store)
                .with_no_client_auth(),
        ))
    };
    Ok(builder.build())
}
pub(super) fn bounded(response: ureq::Response) -> Result<Zeroizing<Vec<u8>>, ()> {
    if response
        .header("Content-Encoding")
        .is_some_and(|v| v != "identity")
    {
        return Err(());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    response
        .into_reader()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 8192 {
        return Err(());
    }
    Ok(bytes)
}
fn token(bytes: &[u8]) -> Result<&str, ()> {
    let s = std::str::from_utf8(bytes).map_err(|_| ())?;
    if s.is_empty() || s.len() > 4096 || !s.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(());
    }
    Ok(s)
}
fn gmail_token(profile: &Profile, credential: &[u8], now: i64) -> Result<Zeroizing<String>, ()> {
    if credential.len() > 16384 || now < 0 {
        return Err(());
    }
    if !profile.client_id.is_empty() {
        let grant = crate::gmail_credentials::Grant::parse(credential)?;
        if !grant.matches(profile) || !grant.usable(now as u64, profile.timeout.as_secs()) {
            return Err(());
        }
        return Ok(grant.access);
    }
    let value = crate::config_decode::decode(credential).map_err(|_| ())?;
    let values = object(&value).map_err(|_| ())?;
    if values.len() != 2 {
        return Err(());
    }
    let t = text(get(values, "access_token").map_err(|_| ())?).map_err(|_| ())?;
    token(t.as_bytes())?;
    let expiry = match get(values, "expires_at") {
        Ok(Value::Integer(n)) => *n,
        _ => return Err(()),
    };
    if expiry > 4_102_444_799
        || expiry
            <= (now as u64)
                .saturating_add(profile.timeout.as_secs())
                .saturating_add(30)
    {
        return Err(());
    }
    Ok(Zeroizing::new(t.into()))
}
// Kept injectable only at the private HTTP-client boundary for controlled TLS tests.
// The request targets always come from the validated fixed provider profile.
pub(super) fn send(
    profile: &Profile,
    credential: &[u8],
    request: &Request,
    client: &ureq::Agent,
    now: i64,
) -> Outcome {
    let auth = match profile.provider {
        Provider::SendGrid => match token(credential) {
            Ok(t) => Zeroizing::new(format!("Bearer {t}")),
            Err(()) => return Outcome::NotAttempted,
        },
        Provider::Mailgun => match token(credential) {
            Ok(t) => {
                let encoded =
                    Zeroizing::new(STANDARD.encode(Zeroizing::new(format!("api:{t}")).as_bytes()));
                Zeroizing::new(format!("Basic {}", encoded.as_str()))
            }
            Err(()) => return Outcome::NotAttempted,
        },
        Provider::Gmail => {
            let t = match gmail_token(profile, credential, now) {
                Ok(t) => t,
                Err(()) => return Outcome::NotAttempted,
            };
            let auth = Zeroizing::new(format!("Bearer {}", t.as_str()));
            let response = match client
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .set("Authorization", &auth)
                .call()
            {
                Ok(r) => r,
                Err(ureq::Error::Status(401, _)) => return Outcome::Authentication,
                Err(ureq::Error::Status(403, _)) => return Outcome::Policy,
                _ => return Outcome::NotAttempted,
            };
            let bytes = match bounded(response) {
                Ok(b) => b,
                Err(()) => return Outcome::NotAttempted,
            };
            let valid = crate::config_decode::decode(&bytes).ok().is_some_and(|v| {
                let Ok(values) = object(&v) else {
                    return false;
                };
                let field = |key| get(values, key).ok().and_then(|v| text(v).ok());
                field("sub") == Some(profile.account.as_str())
                    && field("email") == Some(profile.sender.as_str())
                    && matches!(get(values, "email_verified"), Ok(Value::Bool(true)))
            });
            if !valid {
                return Outcome::NotAttempted;
            }
            auth
        }
    };
    submit(profile, &auth, request, client, now)
}
// Only the typed monitoring helper uses this after separate account preflight.
// Its entire dispatch is one submission; no token or identity requests are counted.
pub(super) fn send_gmail_checked(
    profile: &Profile,
    credential: &[u8],
    request: &Request,
    client: &ureq::Agent,
    now: i64,
) -> Outcome {
    if profile.provider != Provider::Gmail || profile.client_id.is_empty() {
        return Outcome::NotAttempted;
    }
    let token = match token(credential) {
        Ok(t) => t,
        Err(()) => return Outcome::NotAttempted,
    };
    let auth = Zeroizing::new(format!("Bearer {token}"));
    submit(profile, &auth, request, client, now)
}
fn submit(
    profile: &Profile,
    auth: &str,
    request: &Request,
    client: &ureq::Agent,
    now: i64,
) -> Outcome {
    if request.body.len() > profile.payload_limit {
        return Outcome::NotAttempted;
    }
    let response = match client
        .post(&profile.endpoint())
        .set("Content-Type", &request.content_type)
        .set("Authorization", auth)
        .send_bytes(&request.body)
    {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(_) => return Outcome::Unknown,
    };
    let status = response.status();
    let retry = response.header("Retry-After").map(str::to_owned);
    if status == 401 {
        return Outcome::Authentication;
    }
    if status == 403 && profile.provider != Provider::Gmail {
        return Outcome::Policy;
    }
    if profile.provider == Provider::SendGrid && status == 202 {
        return Outcome::Accepted;
    }
    let body = match bounded(response) {
        Ok(b) => b,
        Err(()) => {
            return if status == 401 {
                Outcome::Authentication
            } else if status == 403 {
                Outcome::Policy
            } else {
                Outcome::Unknown
            }
        }
    };
    email_result::classify(profile.provider, status, &body, retry.as_deref(), now)
}
#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn credentials_expiry_and_trust_fail_closed() {
        for bad in [b"".as_slice(), b"bad\nkey", &[0xff], &vec![b'x'; 4097]] {
            assert!(token(bad).is_err());
        }
        assert!(agent(Duration::from_secs(1), &[b"not-a-certificate"]).is_err());
        let p = Profile::parse(
            &serde_json::to_vec(&crate::email_profile::tests::fixture("gmail_api")).unwrap(),
        )
        .unwrap();
        assert!(gmail_token(&p, b"{\"access_token\":\"fixture\",\"expires_at\":100}", 0).is_ok());
        for b in [
            b"{}".as_slice(),
            b"{\"access_token\":\"fixture\",\"expires_at\":30}",
            b"{\"access_token\":\"fixture\",\"expires_at\":100,\"extra\":1}",
            b"{\"access_token\":\"bad\\nkey\",\"expires_at\":100}",
        ] {
            assert!(gmail_token(&p, b, 0).is_err());
        }
        let content =
            crate::email_content::Content::synthetic(&crate::public_event::identifier([0; 16]), 0)
                .unwrap();
        let req = crate::email_wire::prepare(&p, &content).unwrap();
        assert_eq!(
            send(
                &p,
                b"{}",
                &req,
                &agent(Duration::from_secs(1), &[]).unwrap(),
                0
            ),
            Outcome::NotAttempted
        );
    }
}

#[cfg(test)]
#[path = "email_http_tests.rs"]
pub(crate) mod acceptance;
