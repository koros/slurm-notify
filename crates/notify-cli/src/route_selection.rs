//! Private bounded event selection; no installed routing or public event contract.
use crate::lifecycle::State;
pub(super) use crate::routing_validation::{Error, Rule};
use zeroize::Zeroizing;
// No Debug/Clone; owned aliases preserve the compiled selection independently.
pub(super) struct Plan {
    aliases: Vec<Zeroizing<String>>,
    masks: [u64; 9],
}
impl Plan {
    pub(super) fn providers(&self) -> Vec<&str> {
        self.aliases.iter().map(|name| name.as_str()).collect()
    }
    #[cfg(test)]
    pub(super) fn poll_with(
        &self,
        request: &crate::scheduler::PollRequest<'_>,
        reconciler: &mut crate::lifecycle::Reconciler,
        store: &mut crate::storage::Store,
        observed_at: i64,
        query: impl FnMut(
            &mut std::process::Command,
            std::time::Duration,
            usize,
        ) -> Result<String, crate::scheduler::QueryError>,
    ) -> Result<Vec<crate::lifecycle::Event>, crate::scheduler::DurablePollError> {
        let mut candidate = reconciler.clone();
        let events = crate::scheduler::poll_with(request, &mut candidate, query)
            .map_err(crate::scheduler::DurablePollError::Poll)?;
        store
            .persist_routed_reconciler(
                &candidate,
                &events,
                request.cluster,
                request.history_end,
                observed_at,
                |event| self.destinations(&event.state),
            )
            .map_err(crate::scheduler::DurablePollError::Storage)?;
        *reconciler = candidate;
        Ok(events)
    }
    pub(super) fn new(providers: &[&str], rules: &[Rule<'_>]) -> Result<Self, Error> {
        let masks = crate::routing_validation::validate(providers, rules)?;
        Ok(Self {
            aliases: providers
                .iter()
                .map(|name| Zeroizing::new((*name).to_owned()))
                .collect(),
            masks,
        })
    }
    pub(super) fn destinations(&self, state: &State) -> Vec<&str> {
        let index = match state {
            State::Running => 0,
            State::Completed => 1,
            State::Failed => 2,
            State::Timeout => 3,
            State::Cancelled => 4,
            State::OutOfMemory => 5,
            State::NodeFailure => 6,
            State::BootFailure => 7,
            State::Deadline => 8,
            State::Pending | State::Unknown(_) => return vec![],
        };
        self.aliases
            .iter()
            .enumerate()
            .filter(|(i, _)| self.masks[index] & (1u64 << i) != 0)
            .map(|(_, name)| name.as_str())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn overlapping_rules_union_once_in_provider_order_and_own_aliases() {
        let mut source = String::from("first");
        let plan = Plan::new(
            &[&source, "second"],
            &[
                Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["second", "first"],
                },
                Rule {
                    events: &["JOB_FAILED", "JOB_COMPLETED"],
                    destinations: &["second"],
                },
            ],
        )
        .unwrap();
        source.clear();
        drop(source);
        assert_eq!(plan.destinations(&State::Failed), vec!["first", "second"]);
        assert_eq!(plan.destinations(&State::Completed), vec!["second"]);
        assert!(plan.destinations(&State::Running).is_empty());
        assert!(Plan::new(&["first"], &[])
            .unwrap()
            .destinations(&State::Failed)
            .is_empty());
    }
    #[test]
    fn every_lifecycle_kind_selects_and_highest_destination_bit_is_preserved() {
        let names: Vec<_> = (0..64).map(|i| format!("provider{i}")).collect();
        let refs: Vec<_> = names.iter().map(String::as_str).collect();
        let events = [
            "JOB_STARTED",
            "JOB_COMPLETED",
            "JOB_FAILED",
            "JOB_TIMEOUT",
            "JOB_CANCELLED",
            "JOB_OUT_OF_MEMORY",
            "JOB_NODE_FAILURE",
            "JOB_BOOT_FAILURE",
            "JOB_DEADLINE",
        ];
        let plan = Plan::new(
            &refs,
            &[Rule {
                events: &events,
                destinations: &refs,
            }],
        )
        .unwrap();
        for state in [
            State::Running,
            State::Completed,
            State::Failed,
            State::Timeout,
            State::Cancelled,
            State::OutOfMemory,
            State::NodeFailure,
            State::BootFailure,
            State::Deadline,
        ] {
            assert_eq!(plan.destinations(&state), refs);
        }
        assert!(plan.destinations(&State::Pending).is_empty());
        assert!(plan
            .destinations(&State::Unknown("fixture".into()))
            .is_empty());
    }
    #[test]
    fn invalid_rules_fail_before_a_plan_is_published() {
        for providers in [vec![], vec!["x"; 65]] {
            assert!(matches!(Plan::new(&providers, &[]), Err(Error::Limit)));
        }
        for providers in [
            vec!["x", "x"],
            vec!["bad name"],
            vec![""],
            vec![&"x".repeat(65)],
        ] {
            assert!(Plan::new(&providers, &[]).is_err());
        }
        for (events, destinations, error) in [
            (vec![], vec!["x"], Error::Limit),
            (vec!["JOB_FAILED"; 10], vec!["x"], Error::Limit),
            (vec!["JOB_FAILED"], vec![], Error::Limit),
            (vec!["JOB_FAILED"], vec!["x"; 65], Error::Limit),
            (vec!["UNKNOWN"], vec!["x"], Error::Reference),
            (vec!["JOB_FAILED"], vec!["missing"], Error::Reference),
            (vec!["JOB_FAILED"; 2], vec!["x"], Error::Duplicate),
            (vec!["JOB_FAILED"], vec!["x"; 2], Error::Duplicate),
        ] {
            assert_eq!(
                Plan::new(
                    &["x"],
                    &[Rule {
                        events: &events,
                        destinations: &destinations
                    }]
                )
                .err()
                .unwrap(),
                error
            );
        }
        let rules: Vec<_> = (0..128)
            .map(|_| Rule {
                events: &["JOB_FAILED"],
                destinations: &["x"],
            })
            .collect();
        assert_eq!(
            Plan::new(&["x"], &rules)
                .unwrap()
                .destinations(&State::Failed),
            vec!["x"]
        );
        let mut rules = rules;
        rules.push(Rule {
            events: &["JOB_FAILED"],
            destinations: &["x"],
        });
        assert!(matches!(Plan::new(&["x"], &rules), Err(Error::Limit)));
    }
    #[test]
    fn event_routes_commit_atomically_and_do_not_retarget_pending_work_after_reopen() {
        use crate::{lifecycle::Reconciler, scheduler::PollRequest, storage::Store};
        use std::{path::Path, time::Duration};
        let directory = crate::storage::tests::Directory::new();
        let open = || Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut store = open();
        let plan = Plan::new(
            &["success", "failure"],
            &[
                Rule {
                    events: &["JOB_COMPLETED"],
                    destinations: &["success"],
                },
                Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["failure"],
                },
            ],
        )
        .unwrap();
        let request = PollRequest {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program()=="squeue" {""}
            else {"cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|TIMEOUT|1:0"}.into())
        };
        let mut reconciler = Reconciler::with_capacity(10);
        let fault = rusqlite::Connection::open(directory.path()).unwrap();
        fault
            .execute_batch(
                "CREATE TRIGGER block_route BEFORE INSERT ON outbox
            WHEN NEW.destination='failure' BEGIN SELECT RAISE(ABORT,'fixture route block'); END;",
            )
            .unwrap();
        assert!(plan
            .poll_with(&request, &mut reconciler, &mut store, 10, query)
            .is_err());
        assert!(reconciler.latest.is_empty() && reconciler.emitted.is_empty());
        assert!(store.restore("cluster", 10).unwrap().1.is_none());
        assert!(store.due_deliveries("cluster", 10, 10).unwrap().is_empty());
        fault.execute_batch("DROP TRIGGER block_route;").unwrap();
        assert_eq!(
            plan.poll_with(&request, &mut reconciler, &mut store, 10, query)
                .unwrap()
                .len(),
            3
        );
        let pending = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending
            .iter()
            .any(|due| due.event.identity.job == "1" && due.destination == "success"));
        assert!(pending
            .iter()
            .any(|due| due.event.identity.job == "2" && due.destination == "failure"));
        let ids: Vec<_> = pending
            .iter()
            .map(|due| (due.event_id.clone(), due.destination.clone()))
            .collect();
        drop(store);
        store = open();
        let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(reconciler.emitted.len(), 3);
        assert_eq!(checkpoint.as_deref(), Some(request.history_end));
        let changed = Plan::new(
            &["replacement"],
            &[Rule {
                events: &["JOB_COMPLETED", "JOB_FAILED", "JOB_TIMEOUT"],
                destinations: &["replacement"],
            }],
        )
        .unwrap();
        assert!(changed
            .poll_with(&request, &mut reconciler, &mut store, 11, query)
            .unwrap()
            .is_empty());
        let after = store.due_deliveries("cluster", 11, 10).unwrap();
        assert_eq!(
            after
                .iter()
                .map(|due| (due.event_id.clone(), due.destination.clone()))
                .collect::<Vec<_>>(),
            ids
        );
        assert!(after.iter().all(|due| due.attempt_number == 1));
    }
    #[test]
    fn mixed_selected_preparation_counts_only_selected_smtp_pairs_and_freezes_bindings() {
        use crate::{
            lifecycle::Reconciler,
            scheduler::PollRequest,
            smtp_provider_revision::{Profile, TlsMode},
            smtp_render::JobVisibility,
            smtp_routes::{self, Metadata, ProviderPlan, Route},
            storage::Store,
        };
        use std::{path::Path, time::Duration};
        let directory = crate::storage::tests::Directory::new();
        let open = || Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut store = open();
        let rendering = smtp_routes::Plan::new(
            &["mail", "plain"],
            &[Route {
                name: "mail",
                visibility: JobVisibility::Hidden,
                metadata: Metadata {
                    sender: "s@fixture.invalid",
                    recipient: "r@fixture.invalid",
                    namespace: "fixture",
                },
            }],
            1,
        )
        .unwrap();
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let trust = crate::smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "mail",
            relay: "relay.fixture.invalid",
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let bound = crate::smtp_provider_revision::Descriptor::new(&profile).unwrap();
        let providers = ProviderPlan::new(&rendering, &[profile]).unwrap();
        let request = PollRequest {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            history_start: "2026-10-10T00:00:00",
            history_end: "2026-10-11T00:00:00",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
        };
        let query = |command: &mut std::process::Command, _, _| {
            Ok(if command.get_program()=="squeue" {""}
            else {"cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|TIMEOUT|1:0"}.into())
        };
        let mut reconciler = Reconciler::with_capacity(10);
        for invalid in [vec!["missing"], vec!["mail", "mail"], vec!["mail"; 65]] {
            assert!(matches!(
                providers.poll_selected_with(
                    &request,
                    &mut reconciler,
                    &mut store,
                    10,
                    query,
                    |_| invalid.clone()
                ),
                Err(crate::smtp_poll::Error::InvalidInput)
            ));
            assert!(reconciler.latest.is_empty());
        }
        assert!(matches!(
            providers.poll_selected_with(
                &request,
                &mut reconciler,
                &mut store,
                10,
                query,
                |_| vec!["mail"]
            ),
            Err(crate::smtp_poll::Error::Limit)
        ));
        assert!(reconciler.latest.is_empty());
        let selection = Plan::new(
            &["mail", "plain"],
            &[
                Rule {
                    events: &["JOB_COMPLETED"],
                    destinations: &["mail"],
                },
                Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["plain"],
                },
            ],
        )
        .unwrap();
        let fault = rusqlite::Connection::open(directory.path()).unwrap();
        fault.execute_batch("CREATE TRIGGER block_message BEFORE INSERT ON smtp_messages BEGIN SELECT RAISE(ABORT,'fixture block'); END;").unwrap();
        assert!(providers
            .poll_selected_with(&request, &mut reconciler, &mut store, 10, query, |event| {
                selection.destinations(&event.state)
            })
            .is_err());
        assert!(reconciler.latest.is_empty() && store.restore("cluster", 10).unwrap().1.is_none());
        assert!(store.due_deliveries("cluster", 10, 10).unwrap().is_empty());
        fault.execute_batch("DROP TRIGGER block_message;").unwrap();
        assert_eq!(
            providers
                .poll_selected_with(&request, &mut reconciler, &mut store, 10, query, |event| {
                    selection.destinations(&event.state)
                })
                .unwrap()
                .len(),
            3
        );
        let pending = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 2);
        let mail = pending
            .iter()
            .find(|due| due.destination == "mail")
            .unwrap();
        let id = mail.event_id.clone();
        let frozen = store.frozen_smtp(&id, "mail").unwrap().unwrap();
        assert_eq!(
            store
                .bound_smtp_provider(&id, "mail")
                .unwrap()
                .unwrap()
                .token(),
            bound.token()
        );
        let ordinary = pending
            .iter()
            .find(|due| due.destination == "plain")
            .unwrap();
        assert!(store
            .frozen_smtp(&ordinary.event_id, "plain")
            .unwrap()
            .is_none());
        drop(store);
        store = open();
        let (mut reconciler, _) = store.restore("cluster", 10).unwrap();
        assert!(providers
            .poll_selected_with(&request, &mut reconciler, &mut store, 11, query, |_| vec![
                "plain"
            ])
            .unwrap()
            .is_empty());
        assert!(store.frozen_smtp(&id, "mail").unwrap().unwrap().data() == frozen.data());
        assert_eq!(
            store
                .bound_smtp_provider(&id, "mail")
                .unwrap()
                .unwrap()
                .token(),
            bound.token()
        );
        assert_eq!(store.due_deliveries("cluster", 11, 10).unwrap().len(), 2);
        use crate::{
            delivery::{ProviderOutcome, RetryPolicy},
            smtp_provider_delivery::{self, Hooks, MixedHooks, Permission, Permissions, Resolved},
            supervised_provider::Diagnostic,
        };
        let revision = store.bound_smtp_revision(&id, "mail").unwrap().unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "mail",
            rendering: revision.token(),
            provider: bound.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        };
        let report = smtp_provider_delivery::deliver_due_mixed(
            &mut store,
            "cluster",
            11,
            10,
            policy,
            MixedHooks {
                ordinary_destinations: &["plain"],
                ordinary_send: |due: &crate::storage::DueDelivery| {
                    assert_eq!(due.event.identity.job, "2");
                    assert_eq!(due.attempt_number, 1);
                    Ok(ProviderOutcome::Delivered)
                },
                smtp: Hooks {
                    permissions: &permissions,
                    resolve: |provider: &crate::smtp_provider_revision::Descriptor| {
                        Resolved::new(
                            provider,
                            &provider.profile(),
                            &[&root],
                            "fixture-user",
                            "first-fixture",
                        )
                    },
                    send: |due: &crate::storage::DueDelivery,
                           actual: &crate::smtp_revision::Descriptor,
                           message: &crate::storage::FrozenMessage,
                           resolved: Resolved| {
                        assert_eq!(due.event_id, id);
                        assert_eq!(due.attempt_number, 1);
                        assert_eq!(actual.token(), revision.token());
                        assert!(message.data() == frozen.data());
                        assert_eq!(resolved.provider().token(), bound.token());
                        Ok(ProviderOutcome::Retry)
                    },
                    clock: || Some(12),
                    cancelled: || false,
                },
            },
        )
        .unwrap();
        assert_eq!((report.delivered, report.retrying), (1, 1));
        drop(store);
        store = open();
        let pending = store.due_deliveries("cluster", 17, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            (
                pending[0].event_id.as_str(),
                pending[0].destination.as_str(),
                pending[0].attempt_number
            ),
            (id.as_str(), "mail", 2)
        );
        let denied = Permissions::new(&[]).unwrap();
        let failure = smtp_provider_delivery::deliver_due_mixed(
            &mut store,
            "cluster",
            17,
            10,
            policy,
            MixedHooks {
                ordinary_destinations: &["plain"],
                ordinary_send: |_: &crate::storage::DueDelivery| {
                    panic!("completed ordinary route replayed")
                },
                smtp: Hooks {
                    permissions: &denied,
                    resolve: |_: &crate::smtp_provider_revision::Descriptor|
                     -> Result<Resolved, smtp_provider_delivery::Error> {
                        panic!("denied resolution")
                    },
                    send: |_: &crate::storage::DueDelivery,
                           _: &crate::smtp_revision::Descriptor,
                           _: &crate::storage::FrozenMessage,
                           _: Resolved| { panic!("denied send") },
                    clock: || panic!("denied outcome clock"),
                    cancelled: || false,
                },
            },
        ).unwrap_err();
        assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpProviderDenied));
        drop(store);
        store = open();
        assert_eq!(
            store.due_deliveries("cluster", 17, 10).unwrap()[0].attempt_number,
            2
        );
        let recovered = smtp_provider_delivery::deliver_due_mixed(
            &mut store,
            "cluster",
            17,
            10,
            policy,
            MixedHooks {
                ordinary_destinations: &["plain"],
                ordinary_send: |_: &crate::storage::DueDelivery| {
                    panic!("completed ordinary route replayed")
                },
                smtp: Hooks {
                    permissions: &permissions,
                    resolve: |provider: &crate::smtp_provider_revision::Descriptor| {
                        Resolved::new(
                            provider,
                            &provider.profile(),
                            &[&root],
                            "fixture-user",
                            "rotated-fixture",
                        )
                    },
                    send: |due: &crate::storage::DueDelivery,
                           actual: &crate::smtp_revision::Descriptor,
                           message: &crate::storage::FrozenMessage,
                           resolved: Resolved| {
                        assert_eq!(due.event_id, id);
                        assert_eq!(due.attempt_number, 2);
                        assert_eq!(actual.token(), revision.token());
                        assert!(message.data() == frozen.data());
                        let (provider, _, credentials) = resolved.into_parts();
                        assert_eq!(provider.token(), bound.token());
                        assert!(
                            credentials.into_plain().as_slice()
                                == b"\0fixture-user\0rotated-fixture"
                        );
                        Ok(ProviderOutcome::Delivered)
                    },
                    clock: || Some(18),
                    cancelled: || false,
                },
            },
        )
        .unwrap();
        assert_eq!(recovered.delivered, 1);
        drop(store);
        store = open();
        assert!(store.due_deliveries("cluster", 18, 10).unwrap().is_empty());
        assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 3);
    }
}
