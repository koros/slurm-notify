//! Private rendering permission guard; transport/provider profiles remain caller fixtures.
#[cfg(test)]
use super::{
    delivery::{self, BatchFailure, BatchReport, ProviderOutcome, RetryPolicy},
    smtp_revision::Descriptor,
    storage::{DueDelivery, FrozenMessage, Store},
    supervised_provider::Diagnostic,
};

pub(super) struct Permission<'a> {
    pub(super) cluster: &'a str,
    pub(super) destination: &'a str,
    pub(super) revision: &'a str,
}

// An immutable explicit allowlist; an empty list denies all revisions.
#[cfg(test)]
pub(super) struct Permissions<'a> {
    entries: &'a [Permission<'a>],
}
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Policy,
}
pub(super) fn validate(entries: &[Permission<'_>]) -> Result<(), Error> {
    if entries.len() > 64 {
        return Err(Error::Policy);
    }
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        if entry.cluster.is_empty()
            || entry.cluster.len() > 4096
            || entry.destination.is_empty()
            || entry.destination.len() > 64
            || !entry
                .destination
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || entry.revision.len() != 64
            || !entry
                .revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !seen.insert((entry.cluster, entry.destination, entry.revision))
        {
            return Err(Error::Policy);
        }
    }
    Ok(())
}
#[cfg(test)]
impl<'a> Permissions<'a> {
    pub(super) fn new(entries: &'a [Permission<'a>]) -> Result<Self, Error> {
        validate(entries)?;
        Ok(Self { entries })
    }
    #[cfg(test)]
    fn permits(&self, cluster: &str, destination: &str, revision: &str) -> bool {
        self.entries.iter().any(|entry| {
            entry.cluster == cluster
                && entry.destination == destination
                && entry.revision == revision
        })
    }
}

#[cfg(test)]
pub(super) struct Hooks<'a, S, C, X> {
    pub(super) permissions: &'a Permissions<'a>,
    pub(super) send: S,
    pub(super) clock: C,
    pub(super) cancelled: X,
}

