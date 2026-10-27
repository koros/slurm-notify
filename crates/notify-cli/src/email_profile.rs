//! Strict standalone HTTPS-email test profiles; no monitoring config defaults.
use crate::{
    config_decode::Value,
    config_validate::{get, object, text},
};
use std::{path::PathBuf, time::Duration};
use zeroize::Zeroizing;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Provider {
    SendGrid,
    Mailgun,
    Gmail,
}
pub(super) struct Profile {
    pub(super) provider: Provider,
    pub(super) sender: Zeroizing<String>,
    pub(super) recipient: Zeroizing<String>,
    pub(super) account: Zeroizing<String>,
    pub(super) credential: Zeroizing<String>,
    pub(super) domain: Zeroizing<String>,
    pub(super) region: &'static str,
    pub(super) timeout: Duration,
    pub(super) payload_limit: usize,
    pub(super) roots: Vec<PathBuf>,
}
impl Profile {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() > 16384 {
            return Err(());
        }
        let value = crate::config_decode::decode(bytes).map_err(|_| ())?;
        let p = object(&value).map_err(|_| ())?;
        let field = |key| text(get(p, key).map_err(|_| ())?).map_err(|_| ());
        let kind = field("provider")?;
        let provider = match kind {
            "sendgrid" => Provider::SendGrid,
            "mailgun" => Provider::Mailgun,
            "gmail_api" => Provider::Gmail,
            _ => return Err(()),
        };
        let mut required = vec![
            "schema_version",
            "provider",
            "sender",
            "recipient",
            "account_identity",
            "request_timeout_seconds",
            "payload_limit_bytes",
        ];
        required.push(if provider == Provider::Gmail {
            "credential_file"
        } else {
            "api_key_env"
        });
        if provider == Provider::Mailgun {
            required.extend(["domain", "region"]);
        }
        if required.iter().any(|key| get(p, key).is_err())
            || p.iter().any(|(key, _)| {
                !required.contains(&key.as_str()) && key.as_str() != "trust_der_files"
            })
        {
            return Err(());
        }
        if !matches!(get(p, "schema_version"), Ok(Value::Integer(1))) {
            return Err(());
        }
        let sender = field("sender")?;
        let recipient = field("recipient")?;
        crate::smtp_address::address(sender)?;
        crate::smtp_address::address(recipient)?;
        let account = field("account_identity")?;
        if account.is_empty()
            || account.len() > 128
            || !account.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(());
        }
        let credential = field(if provider == Provider::Gmail {
            "credential_file"
        } else {
            "api_key_env"
        })?;
        if provider == Provider::Gmail {
            if !PathBuf::from(credential).is_absolute()
                || credential.len() > 4096
                || credential.chars().any(char::is_control)
            {
                return Err(());
            }
        } else if !crate::credential_resolution::reference(credential) {
            return Err(());
        }
        let domain = if provider == Provider::Mailgun {
            field("domain")?
        } else {
            ""
        };
        if provider == Provider::Mailgun && !valid_domain(domain) {
            return Err(());
        }
        let region = if provider == Provider::Mailgun {
            match field("region")? {
                "us" => "us",
                "eu" => "eu",
                _ => return Err(()),
            }
        } else {
            "us"
        };
        let integer = |key| match get(p, key) {
            Ok(Value::Integer(n)) => Ok(*n),
            _ => Err(()),
        };
        let timeout = integer("request_timeout_seconds")?;
        let limit = integer("payload_limit_bytes")?;
        if !(1..=30).contains(&timeout) || !(1024..=16384).contains(&limit) {
            return Err(());
        }
        let mut roots = Vec::new();
        if let Ok(value) = get(p, "trust_der_files") {
            for v in crate::config_validate::list(value, 1, 32).map_err(|_| ())? {
                let path = text(v).map_err(|_| ())?;
                let path = PathBuf::from(path);
                if !path.is_absolute() || path.as_os_str().len() > 4096 || roots.contains(&path) {
                    return Err(());
                }
                roots.push(path);
            }
        }
        Ok(Self {
            provider,
            sender: Zeroizing::new(sender.into()),
            recipient: Zeroizing::new(recipient.into()),
            account: Zeroizing::new(account.into()),
            credential: Zeroizing::new(credential.into()),
            domain: Zeroizing::new(domain.into()),
            region,
            timeout: Duration::from_secs(timeout),
            payload_limit: limit as usize,
            roots,
        })
    }
    pub(super) fn kind(&self) -> &'static str {
        match self.provider {
            Provider::SendGrid => "sendgrid",
            Provider::Mailgun => "mailgun",
            Provider::Gmail => "gmail_api",
        }
    }
    pub(super) fn endpoint(&self) -> String {
        match self.provider {
            Provider::SendGrid => "https://api.sendgrid.com/v3/mail/send".into(),
            Provider::Mailgun => format!(
                "https://{}/v3/{}/messages",
                if self.region == "eu" {
                    "api.eu.mailgun.net"
                } else {
                    "api.mailgun.net"
                },
                self.domain.as_str()
            ),
            Provider::Gmail => {
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/send".into()
            }
        }
    }
    pub(super) fn revision(&self) -> String {
        let paths = self
            .roots
            .iter()
            .map(|p| p.to_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        crate::config_reference::reference(
            b"api-email-profile-v1\0",
            &[
                self.kind(),
                &self.sender,
                &self.recipient,
                &self.account,
                &self.credential,
                &self.domain,
                self.region,
                &paths,
            ],
        )
    }
}
fn valid_domain(s: &str) -> bool {
    s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn fixture(provider: &str) -> serde_json::Value {
        let mut v = serde_json::json!({"schema_version":1,"provider":provider,"sender":"s@fixture.invalid","recipient":"r@fixture.invalid","account_identity":"fixture-account","request_timeout_seconds":2,"payload_limit_bytes":16384});
        if provider == "gmail_api" {
            v["credential_file"] = serde_json::json!("/protected/google.json");
        } else {
            v["api_key_env"] = serde_json::json!("API_KEY");
        }
        if provider == "mailgun" {
            v["domain"] = serde_json::json!("mg.fixture.invalid");
            v["region"] = serde_json::json!("eu");
        }
        v
    }
    #[test]
    fn exact_profiles_endpoints_and_identity_revisions() {
        for kind in ["sendgrid", "mailgun", "gmail_api"] {
            let v = fixture(kind);
            let p = Profile::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
            assert!(p.endpoint().starts_with("https://"));
            assert_eq!(p.kind(), kind);
            let mut changed = v.clone();
            changed["recipient"] = serde_json::json!("different@fixture.invalid");
            assert_ne!(
                p.revision(),
                Profile::parse(&serde_json::to_vec(&changed).unwrap())
                    .unwrap()
                    .revision()
            );
            for (key, bad) in [
                ("schema_version", serde_json::json!(2)),
                ("sender", serde_json::json!("name <s@fixture.invalid>")),
                ("recipient", serde_json::json!("x\r\nBcc: victim")),
                ("request_timeout_seconds", serde_json::json!(0)),
                ("payload_limit_bytes", serde_json::json!(16385)),
                ("account_identity", serde_json::json!("")),
                ("endpoint", serde_json::json!("https://attacker.invalid")),
                ("trust_der_files", serde_json::json!(["relative"])),
            ] {
                let mut bad_v = v.clone();
                bad_v[key] = bad;
                assert!(Profile::parse(&serde_json::to_vec(&bad_v).unwrap()).is_err());
            }
        }
        let mut v = fixture("mailgun");
        for domain in [
            "../bad",
            "-mg.example",
            "a..org",
            "UPPER.org",
            "x.org?query=1",
        ] {
            v["domain"] = serde_json::json!(domain);
            assert!(Profile::parse(&serde_json::to_vec(&v).unwrap()).is_err());
        }
        assert!(Profile::parse(b"{\"schema_version\":1,\"schema_version\":1}").is_err());
        assert!(Profile::parse(b"{}").is_err());
        assert!(Profile::parse(&vec![b' '; 16385]).is_err());
    }
}
