//! Owned API-email resources and enrolled revisions. Credentials are resolved last.
use crate::{
    config_decode::Value,
    config_validate::{get, object, text},
    email_profile::Profile,
    email_result::Outcome,
    supervised_provider::Diagnostic,
};
use std::{io::Read, process::Command, time::Duration};
use zeroize::Zeroizing;
pub(super) struct Resources {
    pub(super) profile: Profile,
    trust: Option<crate::trust_snapshot::Trust>,
    provider_revision: String,
    rendering_revision: String,
    revision: String,
    approved: bool,
}
pub(super) struct Request {
    frame: Zeroizing<Vec<u8>>,
    timeout: Duration,
}
impl Resources {
    pub(super) fn new(
        value: &Value,
        delivery: &Value,
        cluster: &str,
        alias: &str,
    ) -> Result<Self, ()> {
        let profile = Profile::from_monitor(value)?;
        profile.encode()?;
        let trust = if profile.roots.is_empty() {
            None
        } else {
            Some(crate::trust_snapshot::load(&profile.roots).map_err(|_| ())?)
        };
        let provider_revision = crate::config_reference::reference(
            b"api-email-provider-v1\0",
            &[
                &profile.revision(),
                trust
                    .as_ref()
                    .map(|t| t.fingerprint())
                    .unwrap_or("bundled-public-roots-v1"),
            ],
        );
        let rendering_revision = crate::config_reference::reference(
            b"api-email-content-v1\0",
            &["state,job_identifier,cluster_id,observed_at,event_id"],
        );
        let revision = crate::config_reference::reference(
            b"api-email-route-v1\0",
            &[&provider_revision, &rendering_revision],
        );
        let fields = object(delivery).map_err(|_| ())?;
        let permissions = crate::config_validate::list(
            get(fields, "api_email_permissions").map_err(|_| ())?,
            0,
            64,
        )
        .map_err(|_| ())?;
        let approved = permissions.iter().any(|v| {
            let Ok(p) = object(v) else {
                return false;
            };
            let field = |key| get(p, key).ok().and_then(|v| text(v).ok());
            field("cluster_id") == Some(cluster)
                && field("destination") == Some(alias)
                && field("rendering_revision") == Some(rendering_revision.as_str())
                && field("provider_revision") == Some(provider_revision.as_str())
        });
        Ok(Self {
            profile,
            trust,
            provider_revision,
            rendering_revision,
            revision,
            approved,
        })
    }
    pub(super) fn approved(&self) -> bool {
        self.approved
    }
    pub(super) fn revisions(&self) -> (&str, &str) {
        (&self.rendering_revision, &self.provider_revision)
    }
    pub(super) fn revision(&self) -> &str {
        &self.revision
    }
    pub(super) fn wire_payload(&self, payload: &[u8]) -> Result<Vec<u8>, ()> {
        let content = crate::email_content::Content::from_event(payload)?;
        if payload.len() > self.profile.payload_limit {
            return Err(());
        }
        Ok(crate::email_wire::prepare(&self.profile, &content)?
            .body
            .to_vec())
    }
    pub(super) fn prepare(
        &self,
        store: &crate::storage::Store,
        item: &crate::storage::DueDelivery,
        lookup: impl FnOnce(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    ) -> Result<Request, Diagnostic> {
        if !self.approved {
            return Err(Diagnostic::EmailPermission);
        }
        let frozen = store
            .email_delivery(
                &item.event_id,
                &item.destination,
                &self.revision,
                &self.profile,
            )
            .map_err(|_| Diagnostic::InvalidInput)?;
        let public = store
            .public_delivery(&item.event_id, &item.destination, &self.revision)
            .map_err(|_| Diagnostic::InvalidInput)?
            .ok_or(Diagnostic::InvalidInput)?;
        let credential =
            lookup(&self.profile.credential, 4096).map_err(|_| Diagnostic::EmailCredentials)?;
        if !valid_key(&credential) {
            return Err(Diagnostic::EmailCredentials);
        }
        self.request(&credential, &public.payload, &frozen)
            .map_err(|_| Diagnostic::InvalidInput)
    }
    pub(super) fn synthetic(
        &self,
        id: &str,
        now: u64,
        lookup: impl FnOnce(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    ) -> Result<Request, Diagnostic> {
        if !self.approved {
            return Err(Diagnostic::EmailPermission);
        }
        let content = crate::email_content::Content::synthetic(id, now)
            .map_err(|_| Diagnostic::InvalidInput)?;
        let wire = crate::email_wire::prepare(&self.profile, &content)
            .map_err(|_| Diagnostic::InvalidInput)?;
        let payload=serde_json::to_vec(&serde_json::json!({"schema_version":"1.0","event_type":"TEST","event_id":id,"email_test_timestamp":now})).map_err(|_|Diagnostic::InvalidInput)?;
        let credential =
            lookup(&self.profile.credential, 4096).map_err(|_| Diagnostic::EmailCredentials)?;
        if !valid_key(&credential) {
            return Err(Diagnostic::EmailCredentials);
        }
        self.request(&credential, &payload, &wire)
            .map_err(|_| Diagnostic::InvalidInput)
    }
    fn request(
        &self,
        credential: &[u8],
        payload: &[u8],
        wire: &crate::email_wire::Request,
    ) -> Result<Request, ()> {
        let profile = self.profile.encode()?;
        let roots = self
            .trust
            .as_ref()
            .map(|t| crate::provider_frame::frame(&t.roots(), 65664))
            .transpose()
            .map_err(|_| ())?
            .unwrap_or_default();
        let frame = crate::provider_frame::frame(
            &[
                &profile,
                credential,
                payload,
                &wire.body,
                wire.content_type.as_bytes(),
                &roots,
            ],
            INPUT_LIMIT,
        )
        .map_err(|_| ())?;
        Ok(Request {
            frame: Zeroizing::new(frame),
            timeout: self.profile.timeout,
        })
    }
}
fn valid_key(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len() <= 4096 && bytes.iter().all(|b| (33..=126).contains(b))
}
pub(super) const INPUT_LIMIT: usize = 132000;
impl Request {
    pub(super) fn supervise(self, command: &mut Command) -> crate::supervised_provider::Report {
        // Keep the selected owned executable, with a distinct private email protocol.
        let mut child = Command::new(command.get_program());
        child.arg("--internal-api-email-v1");
        let result = crate::transport::supervise_with(
            &mut child,
            self.frame,
            self.timeout,
            INPUT_LIMIT,
            64,
            Outcome::decode,
        );
        let result = match result {
            Ok(Outcome::Accepted) => Ok(crate::provider_outcome::ProviderOutcome::Delivered),
            Ok(Outcome::Rejected) => Ok(crate::provider_outcome::ProviderOutcome::Rejected),
            Ok(Outcome::Retry(n)) => Ok(crate::provider_outcome::ProviderOutcome::RetryAfter(
                n as i64,
            )),
            Ok(Outcome::Authentication) => {
                Ok(crate::provider_outcome::ProviderOutcome::PauseAuthentication)
            }
            Ok(Outcome::Policy) => Ok(crate::provider_outcome::ProviderOutcome::PausePolicy),
            Ok(Outcome::NotAttempted) => {
                return crate::supervised_provider::Report {
                    outcome: None,
                    diagnostic: Diagnostic::EmailCredentials,
                    acceptance: crate::supervised_provider::Acceptance::NotAttempted,
                }
            }
            Ok(Outcome::Unknown) => Ok(crate::provider_outcome::ProviderOutcome::Retry),
            Err(e) => Err(e),
        };
        crate::supervised_provider::classify(result)
    }
}

pub(super) fn serve(
    input: impl std::io::Read,
    mut output: impl std::io::Write,
) -> Result<(), crate::provider_frame::Error> {
    use crate::provider_frame::Error;
    let mut bytes = Zeroizing::new(Vec::new());
    input
        .take((INPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidInput)?;
    if bytes.len() > INPUT_LIMIT {
        return Err(Error::InvalidInput);
    }
    let mut remaining = bytes.as_slice();
    let mut fields = Vec::new();
    for _ in 0..6 {
        let prefix: [u8; 4] = remaining
            .get(..4)
            .ok_or(Error::InvalidInput)?
            .try_into()
            .map_err(|_| Error::InvalidInput)?;
        remaining = &remaining[4..];
        let len = u32::from_be_bytes(prefix) as usize;
        fields.push(remaining.get(..len).ok_or(Error::InvalidInput)?);
        remaining = &remaining[len..];
    }
    if !remaining.is_empty()
        || fields[0].len() > 16384
        || fields[1].len() > 4096
        || fields[2].len() > 16384
        || fields[3].len() > 16384
        || fields[4].len() > 200
        || fields[5].len() > 65664
    {
        return Err(Error::InvalidInput);
    }
    let profile = Profile::parse(fields[0]).map_err(|_| Error::InvalidInput)?;
    if profile.provider == crate::email_profile::Provider::Gmail || !valid_key(fields[1]) {
        return Err(Error::InvalidInput);
    }
    crate::config_decode::decode(fields[2]).map_err(|_| Error::InvalidInput)?;
    let payload: serde_json::Value =
        serde_json::from_slice(fields[2]).map_err(|_| Error::InvalidInput)?;
    let content = if payload["event_type"] == "TEST" {
        if payload.as_object().is_none_or(|v| v.len() != 4) || payload["schema_version"] != "1.0" {
            return Err(Error::InvalidInput);
        }
        crate::email_content::Content::synthetic(
            payload["event_id"].as_str().ok_or(Error::InvalidInput)?,
            payload["email_test_timestamp"]
                .as_u64()
                .ok_or(Error::InvalidInput)?,
        )
    } else {
        crate::email_content::Content::from_event(fields[2])
    }
    .map_err(|_| Error::InvalidInput)?;
    let wire = crate::email_wire::prepare(&profile, &content).map_err(|_| Error::InvalidInput)?;
    if wire.body.as_slice() != fields[3] || wire.content_type.as_bytes() != fields[4] {
        return Err(Error::InvalidInput);
    }
    let mut roots = Vec::new();
    let mut remaining = fields[5];
    while !remaining.is_empty() {
        let prefix: [u8; 4] = remaining
            .get(..4)
            .ok_or(Error::InvalidInput)?
            .try_into()
            .map_err(|_| Error::InvalidInput)?;
        remaining = &remaining[4..];
        let len = u32::from_be_bytes(prefix) as usize;
        roots.push(remaining.get(..len).ok_or(Error::InvalidInput)?);
        remaining = &remaining[len..];
        if roots.len() > 32 {
            return Err(Error::InvalidInput);
        }
    }
    if profile.roots.is_empty() != roots.is_empty() {
        return Err(Error::InvalidInput);
    }
    let agent =
        crate::email_http::agent(profile.timeout, &roots).map_err(|_| Error::InvalidInput)?;
    let now = crate::provider_outcome::unix_time().ok_or(Error::InvalidInput)?;
    let result = crate::email_http::send(&profile, fields[1], &wire, &agent, now);
    output
        .write_all(result.encode().as_bytes())
        .map_err(|_| Error::Io)?;
    output.flush().map_err(|_| Error::Io)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    fn fixture(kind: &str) -> (Value, Value) {
        let mut profile = crate::email_profile::tests::fixture(kind);
        profile.as_object_mut().unwrap().remove("schema_version");
        profile["type"] = profile.as_object_mut().unwrap().remove("provider").unwrap();
        let value = crate::config_decode::decode(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let delivery = crate::config_decode::decode(br#"{"api_email_permissions":[]}"#).unwrap();
        (value, delivery)
    }
    fn enrolled(kind: &str) -> Resources {
        let (value, delivery) = fixture(kind);
        let r = Resources::new(&value, &delivery, "cluster", "email").unwrap();
        let delivery = serde_json::json!({"api_email_permissions":[{"cluster_id":"cluster","destination":"email","rendering_revision":r.rendering_revision,"provider_revision":r.provider_revision}]});
        let delivery =
            crate::config_decode::decode(&serde_json::to_vec(&delivery).unwrap()).unwrap();
        Resources::new(&value, &delivery, "cluster", "email").unwrap()
    }
    #[test]
    fn enrollment_precedes_secrets_and_profile_round_trip_preserves_revision() {
        for kind in ["sendgrid", "mailgun"] {
            let (value, delivery) = fixture(kind);
            let r = Resources::new(&value, &delivery, "cluster", "email").unwrap();
            assert!(!r.approved());
            assert!(matches!(
                r.synthetic("evt_00000000000040008000000000000000", 0, |_, _| panic!(
                    "secret lookup before approval"
                )),
                Err(Diagnostic::EmailPermission)
            ));
            let r = enrolled(kind);
            assert!(r.approved());
            assert_eq!(
                Profile::parse(&r.profile.encode().unwrap())
                    .unwrap()
                    .revision(),
                r.profile.revision()
            );
            let mut called = 0;
            assert!(matches!(
                r.synthetic("evt_00000000000040008000000000000000", 0, |_, _| {
                    called += 1;
                    Err(())
                }),
                Err(Diagnostic::EmailCredentials)
            ));
            assert_eq!(called, 1);
            for invalid in [vec![], vec![b'x'; 4097], b"key\n".to_vec()] {
                assert!(matches!(
                    r.synthetic("evt_00000000000040008000000000000000", 0, |_, _| Ok(
                        Zeroizing::new(invalid)
                    )),
                    Err(Diagnostic::EmailCredentials)
                ));
            }
            let request = r
                .synthetic("evt_00000000000040008000000000000000", 0, |_, _| {
                    Ok(Zeroizing::new(b"fake-key".to_vec()))
                })
                .unwrap();
            // Corrupt the canonical/wire binding before the helper can connect.
            let mut corrupt = request.frame.to_vec();
            let index = corrupt.windows(8).position(|b| b == b"Slurm No").unwrap();
            corrupt[index] = b'X';
            assert!(serve(corrupt.as_slice(), Vec::new()).is_err());
            for malformed in [&b""[..], &request.frame[..request.frame.len() - 1]] {
                assert!(serve(malformed, Vec::new()).is_err());
            }
            let mut trailing = request.frame.to_vec();
            trailing.push(0);
            assert!(serve(trailing.as_slice(), Vec::new()).is_err());
        }
        assert!(Profile::from_monitor(&fixture("gmail_api").0).is_err());
    }
    #[test]
    fn frozen_monitor_request_survives_reopen_and_checks_profile_before_key() {
        for kind in ["sendgrid", "mailgun"] {
            let r = enrolled(kind);
            let d = crate::storage::tests::Directory::new();
            let mut store =
                crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
            let mut candidate = crate::lifecycle::Reconciler::with_capacity(10);
            let events = candidate
                .observe_snapshot(
                    "",
                    "cluster|123_2|alice|s|a|COMPLETED|0:0",
                    "cluster",
                    "alice",
                    true,
                )
                .unwrap();
            store
                .persist_public_webhooks(&candidate, &events, ("cluster", "window", 0), |_| {
                    vec![crate::storage::PublicRoute {
                        destination: "email",
                        revision: r.revision(),
                        slack: None,
                        email: Some(&r.profile),
                    }]
                })
                .unwrap();
            let item = store.due_deliveries("cluster", 0, 10).unwrap().remove(0);
            let request = r
                .prepare(&store, &item, |_, _| {
                    Ok(Zeroizing::new(b"fake-key".to_vec()))
                })
                .unwrap();
            drop(store);
            let store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
            assert_eq!(
                request.frame.as_slice(),
                r.prepare(&store, &item, |_, _| Ok(Zeroizing::new(
                    b"fake-key".to_vec()
                )))
                .unwrap()
                .frame
                .as_slice()
            );
            let mut changed = enrolled(kind);
            changed.revision = "a".repeat(64);
            assert!(matches!(
                changed.prepare(&store, &item, |_, _| panic!("lookup before frozen guard")),
                Err(Diagnostic::InvalidInput)
            ));
            assert!(matches!(
                r.prepare(&store, &item, |_, _| Err(())),
                Err(Diagnostic::EmailCredentials)
            ));
        }
    }
    #[test]
    fn disclosure_changes_invalidate_enrollment_without_credentials() {
        let r = enrolled("mailgun");
        let delivery = serde_json::json!({"api_email_permissions":[{"cluster_id":"cluster","destination":"email","rendering_revision":r.rendering_revision,"provider_revision":r.provider_revision}]});
        let delivery =
            crate::config_decode::decode(&serde_json::to_vec(&delivery).unwrap()).unwrap();
        for (key, replacement) in [
            ("recipient", "other@fixture.invalid"),
            ("sender", "other@fixture.invalid"),
            ("domain", "other.fixture.invalid"),
            ("region", "us"),
            ("account_identity", "another-account"),
            ("api_key_env", "OTHER_KEY"),
        ] {
            let (Value::Object(mut fields), _) = fixture("mailgun") else {
                panic!()
            };
            fields
                .iter_mut()
                .find(|(name, _)| name.as_str() == key)
                .unwrap()
                .1 = Value::Text(Zeroizing::new(replacement.into()));
            let changed =
                Resources::new(&Value::Object(fields), &delivery, "cluster", "email").unwrap();
            assert!(!changed.approved(), "{key}");
        }
    }
}