#[cfg(test)]
pub(super) fn deliver_due<S, C, X>(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    mut hooks: Hooks<'_, S, C, X>,
) -> Result<BatchReport, BatchFailure>
where
    S: FnMut(&DueDelivery, &Descriptor, &FrozenMessage) -> Result<ProviderOutcome, Diagnostic>,
    C: FnMut() -> Option<i64>,
    X: FnMut() -> bool,
{
    delivery::deliver_due_checked(
        store,
        cluster,
        now,
        limit,
        policy,
        delivery::CheckedHooks {
            authorize: |store: &Store, item: &DueDelivery| {
                let descriptor = store
                    .bound_smtp_revision(&item.event_id, &item.destination)
                    .map_err(|_| Diagnostic::SmtpRevisionInvalid)?
                    .ok_or(Diagnostic::SmtpRevisionMissing)?;
                if !hooks
                    .permissions
                    .permits(cluster, &item.destination, descriptor.token())
                {
                    return Err(Diagnostic::SmtpRevisionDenied);
                }
                let frozen = store
                    .frozen_smtp(&item.event_id, &item.destination)
                    .map_err(|_| Diagnostic::SmtpRevisionInvalid)?
                    .ok_or(Diagnostic::SmtpRevisionMissing)?;
                Ok((descriptor, frozen))
            },
            send: |item: &DueDelivery, (descriptor, frozen): (Descriptor, FrozenMessage)| {
                (hooks.send)(item, &descriptor, &frozen)
            },
            clock: hooks.clock,
            cancelled: hooks.cancelled,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)] // Custom harness compiles helpers without running unit tests.
    use crate::{
        lifecycle::Reconciler,
        scheduler::PollRequest,
        smtp_render::JobVisibility,
        smtp_routes::{Metadata, Plan, Route},
        storage::{self, DeliveryOutcome},
    };
    use std::{path::Path, time::Duration};
    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        }
    }
    fn seed(store: &mut Store, names: &[&str], legacy: bool) {
        let routes: Vec<_> = names
            .iter()
            .map(|name| Route {
                name,
                visibility: JobVisibility::Hidden,
                metadata: Metadata {
                    sender: "s@fixture.invalid",
                    recipient: "r@fixture.invalid",
                    namespace: "fixture",
                },
            })
            .collect();
        let plan = Plan::new(names, &routes, 4).unwrap();
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
            Ok(if command.get_program() == "squeue" {
                ""
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let mut reconciler = Reconciler::with_capacity(10);
        if legacy {
            plan.poll_with(&request, &mut reconciler, store, 10, query)
                .unwrap();
        } else {
            plan.poll_revision_with(&request, &mut reconciler, store, 10, query)
                .unwrap();
        }
    }
    fn open(directory: &storage::tests::Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }
    #[test]
    fn permission_validation_is_bounded_explicit_and_static() {
        let token = "a".repeat(64);
        let entry = || Permission {
            cluster: "cluster",
            destination: "smtp",
            revision: &token,
        };
        assert!(Permissions::new(&[]).unwrap().entries.is_empty());
        let entries = [entry()];
        assert!(Permissions::new(&entries)
            .unwrap()
            .permits("cluster", "smtp", &token));
        assert!(matches!(
            Permissions::new(&[entry(), entry()]),
            Err(Error::Policy)
        ));
        for (cluster, destination, revision) in [
            ("", "smtp", token.as_str()),
            ("cluster", "bad\r\n", token.as_str()),
            ("cluster", "smtp", "bad"),
        ] {
            assert!(matches!(
                Permissions::new(&[Permission {
                    cluster,
                    destination,
                    revision
                }]),
                Err(Error::Policy)
            ));
        }
        let many: Vec<_> = (0..65).map(|_| entry()).collect();
        assert!(matches!(Permissions::new(&many), Err(Error::Policy)));
        assert_eq!(format!("{:?}", Error::Policy), "Policy");
    }
    #[test]
    fn denied_legacy_and_malformed_bindings_block_before_send_or_clock() {
        for case in 0..5 {
            let directory = storage::tests::Directory::new();
            let mut store = open(&directory);
            seed(&mut store, &["smtp"], case == 0);
            let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
            let token = store
                .bound_smtp_revision(&item.event_id, "smtp")
                .unwrap()
                .map(|r| r.token().to_owned())
                .unwrap_or("a".repeat(64));
            let entries = [Permission {
                cluster: if case == 3 { "other" } else { "cluster" },
                destination: if case == 4 { "other" } else { "smtp" },
                revision: &token,
            }];
            let permissions = Permissions::new(if case == 1 { &[] } else { &entries }).unwrap();
            if case == 2 {
                rusqlite::Connection::open(directory.path())
                    .unwrap()
                    .execute(
                        "UPDATE smtp_render_revisions SET descriptor=?1",
                        [b"invalid".as_slice()],
                    )
                    .unwrap();
            }
            let failure = deliver_due(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                Hooks {
                    permissions: &permissions,
                    send: |_: &DueDelivery, _: &Descriptor, _: &FrozenMessage| {
                        panic!("blocked revision sent")
                    },
                    clock: || panic!("blocked revision clock"),
                    cancelled: || false,
                },
            )
            .unwrap_err();
            assert_eq!(failure.kind, delivery::FailureKind::DispatchBlocked);
            assert_eq!(
                failure.diagnostic,
                Some(match case {
                    0 => Diagnostic::SmtpRevisionMissing,
                    2 => Diagnostic::SmtpRevisionInvalid,
                    _ => Diagnostic::SmtpRevisionDenied,
                })
            );
            assert!(!failure.acceptance_uncertain);
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
            assert_eq!(store.prune_smtp_revisions("cluster", 10).unwrap(), 0);
        }
    }
    #[test]
    fn permission_denial_protects_exhausted_rows_from_retirement() {
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &["smtp"], false);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 10, Some(15))
            .unwrap();
        let permissions = Permissions::new(&[]).unwrap();
        let mut lowered = policy();
        lowered.max_attempts = 1;
        let failure = deliver_due(
            &mut store,
            "cluster",
            15,
            10,
            lowered,
            Hooks {
                permissions: &permissions,
                send: |_: &DueDelivery, _: &Descriptor, _: &FrozenMessage| {
                    panic!("exhausted row sent")
                },
                clock: || None,
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(failure.diagnostic, Some(Diagnostic::SmtpRevisionDenied));
        assert_eq!(
            store.due_deliveries("cluster", 15, 10).unwrap()[0].attempt_number,
            2
        );
    }
    #[test]
    fn partial_progress_and_cancellation_keep_committed_work() {
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &["a", "b"], false);
        let items = store.due_deliveries("cluster", 10, 10).unwrap();
        let a = store
            .bound_smtp_revision(&items[0].event_id, "a")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "a",
            revision: a.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let mut calls = 0;
        let failure = deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |item: &DueDelivery, descriptor: &Descriptor, frozen: &FrozenMessage| {
                    calls += 1;
                    assert_eq!(item.destination, "a");
                    assert_eq!(descriptor.token(), a.token());
                    assert!(!frozen.data().is_empty());
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(11),
                cancelled: || false,
            },
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(failure.committed.delivered, 1);
        assert!(!failure.acceptance_uncertain);
        let remaining = store.due_deliveries("cluster", 12, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].destination, "b");
        let failure = deliver_due(
            &mut store,
            "cluster",
            12,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |_: &DueDelivery, _: &Descriptor, _: &FrozenMessage| panic!("cancelled send"),
                clock: || None,
                cancelled: || true,
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, delivery::FailureKind::Cancelled);
        drop(store);
        let mut store = open(&directory);
        let b = store
            .bound_smtp_revision(&remaining[0].event_id, "b")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "b",
            revision: b.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let cancelled = std::cell::Cell::new(false);
        let report = deliver_due(
            &mut store,
            "cluster",
            12,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |_: &DueDelivery, _: &Descriptor, _: &FrozenMessage| {
                    cancelled.set(true);
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(13),
                cancelled: || cancelled.get(),
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        assert!(store.due_deliveries("cluster", 20, 10).unwrap().is_empty());
    }
    #[test]
    fn cancellation_between_permitted_destinations_records_only_completed_send() {
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &["a", "b"], false);
        let items = store.due_deliveries("cluster", 10, 10).unwrap();
        let a = store
            .bound_smtp_revision(&items[0].event_id, "a")
            .unwrap()
            .unwrap();
        let b = store
            .bound_smtp_revision(&items[0].event_id, "b")
            .unwrap()
            .unwrap();
        let entries = [
            Permission {
                cluster: "cluster",
                destination: "a",
                revision: a.token(),
            },
            Permission {
                cluster: "cluster",
                destination: "b",
                revision: b.token(),
            },
        ];
        let permissions = Permissions::new(&entries).unwrap();
        let cancelled = std::cell::Cell::new(false);
        let failure = deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |item: &DueDelivery, _: &Descriptor, _: &FrozenMessage| {
                    assert_eq!(item.destination, "a");
                    cancelled.set(true);
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(11),
                cancelled: || cancelled.get(),
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, delivery::FailureKind::Cancelled);
        assert_eq!(failure.committed.delivered, 1);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let store = open(&directory);
        let pending = store.due_deliveries("cluster", 12, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].destination, "b");
        assert_eq!(pending[0].attempt_number, 1);
    }
    #[test]
    fn retry_reopen_keeps_exact_bound_bytes_and_completion_time() {
        let directory = storage::tests::Directory::new();
        let mut store = open(&directory);
        seed(&mut store, &["smtp"], false);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let descriptor = store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .unwrap();
        let saved = store.frozen_smtp(&item.event_id, "smtp").unwrap().unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "smtp",
            revision: descriptor.token(),
        }];
        let permissions = Permissions::new(&entries).unwrap();
        let report = deliver_due(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |_: &DueDelivery, revision: &Descriptor, frozen: &FrozenMessage| {
                    assert_eq!(revision.token(), descriptor.token());
                    assert_eq!(frozen.data(), saved.data());
                    Ok(ProviderOutcome::Retry)
                },
                clock: || Some(12),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.retrying, 1);
        assert!(store.due_deliveries("cluster", 16, 10).unwrap().is_empty());
        drop(store);
        let mut store = open(&directory);
        let report = deliver_due(
            &mut store,
            "cluster",
            17,
            10,
            policy(),
            Hooks {
                permissions: &permissions,
                send: |item: &DueDelivery, revision: &Descriptor, frozen: &FrozenMessage| {
                    assert_eq!(item.attempt_number, 2);
                    assert_eq!(revision.token(), descriptor.token());
                    assert_eq!(frozen.data(), saved.data());
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(18),
                cancelled: || false,
            },
        )
        .unwrap();
        assert_eq!(report.delivered, 1);
        assert_eq!(store.prune_smtp_revisions("cluster", 10).unwrap(), 0);
        let candidates = store.retention_plan("cluster", 30, 5, 10).unwrap();
        store
            .apply_retention("cluster", 30, 5, 10, &candidates)
            .unwrap();
        assert_eq!(store.prune_smtp_revisions("cluster", 1).unwrap(), 1);
        assert!(store
            .restore("cluster", 10)
            .unwrap()
            .0
            .emitted
            .contains(&item.event));
    }
}
