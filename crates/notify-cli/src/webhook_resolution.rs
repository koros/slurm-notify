//! Private injected webhook resources; no installed environment/config contract.
use crate::{credential_resolution::reference, transport, webhook};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Reference,
    Policy,
    Lookup,
    Endpoint,
    Authorization,
    Request,
}
// No Debug/Clone; keep original URL/header bytes, including secret paths/queries.
pub(super) struct Resources {
    endpoint: Zeroizing<Vec<u8>>,
    authorization: Option<Zeroizing<Vec<u8>>>,
    timeout: Duration,
    payload_limit: usize,
    revision: String,
    public_https: bool,
    trust: Option<crate::trust_snapshot::Trust>,
    slack: bool,
}
// Owned secret-stdin request; constructed only from a checked frozen delivery.
// No Debug/Clone and no endpoint/header/payload command arguments.
pub(super) struct PublicRequest {
    frame: Zeroizing<Vec<u8>>,
    timeout: Duration,
}
impl PublicRequest {
    pub(super) fn supervise(
        self,
        command: &mut std::process::Command,
    ) -> crate::supervised_provider::Report {
        crate::supervised_provider::classify(transport::supervise(
            command,
            self.frame,
            self.timeout,
            20 + 4096 + 4096 + 128 + 16384 + 5 + 4 + 65664,
            64,
        ))
    }
}
impl Resources {
    pub(super) fn with_slack(mut self) -> Self {
        self.revision = crate::config_reference::reference(b"slack-render-v1\0", &[&self.revision]);
        self.slack = true;
        self
    }
    pub(super) fn kind(&self) -> &'static str {
        if self.slack {
            "slack"
        } else {
            "webhook"
        }
    }
    pub(super) fn wire_payload(&self, payload: &[u8]) -> Result<Vec<u8>, Error> {
        let bytes = if self.slack {
            crate::slack_message::render(payload).map_err(|_| Error::Request)?
        } else {
            payload.to_vec()
        };
        if bytes.len() > self.payload_limit {
            return Err(Error::Request);
        }
        Ok(bytes)
    }

    pub(super) fn with_trust(mut self, paths: &[std::path::PathBuf]) -> Result<Self, Error> {
        let trust = crate::trust_snapshot::load(paths).map_err(|_| Error::Policy)?;
        self.revision = crate::config_reference::reference(
            b"webhook-trust\0",
            &[&self.revision, trust.fingerprint()],
        );
        self.trust = Some(trust);
        Ok(self)
    }
    pub(super) fn accepts_public_payload(&self, payload: &[u8]) -> bool {
        self.public_https
            && payload.len() <= self.payload_limit
            && self.wire_payload(payload).is_ok()
    }
    pub(super) fn public_revision(&self) -> Option<&str> {
        self.public_https.then_some(self.revision.as_str())
    }
    pub(super) fn safe_origin(&self) -> Option<String> {
        if !self.public_https {
            return None;
        }
        let endpoint = std::str::from_utf8(&self.endpoint).ok()?;
        Some(
            webhook::endpoint_url(endpoint)
                .ok()?
                .origin()
                .ascii_serialization(),
        )
    }
    pub(super) fn synthetic_request(
        &self,
        id: &str,
        payload: &[u8],
    ) -> Result<PublicRequest, Error> {
        if !crate::public_event::valid_id(id) || !self.accepts_public_payload(payload) {
            return Err(Error::Request);
        }
        Ok(PublicRequest {
            frame: self.frame(id, payload)?,
            timeout: self.timeout,
        })
    }
    pub(super) fn prepare_public(
        &self,
        store: &crate::storage::Store,
        item: &crate::storage::DueDelivery,
    ) -> Result<PublicRequest, crate::supervised_provider::Diagnostic> {
        let invalid = crate::supervised_provider::Diagnostic::InvalidInput;
        let revision = self.public_revision().ok_or(invalid)?;
        let frozen = store
            .public_delivery(&item.event_id, &item.destination, revision)
            .map_err(|_| invalid)?
            .ok_or(invalid)?;
        if !self.accepts_public_payload(&frozen.payload) {
            return Err(invalid);
        }
        Ok(PublicRequest {
            frame: self
                .frame(&frozen.id, &frozen.payload)
                .map_err(|_| invalid)?,
            timeout: self.timeout,
        })
    }
    #[cfg(test)]
    pub(super) fn into_frame(
        self,
        event_id: &str,
        payload: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        self.frame(event_id, payload)
    }
    fn frame(&self, event_id: &str, payload: &[u8]) -> Result<Zeroizing<Vec<u8>>, Error> {
        if event_id.is_empty()
            || event_id.len() > 128
            || !event_id.bytes().all(|byte| (32..=126).contains(&byte))
            || payload.len() > self.payload_limit
        {
            return Err(Error::Request);
        }
        let payload = self.wire_payload(payload)?;
        let millis = self.timeout.as_millis().to_string();
        let mut fields: Vec<&[u8]> = vec![
            &self.endpoint,
            self.authorization
                .as_deref()
                .map(|bytes| bytes.as_slice())
                .unwrap_or_default(),
            event_id.as_bytes(),
            &payload,
            millis.as_bytes(),
        ];
        let roots = self
            .trust
            .as_ref()
            .map(|trust| transport::frame(&trust.roots(), 65664))
            .transpose()
            .map_err(|_| Error::Request)?;
        if let Some(roots) = &roots {
            fields.push(roots);
        }
        transport::frame(&fields, 20 + 4096 + 4096 + 128 + 16384 + 5 + 4 + 65664)
            .map(Zeroizing::new)
            .map_err(|_| Error::Request)
    }
}
pub(super) fn resolve(
    endpoint_reference: &str,
    authorization_reference: Option<&str>,
    timeout: Duration,
    payload_limit: usize,
    mut lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Resources, Error> {
    if !reference(endpoint_reference)
        || authorization_reference.is_some_and(|name| !reference(name))
    {
        return Err(Error::Reference);
    }
    if !(1..=30).contains(&timeout.as_secs())
        || timeout.subsec_nanos() != 0
        || !(1024..=16384).contains(&payload_limit)
    {
        return Err(Error::Policy);
    }
    let endpoint = lookup(endpoint_reference, 4096).map_err(|_| Error::Lookup)?;
    if endpoint.is_empty()
        || endpoint.len() > 4096
        || !endpoint.iter().all(|byte| (33..=126).contains(byte))
    {
        return Err(Error::Endpoint);
    }
    let text = std::str::from_utf8(&endpoint).map_err(|_| Error::Endpoint)?;
    let public_https = webhook::endpoint_url(text)
        .map_err(|_| Error::Endpoint)?
        .scheme()
        == "https";
    let authorization = if let Some(name) = authorization_reference {
        let bytes = lookup(name, 4096).map_err(|_| Error::Lookup)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Authorization)?;
        if !webhook::header_value(text) {
            return Err(Error::Authorization);
        }
        Some(bytes)
    } else {
        None
    };
    let revision = crate::config_reference::reference(
        b"webhook\0",
        &[
            text,
            endpoint_reference,
            authorization_reference.unwrap_or(""),
        ],
    );
    Ok(Resources {
        endpoint,
        authorization,
        timeout,
        payload_limit,
        revision,
        public_https,
        trust: None,
        slack: false,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn slack_revision_binds_renderer_and_checks_transformed_size() {
        let resolve = || {
            super::resolve(
                "URL",
                None,
                std::time::Duration::from_secs(1),
                1024,
                |_, _| {
                    Ok(zeroize::Zeroizing::new(
                        b"https://fixture.invalid/hook".to_vec(),
                    ))
                },
            )
            .unwrap()
        };
        let generic = resolve();
        let slack = resolve().with_slack();
        assert_ne!(generic.public_revision(), slack.public_revision());
        assert_eq!(
            slack.public_revision(),
            resolve().with_slack().public_revision()
        );
        assert_eq!(slack.kind(), "slack");
        assert_eq!(generic.kind(), "webhook");
        let payload = crate::public_event::payload(
            &crate::public_event::identifier([0; 16]),
            "cluster",
            "1",
            "COMPLETED",
            0,
        )
        .unwrap();
        assert!(slack.accepts_public_payload(&payload));
        assert_ne!(slack.wire_payload(&payload).unwrap(), payload);
        assert_eq!(generic.wire_payload(&payload).unwrap(), payload);
        assert!(!slack.accepts_public_payload(b"{}"));
        let mut small = slack;
        small.payload_limit = 1;
        assert!(small.wire_payload(&payload).is_err());
        assert!(small
            .synthetic_request(&crate::public_event::identifier([0; 16]), &payload)
            .is_err());
    }
    #[allow(unused_imports)] // Custom process harness omits the unit test bodies.
    use super::*;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn explicit_trust_is_owned_and_changes_the_bound_revision() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::storage::tests::Directory::new();
        let path = dir
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("webhook-root.der");
        let first = rcgen::generate_simple_self_signed(vec!["first.invalid".into()]).unwrap();
        std::fs::write(&path, first.cert.der()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let base = public_resources("https://fixture.invalid/hook", 16384);
        let base_revision = base.public_revision().unwrap().to_owned();
        let owned = base.with_trust(std::slice::from_ref(&path)).unwrap();
        assert_ne!(owned.public_revision().unwrap(), base_revision);
        let revision = owned.public_revision().unwrap().to_owned();
        let same = public_resources("https://fixture.invalid/hook", 16384)
            .with_trust(std::slice::from_ref(&path))
            .unwrap();
        assert_eq!(same.public_revision(), Some(revision.as_str()));
        let second = rcgen::generate_simple_self_signed(vec!["second.invalid".into()]).unwrap();
        std::fs::write(&path, second.cert.der()).unwrap();
        let changed = public_resources("https://fixture.invalid/hook", 16384)
            .with_trust(std::slice::from_ref(&path))
            .unwrap();
        assert_ne!(changed.public_revision(), owned.public_revision());
        std::fs::remove_file(&path).unwrap();
        let bytes = owned.into_frame("fixture", b"{}").unwrap();
        assert!(bytes
            .windows(first.cert.der().len())
            .any(|bytes| bytes == first.cert.der().as_ref()));
        assert!(public_resources("https://fixture.invalid/hook", 16384)
            .with_trust(&[path])
            .is_err());
    }
    fn public_resources(url: &str, limit: usize) -> Resources {
        resolve(
            "URL",
            Some("AUTH"),
            Duration::from_secs(2),
            limit,
            |name, _| {
                Ok(Zeroizing::new(if name == "URL" {
                    url.as_bytes().to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap()
    }
    fn public_seed(
        store: &mut crate::storage::Store,
        resources: &Resources,
        job: &str,
    ) -> crate::storage::DueDelivery {
        let mut candidate = crate::lifecycle::Reconciler::with_capacity(10);
        let events = candidate
            .observe_snapshot(
                "",
                &format!("cluster|{job}|alice|s|a|COMPLETED|0:0"),
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        store
            .persist_public_webhooks(&candidate, &events, ("cluster", "window", 10), |_| {
                vec![crate::storage::PublicRoute {
                    destination: "primary",
                    revision: resources.public_revision().unwrap(),
                }]
            })
            .unwrap();
        store.due_deliveries("cluster", 10, 10).unwrap().remove(0)
    }
    #[test]
    fn pending_slack_cannot_be_reinterpreted_as_generic_webhook() {
        let d = crate::storage::tests::Directory::new();
        let mut store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let slack = public_resources("https://fixture.invalid/secret", 16384).with_slack();
        let item = public_seed(&mut store, &slack, "1");
        assert!(public_resources("https://fixture.invalid/secret", 16384)
            .prepare_public(&store, &item)
            .is_err());
        let request = slack.prepare_public(&store, &item).unwrap();
        transport::serve(
            request.frame.as_slice(),
            std::io::sink(),
            request.frame.len(),
            |fields| {
                let message: serde_json::Value = serde_json::from_slice(&fields[3]).unwrap();
                assert!(message["text"].as_str().unwrap().contains("Job: 1"));
                Ok(crate::delivery::ProviderOutcome::Delivered)
            },
        )
        .unwrap();
    }
    #[test]
    fn public_request_uses_frozen_uuid_and_bytes_and_blocks_changed_resources() {
        let d = crate::storage::tests::Directory::new();
        let mut store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let resources = public_resources("https://fixture.invalid/secret", 16384);
        let item = public_seed(&mut store, &resources, "1");
        let request = resources.prepare_public(&store, &item).unwrap();
        let frozen = store
            .public_delivery(
                &item.event_id,
                "primary",
                resources.public_revision().unwrap(),
            )
            .unwrap()
            .unwrap();
        transport::serve(
            request.frame.as_slice(),
            std::io::sink(),
            request.frame.len(),
            |fields| {
                assert_eq!(fields[0], b"https://fixture.invalid/secret");
                assert_eq!(fields[1], b"Bearer fixture");
                assert_eq!(fields[2], frozen.id.as_bytes());
                assert_eq!(fields[3], frozen.payload);
                assert_ne!(fields[2], item.event_id.as_bytes());
                Ok(crate::delivery::ProviderOutcome::Delivered)
            },
        )
        .unwrap();
        let slack = public_resources("https://fixture.invalid/secret", 16384).with_slack();
        assert!(slack.prepare_public(&store, &item).is_err());
        let changed = public_resources("https://fixture.invalid/changed", 16384);
        assert!(changed.prepare_public(&store, &item).is_err());
        let blocked = crate::delivery::deliver_due_checked(
            &mut store,
            "cluster",
            10,
            10,
            crate::delivery::RetryPolicy {
                max_attempts: 3,
                initial_delay_seconds: 5,
                max_delay_seconds: 20,
                jitter_seconds: 0,
                jitter_seed: 0,
            },
            crate::delivery::CheckedHooks {
                authorize: |store: &crate::storage::Store, item: &crate::storage::DueDelivery| {
                    changed.prepare_public(store, item)
                },
                send: |_: &crate::storage::DueDelivery, _: PublicRequest| {
                    panic!("blocked request spawned")
                },
                clock: || panic!("blocked request clock"),
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(blocked.kind, crate::delivery::FailureKind::DispatchBlocked);
        assert!(!blocked.acceptance_uncertain);
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );

        let http = public_resources("http://127.0.0.1:1234/hook", 16384);
        assert!(http.prepare_public(&store, &item).is_err());
        let mut unknown = item;
        unknown.destination = "missing".into();
        assert!(resources.prepare_public(&store, &unknown).is_err());
    }
    #[test]
    fn public_request_rechecks_current_size_limit_without_mutating_queue() {
        let d = crate::storage::tests::Directory::new();
        let mut store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let resources = public_resources("https://fixture.invalid/hook", 16384);
        let item = public_seed(&mut store, &resources, &"1".repeat(1200));
        let limited = public_resources("https://fixture.invalid/hook", 1024);
        assert_eq!(limited.public_revision(), resources.public_revision());
        assert!(limited.prepare_public(&store, &item).is_err());
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
    }
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn public_supervised_retry_and_restart_preserve_exact_frame_and_commit_completion() {
        use crate::delivery::{self, CheckedHooks, RetryPolicy};
        let d = crate::storage::tests::Directory::new();
        let resources = public_resources("https://fixture.invalid/hook", 16384);
        let mut store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        let item = public_seed(&mut store, &resources, "1");
        let original = resources.prepare_public(&store, &item).unwrap().frame;
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        };
        for (now, ack) in [(10, "RETRY"), (15, "OK")] {
            let report = delivery::deliver_due_checked(&mut store, "cluster", now, 10, policy,
                CheckedHooks {
                    authorize: |store: &crate::storage::Store, item: &crate::storage::DueDelivery| {
                        let request = resources.prepare_public(store, item)?;
                        assert_eq!(request.frame.as_slice(), original.as_slice());
                        Ok(request)
                    },
                    send: |_: &crate::storage::DueDelivery, request: PublicRequest| {
                        let mut command = std::process::Command::new("python3");
                        command.args(["-c", &format!("import sys,struct,json; b=sys.stdin.buffer.read(); f=[]\nwhile b:\n n=struct.unpack('>I',b[:4])[0]; f.append(b[4:4+n]); b=b[4+n:]\nassert len(f)==5; assert f[2].decode()==json.loads(f[3])['event_id']; assert f[2].startswith(b'evt_'); sys.stdout.write('{ack}\\n')")]);
                        request.supervise(&mut command).into_dispatch()
                    }, clock: || Some(now), cancelled: || false,
                }).unwrap();
            if now == 10 {
                assert_eq!(report.retrying, 1);
            } else {
                assert_eq!(report.delivered, 1);
            }
            drop(store);
            store = crate::storage::Store::open(&d.path(), Duration::from_millis(50)).unwrap();
        }
        assert!(store.due_deliveries("cluster", 20, 10).unwrap().is_empty());
    }

    #[test]
    fn exact_owned_resources_survive_source_rotation_and_use_existing_frame() {
        let mut endpoint = Zeroizing::new(b"https://fixture.invalid/hook?token=fixture".to_vec());
        let mut calls = vec![];
        let resources = resolve(
            "URL",
            Some("AUTH"),
            Duration::from_secs(2),
            1024,
            |name, limit| {
                calls.push((name.to_owned(), limit));
                Ok(Zeroizing::new(if name == "URL" {
                    endpoint.to_vec()
                } else {
                    b"Bearer fixture ".to_vec()
                }))
            },
        )
        .unwrap();
        endpoint.clear();
        drop(endpoint);
        assert_eq!(calls, vec![("URL".into(), 4096), ("AUTH".into(), 4096)]);
        let frame = resources
            .into_frame("opaque-fixture", b"fixture body")
            .unwrap();
        transport::serve(frame.as_slice(), std::io::sink(), frame.len(), |fields| {
            assert!(fields[0] == b"https://fixture.invalid/hook?token=fixture");
            assert!(fields[1] == b"Bearer fixture ");
            assert!(
                fields[2] == b"opaque-fixture"
                    && fields[3] == b"fixture body"
                    && fields[4] == b"2000"
            );
            Ok(crate::delivery::ProviderOutcome::Delivered)
        })
        .unwrap();
    }
    #[test]
    fn preflight_rejects_references_and_policy_without_lookup() {
        for (url, auth) in [("1URL", None), ("URL", Some("BAD-NAME"))] {
            assert!(matches!(
                resolve(url, auth, Duration::from_secs(1), 1024, |_, _| panic!(
                    "preflight lookup"
                )),
                Err(Error::Reference)
            ));
        }
        for (timeout, limit) in [
            (Duration::ZERO, 1024),
            (Duration::from_secs(31), 1024),
            (Duration::from_millis(1500), 1024),
            (Duration::from_secs(1), 1023),
            (Duration::from_secs(1), 16385),
        ] {
            assert!(matches!(
                resolve("URL", None, timeout, limit, |_, _| panic!("policy lookup")),
                Err(Error::Policy)
            ));
        }
    }
    #[test]
    fn invalid_endpoint_stops_before_authorization_with_static_errors() {
        for bad in [
            b"".to_vec(),
            vec![b'x'; 4097],
            vec![0xff],
            b"http://remote.invalid/".to_vec(),
            b"https://user:fixture@fixture.invalid/".to_vec(),
            b"https://fixture.invalid/#fragment".to_vec(),
            b"https://fixture.invalid/\n".to_vec(),
        ] {
            let error = resolve(
                "URL",
                Some("AUTH"),
                Duration::from_secs(1),
                1024,
                |name, _| {
                    assert_eq!(name, "URL");
                    Ok(Zeroizing::new(bad.clone()))
                },
            )
            .err()
            .unwrap();
            assert_eq!(format!("{error:?}"), "Endpoint");
        }
        assert!(matches!(
            resolve("URL", None, Duration::from_secs(1), 1024, |_, _| Err(())),
            Err(Error::Lookup)
        ));
    }
    #[test]
    fn configured_auth_never_falls_back_and_frame_checks_request_budget() {
        for bad in [
            None,
            Some(vec![]),
            Some(vec![b'x'; 4097]),
            Some(vec![0xff]),
            Some(b"fixture\r\n".to_vec()),
        ] {
            let error = resolve(
                "URL",
                Some("AUTH"),
                Duration::from_secs(1),
                1024,
                |name, _| {
                    if name == "URL" {
                        Ok(Zeroizing::new(b"http://127.0.0.1/hook".to_vec()))
                    } else {
                        bad.clone().map(Zeroizing::new).ok_or(())
                    }
                },
            )
            .err()
            .unwrap();
            assert_eq!(
                error,
                if bad.is_none() {
                    Error::Lookup
                } else {
                    Error::Authorization
                }
            );
        }
        let load = || {
            resolve("URL", None, Duration::from_secs(30), 16384, |name, _| {
                assert_eq!(name, "URL");
                Ok(Zeroizing::new(b"http://127.0.0.1/hook".to_vec()))
            })
            .unwrap()
        };
        for id in ["", "bad\n", &"x".repeat(129)] {
            assert!(matches!(load().into_frame(id, b"x"), Err(Error::Request)));
        }
        assert!(matches!(
            load().into_frame("fixture", &vec![0; 16385]),
            Err(Error::Request)
        ));
        assert!(load().into_frame(&"x".repeat(128), &vec![0; 16384]).is_ok());
    }
}
