//! Shared webhook resource composition; owner supplies runtime and lookup adapters.
use crate::{
    config_validate::{self, get, list, object, text},
    observation_mapping, route_selection, runtime_mapping, webhook_resolution,
};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    #[cfg(test)]
    Validation(config_validate::Error),
    UnsupportedProvider,
    Shape,
    Identity(observation_mapping::Error),
    Resource(webhook_resolution::Error),
}
// No Debug/Clone; config/resource metadata remains in zeroizing owners.
pub(super) struct Webhook {
    pub(super) alias: Zeroizing<String>,
    pub(super) resources: webhook_resolution::Resources,
}
pub(super) struct Plan {
    pub(super) observation: observation_mapping::Plan,
    pub(super) controls: runtime_mapping::Plan,
    pub(super) routes: route_selection::Plan,
    pub(super) storage_path: Zeroizing<String>,
    pub(super) webhooks: Vec<Webhook>,
}
pub(super) struct PollInput<'a> {
    pub(super) active_program: &'a std::path::Path,
    pub(super) accounting_program: &'a std::path::Path,
    pub(super) history_start: &'a str,
    pub(super) history_end: &'a str,
    pub(super) observed_at: i64,
}
impl Plan {
    // Public webhook transactions use actual prepared HTTPS resource revisions.
    // Mixed SMTP composition must opt in separately; never silently drop its routes.
    pub(super) fn poll_public_with(
        &self,
        input: PollInput<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        query: impl FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::scheduler::DurablePollError> {
        let invalid = || {
            crate::scheduler::DurablePollError::Storage(crate::storage::StorageError::InvalidInput)
        };
        let mut bindings = std::collections::HashMap::new();
        for webhook in &self.webhooks {
            let revision = webhook.resources.public_revision().ok_or_else(invalid)?;
            if bindings.insert(webhook.alias.as_str(), revision).is_some() {
                return Err(invalid());
            }
        }
        let names = self.routes.providers();
        if names.len() != bindings.len() || names.iter().any(|name| !bindings.contains_key(name)) {
            return Err(invalid());
        }
        let request = crate::scheduler::PollRequest {
            active_program: input.active_program,
            accounting_program: input.accounting_program,
            cluster: self.observation.cluster(),
            user: self.observation.user(),
            history_start: input.history_start,
            history_end: input.history_end,
            timeout: self.controls.query_timeout,
            output_limit: self.controls.output_limit,
            emit_discovered: self.observation.emit_discovered(),
        };
        let mut candidate = reconciler.clone();
        let events = crate::scheduler::poll_with(&request, &mut candidate, query)
            .map_err(crate::scheduler::DurablePollError::Poll)?;
        // UUID strings have fixed encoded width, so this admission render matches
        // the eventual payload size without allocating an identity before commit.
        for event in &events {
            let selected = self.routes.destinations(&event.state);
            if selected.is_empty() {
                continue;
            }
            let payload = crate::public_event::payload(
                "evt_00000000000040008000000000000000",
                &event.identity.cluster,
                &event.identity.job,
                event.state.code(),
                input.observed_at,
            )
            .map_err(|_| invalid())?;
            if self.webhooks.iter().any(|webhook| {
                selected.contains(&webhook.alias.as_str())
                    && !webhook.resources.accepts_public_payload(&payload)
            }) {
                return Err(invalid());
            }
        }
        store
            .persist_public_webhooks(
                &candidate,
                &events,
                (request.cluster, request.history_end, input.observed_at),
                |event| {
                    self.routes
                        .destinations(&event.state)
                        .into_iter()
                        .map(|destination| crate::storage::PublicRoute {
                            destination,
                            revision: bindings[destination],
                        })
                        .collect()
                },
            )
            .map_err(crate::scheduler::DurablePollError::Storage)?;
        *reconciler = candidate;
        Ok(events)
    }
    #[cfg(test)]
    pub(super) fn poll_with(
        &self,
        input: PollInput<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        query: impl FnMut(
            &mut std::process::Command,
            Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::scheduler::DurablePollError> {
        self.routes.poll_with(
            &crate::scheduler::PollRequest {
                active_program: input.active_program,
                accounting_program: input.accounting_program,
                cluster: self.observation.cluster(),
                user: self.observation.user(),
                history_start: input.history_start,
                history_end: input.history_end,
                timeout: self.controls.query_timeout,
                output_limit: self.controls.output_limit,
                emit_discovered: self.observation.emit_discovered(),
            },
            reconciler,
            store,
            input.observed_at,
            query,
        )
    }
}
#[cfg(test)]
pub(super) fn prepare(
    bytes: &[u8],
    identity: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Plan, Error> {
    let draft = config_validate::validate(bytes).map_err(Error::Validation)?;
    prepare_draft(draft, false, identity, lookup)
}
pub(super) fn prepare_draft(
    draft: config_validate::Draft,
    allow_smtp: bool,
    identity: impl FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    mut lookup: impl FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
) -> Result<Plan, Error> {
    // Entire structural/policy validation precedes all caller-owned lookups.
    let shape = |_| Error::Shape;
    let root = object(&draft.value).map_err(shape)?;
    let providers = object(get(root, "providers").map_err(shape)?).map_err(shape)?;
    for (_, provider) in providers {
        if text(get(object(provider).map_err(shape)?, "type").map_err(shape)?).map_err(shape)?
            != "webhook"
            && !allow_smtp
        {
            return Err(Error::UnsupportedProvider);
        }
    }
    let aliases: Vec<_> = providers.iter().map(|(name, _)| name.as_str()).collect();
    let mut events = Vec::new();
    let mut destinations = Vec::new();
    for value in list(get(root, "routes").map_err(shape)?, 0, 128).map_err(shape)? {
        let route = object(value).map_err(shape)?;
        events.push(
            list(get(route, "events").map_err(shape)?, 1, 9)
                .map_err(shape)?
                .iter()
                .map(text)
                .collect::<Result<Vec<_>, _>>()
                .map_err(shape)?,
        );
        destinations.push(
            list(get(route, "send_to").map_err(shape)?, 1, 64)
                .map_err(shape)?
                .iter()
                .map(text)
                .collect::<Result<Vec<_>, _>>()
                .map_err(shape)?,
        );
    }
    let rules: Vec<_> = events
        .iter()
        .zip(&destinations)
        .map(|(events, destinations)| route_selection::Rule {
            events,
            destinations,
        })
        .collect();
    let routes = route_selection::Plan::new(&aliases, &rules).map_err(|_| Error::Shape)?;
    let monitor = object(get(root, "monitor").map_err(shape)?).map_err(shape)?;
    let observation = observation_mapping::map(
        text(get(monitor, "cluster_id").map_err(shape)?).map_err(shape)?,
        true,
        text(get(monitor, "initial_observation").map_err(shape)?).map_err(shape)?,
        identity,
    )
    .map_err(Error::Identity)?;
    let mut webhooks = Vec::new();
    for (alias, provider) in providers {
        let p = object(provider).map_err(shape)?;
        if text(get(p, "type").map_err(shape)?).map_err(shape)? == "smtp" {
            continue;
        }
        let integer = |key| match get(p, key).map_err(shape)? {
            crate::config_decode::Value::Integer(n) => Ok(*n),
            _ => Err(Error::Shape),
        };
        let auth = get(p, "authorization_env")
            .ok()
            .map(text)
            .transpose()
            .map_err(shape)?;
        let mut resources = webhook_resolution::resolve(
            text(get(p, "url_env").map_err(shape)?).map_err(shape)?,
            auth,
            Duration::from_secs(integer("request_timeout_seconds")?),
            usize::try_from(integer("payload_limit_bytes")?).map_err(|_| Error::Shape)?,
            &mut lookup,
        )
        .map_err(Error::Resource)?;
        if let Ok(roots) = get(p, "trust_der_files") {
            let paths = list(roots, 1, 32)
                .map_err(shape)?
                .iter()
                .map(|value| text(value).map(std::path::PathBuf::from))
                .collect::<Result<Vec<_>, _>>()
                .map_err(shape)?;
            resources = resources.with_trust(&paths).map_err(Error::Resource)?;
        }
        webhooks.push(Webhook {
            alias: Zeroizing::new(alias.to_string()),
            resources,
        });
    }
    let storage = object(get(root, "storage").map_err(shape)?).map_err(shape)?;
    let storage_path = Zeroizing::new(
        text(get(storage, "path").map_err(shape)?)
            .map_err(shape)?
            .to_owned(),
    );
    Ok(Plan {
        observation,
        controls: draft.controls,
        routes,
        storage_path,
        webhooks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn fixture() -> Value {
        serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-webhook-config.json"
        ))
        .unwrap()
    }
    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }
    #[test]
    fn public_poll_binds_resolved_endpoint_and_publishes_candidate_only_after_commit() {
        let directory = crate::storage::tests::Directory::new();
        let mut store =
            crate::storage::Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut source = fixture();
        source["monitor"]["initial_observation"] = json!("emit");
        let bytes = serde_json::to_vec(&source).unwrap();
        let make_plan = |url: &str| {
            prepare(
                &bytes,
                |_| Ok(Zeroizing::new(b"alice".to_vec())),
                |name, _| {
                    Ok(Zeroizing::new(if name.ends_with("_URL") {
                        url.as_bytes().to_vec()
                    } else {
                        b"Bearer fixture".to_vec()
                    }))
                },
            )
            .unwrap()
        };
        let plan = make_plan("https://fixture.invalid/private?secret=first");
        let input = || PollInput {
            active_program: std::path::Path::new("squeue"),
            accounting_program: std::path::Path::new("sacct"),
            history_start: "1970-01-01T00:00:00",
            history_end: "1970-01-01T00:00:10",
            observed_at: 10,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                String::new()
            } else {
                "research-cluster|42|alice|private-submit|private-start|COMPLETED|0:0".to_string()
            })
        };
        let mut reconciler = crate::lifecycle::Reconciler::with_capacity(10);
        // Fault the final checkpoint after frame preparation: neither ledger nor caller advances.
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER public_checkpoint_fault BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").unwrap();
        assert!(plan
            .poll_public_with(input(), &mut reconciler, &mut store, query)
            .is_err());
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        db.execute_batch("DROP TRIGGER public_checkpoint_fault")
            .unwrap();
        drop(db);
        assert_eq!(
            plan.poll_public_with(input(), &mut reconciler, &mut store, query)
                .unwrap()
                .len(),
            1
        );
        let due = store.due_deliveries("research-cluster", 10, 10).unwrap();
        let revision = plan.webhooks[0].resources.public_revision().unwrap();
        let first = store
            .public_delivery(&due[0].event_id, "primary", revision)
            .unwrap()
            .unwrap();
        let changed = make_plan("https://fixture.invalid/private?secret=changed");
        let changed_revision = changed.webhooks[0].resources.public_revision().unwrap();
        assert_ne!(revision, changed_revision);
        assert!(matches!(
            store.public_delivery(&due[0].event_id, "primary", changed_revision),
            Err(crate::storage::StorageError::Conflict)
        ));
        assert_eq!(
            store
                .public_delivery(&due[0].event_id, "primary", revision)
                .unwrap()
                .unwrap()
                .payload,
            first.payload
        );
        let http = make_plan("http://127.0.0.1/fixture");
        assert!(http
            .poll_public_with(input(), &mut reconciler, &mut store, |_, _, _| panic!(
                "public HTTP must fail before query"
            ))
            .is_err());
        let mut incomplete = make_plan("https://fixture.invalid/private");
        incomplete.webhooks.clear();
        assert!(incomplete
            .poll_public_with(input(), &mut reconciler, &mut store, |_, _, _| panic!(
                "incomplete provider set must not query"
            ))
            .is_err());
        let mixed_draft = crate::config_validate::validate(include_bytes!(
            "../../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap();
        let mixed = prepare_draft(
            mixed_draft,
            true,
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
        assert!(mixed
            .poll_public_with(input(), &mut reconciler, &mut store, |_, _, _| panic!(
                "mixed plans must not silently drop SMTP routes"
            ))
            .is_err());
        let mut limited_source = source.clone();
        limited_source["providers"]["primary"]["payload_limit_bytes"] = json!(1024);
        let limited = prepare(
            &serde_json::to_vec(&limited_source).unwrap(),
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
        let mut fresh = crate::lifecycle::Reconciler::with_capacity(10);
        let long_job = "1".repeat(1200);
        assert!(limited
            .poll_public_with(input(), &mut fresh, &mut store, |command, _, _| Ok(
                if command.get_program() == "squeue" {
                    String::new()
                } else {
                    format!("research-cluster|{long_job}|alice|s|a|COMPLETED|0:0")
                }
            ))
            .is_err());
        assert!(fresh.latest.is_empty() && fresh.emitted.is_empty());
        assert_eq!(
            store
                .due_deliveries("research-cluster", 10, 10)
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn validation_and_unsupported_provider_fail_before_any_lookup() {
        for data in [
            b"{}".as_slice(),
            b"{\"schema_version\":true}",
            include_bytes!("../../../docs/proposals/examples/local-mixed-config.json"),
        ] {
            assert!(prepare(
                data,
                |_| panic!("invalid identity lookup"),
                |_, _| panic!("invalid resource lookup")
            )
            .is_err());
        }
        let mut source = fixture();
        source["providers"]["primary"]["authorization_env"] = json!("bad-reference");
        assert!(matches!(
            prepare(
                &bytes(&source),
                |_| panic!("invalid identity lookup"),
                |_, _| panic!("invalid resource lookup")
            ),
            Err(Error::Validation(_))
        ));
        let mut source = fixture();
        source["monitor"]["query_timeout_seconds"] = json!(60);
        assert!(matches!(
            prepare(
                &bytes(&source),
                |_| panic!("invalid controls lookup"),
                |_, _| panic!("invalid controls resource")
            ),
            Err(Error::Validation(_))
        ));
        assert!(matches!(
            prepare(
                include_bytes!("../../../docs/proposals/examples/local-mixed-config.json"),
                |_| panic!("unsupported identity lookup"),
                |_, _| panic!("unsupported resource lookup")
            ),
            Err(Error::UnsupportedProvider)
        ));
    }
    #[test]
    fn identity_and_resource_failures_stop_resolution_without_publishing_plan() {
        let source = bytes(&fixture());
        assert!(matches!(
            prepare(
                &source,
                |_| Err(()),
                |_, _| panic!("identity failure resource")
            ),
            Err(Error::Identity(observation_mapping::Error::Lookup))
        ));
        assert!(matches!(
            prepare(
                &source,
                |_| Ok(Zeroizing::new(b"-user".to_vec())),
                |_, _| panic!("bad identity resource")
            ),
            Err(Error::Identity(observation_mapping::Error::Identity))
        ));
        let mut requests = Vec::new();
        let result = prepare(
            &source,
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            |name, limit| {
                requests.push((name.to_string(), limit));
                Ok(Zeroizing::new(b"http://fixture.invalid/hook".to_vec()))
            },
        );
        assert!(matches!(
            result,
            Err(Error::Resource(webhook_resolution::Error::Endpoint))
        ));
        assert_eq!(requests, vec![("SLURM_NOTIFY_WEBHOOK_URL".into(), 4096)]);
        assert!(matches!(
            prepare(
                &source,
                |_| Ok(Zeroizing::new(b"alice".to_vec())),
                |_, _| Err(())
            ),
            Err(Error::Resource(webhook_resolution::Error::Lookup))
        ));
    }
    #[test]
    fn late_unselected_resource_failure_returns_no_partial_plan() {
        let mut source = fixture();
        source["providers"]["secondary"] = json!({
            "type":"webhook", "url_env":"SECOND_URL", "request_timeout_seconds":2,
            "payload_limit_bytes":1024
        });
        let mut calls = Vec::new();
        let result = prepare(
            &bytes(&source),
            |_| Ok(Zeroizing::new(b"alice".to_vec())),
            |name, limit| {
                calls.push((name.to_string(), limit));
                match name {
                    "SECOND_URL" => Err(()),
                    "SLURM_NOTIFY_WEBHOOK_URL" => {
                        Ok(Zeroizing::new(b"https://fixture.invalid/hook".to_vec()))
                    }
                    _ => Ok(Zeroizing::new(b"Bearer fixture".to_vec())),
                }
            },
        );
        assert!(matches!(
            result,
            Err(Error::Resource(webhook_resolution::Error::Lookup))
        ));
        assert_eq!(
            calls,
            vec![
                ("SLURM_NOTIFY_WEBHOOK_URL".into(), 4096),
                ("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION".into(), 4096),
                ("SECOND_URL".into(), 4096)
            ]
        );
    }
    #[test]
    fn owned_composition_preserves_routes_resources_and_numeric_mapping_after_source_drop() {
        let mut source = fixture();
        source["routes"] = json!([
            {"events":["JOB_COMPLETED"],"send_to":["primary"]},
            {"events":["JOB_FAILED"],"send_to":["primary"]}
        ]);
        let mut encoded = bytes(&source);
        let mut identity = Zeroizing::new(b"alice".to_vec());
        let mut endpoint = Zeroizing::new(b"https://fixture.invalid/hook?token=fixture".to_vec());
        let mut requests = Vec::new();
        let plan = prepare(
            &encoded,
            |limit| {
                assert_eq!(limit, 256);
                Ok(Zeroizing::new(identity.to_vec()))
            },
            |name, limit| {
                requests.push((name.to_string(), limit));
                Ok(Zeroizing::new(if name.ends_with("_URL") {
                    endpoint.to_vec()
                } else {
                    b"Bearer fixture".to_vec()
                }))
            },
        )
        .unwrap();
        encoded.fill(0);
        identity.clear();
        endpoint.clear();
        drop(source);
        drop(encoded);
        assert_eq!(
            (
                plan.observation.cluster(),
                plan.observation.user(),
                plan.observation.emit_discovered()
            ),
            ("research-cluster", "alice", false)
        );
        assert_eq!(plan.controls.query_timeout.as_secs(), 10);
        assert_eq!(plan.controls.output_limit, 1048576);
        assert_eq!(plan.controls.maintenance.retry.jitter_seed, 42);
        assert_eq!(
            plan.storage_path.as_str(),
            "/home/researcher/.local/state/slurm-notify/state.sqlite3"
        );
        assert_eq!(
            plan.routes
                .destinations(&crate::lifecycle::State::Completed),
            vec!["primary"]
        );
        assert!(plan
            .routes
            .destinations(&crate::lifecycle::State::Timeout)
            .is_empty());
        assert_eq!(
            requests,
            vec![
                ("SLURM_NOTIFY_WEBHOOK_URL".into(), 4096),
                ("SLURM_NOTIFY_WEBHOOK_AUTHORIZATION".into(), 4096)
            ]
        );
        let hook = plan.webhooks.into_iter().next().unwrap();
        assert_eq!(hook.alias.as_str(), "primary");
        let frame = hook
            .resources
            .into_frame("fixture-event", b"synthetic")
            .unwrap();
        assert!(frame
            .windows(b"https://fixture.invalid/hook?token=fixture".len())
            .any(|w| w == b"https://fixture.invalid/hook?token=fixture"));
        assert!(frame
            .windows(b"Bearer fixture".len())
            .any(|w| w == b"Bearer fixture"));
    }
    #[test]
    fn composed_poll_uses_validated_identity_budgets_and_initial_policy_after_reopen() {
        use crate::{lifecycle::Reconciler, storage::Store};
        use std::{path::Path, process::Command};
        for (initial, expected) in [("emit", 2), ("suppress", 0)] {
            let directory = crate::storage::tests::Directory::new();
            let mut source = fixture();
            source["monitor"]["initial_observation"] = json!(initial);
            source["storage"]["path"] = json!(directory.path().to_str().unwrap());
            source["routes"] = json!([{"events":["JOB_FAILED"],"send_to":["primary"]}]);
            let plan = prepare(
                &bytes(&source),
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
            let mut store = Store::open(
                Path::new(plan.storage_path.as_str()),
                Duration::from_millis(50),
            )
            .unwrap();
            let mut reconciler = Reconciler::with_capacity(plan.controls.identity_limit);
            let input = || PollInput {
                active_program: Path::new("squeue"),
                accounting_program: Path::new("sacct"),
                history_start: "2026-10-10T00:00:00",
                history_end: "2026-10-11T00:00:00",
                observed_at: 10,
            };
            let mut queries = 0;
            let mut query = |command: &mut Command, timeout, limit| {
                queries += 1;
                assert!(timeout <= Duration::from_secs(10));
                assert_eq!(limit, 1048576);
                assert!(command
                    .get_args()
                    .any(|arg| arg.to_string_lossy().contains("alice")));
                Ok(if command.get_program()=="squeue"{""}else{"research-cluster|1|alice|s|actual|FAILED|1:0\nresearch-cluster|2|alice|s|actual|TIMEOUT|1:0"}.into())
            };
            assert_eq!(
                plan.poll_with(input(), &mut reconciler, &mut store, &mut query)
                    .unwrap()
                    .len(),
                expected
            );
            let pending = store
                .due_deliveries(plan.observation.cluster(), 10, 10)
                .unwrap();
            assert_eq!(pending.len(), usize::from(initial == "emit"));
            if let Some(item) = pending.first() {
                assert_eq!(item.destination, "primary");
                assert_eq!(item.event.identity.job, "1");
            }
            drop(store);
            let mut store = Store::open(
                Path::new(plan.storage_path.as_str()),
                Duration::from_millis(50),
            )
            .unwrap();
            let (mut reconciler, _) = store
                .restore(plan.observation.cluster(), plan.controls.identity_limit)
                .unwrap();
            assert!(plan
                .poll_with(input(), &mut reconciler, &mut store, &mut query)
                .unwrap()
                .is_empty());
            assert_eq!(
                store
                    .due_deliveries(plan.observation.cluster(), 10, 10)
                    .unwrap()
                    .len(),
                pending.len()
            );
            assert_eq!(queries, 4);
        }
    }
}
