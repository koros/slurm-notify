//! Private mock-provider worker; no HTTP/SMTP transport or delivery defaults.
use super::storage::{DeliveryOutcome, DueDelivery, StorageError, Store};

#[derive(Clone, Copy)]
pub(super) struct RetryPolicy {
    pub(super) max_attempts: u32,
    pub(super) initial_delay_seconds: i64,
    pub(super) max_delay_seconds: i64,
    pub(super) jitter_seconds: i64,
    pub(super) jitter_seed: u64,
}

impl RetryPolicy {
    pub(super) fn valid(&self) -> bool {
        self.max_attempts > 0
            && self.initial_delay_seconds > 0
            && self.max_delay_seconds >= self.initial_delay_seconds
            && (0..=self.max_delay_seconds).contains(&self.jitter_seconds)
    }

    pub(super) fn valid_at(&self, now: i64) -> bool {
        self.valid()
            && now >= 0
            && now
                .checked_add(self.max_delay_seconds)
                .and_then(|time| time.checked_add(self.jitter_seconds))
                .is_some()
    }

    // Stable non-cryptographic spreading; the seed and bound are explicit inputs.
    fn jitter(&self, item: &DueDelivery) -> i64 {
        let mut hash = 0xcbf29ce484222325u64 ^ self.jitter_seed;
        for field in [
            item.event_id.as_bytes(),
            item.destination.as_bytes(),
            &item.attempt_number.to_be_bytes(),
        ] {
            for byte in (field.len() as u64)
                .to_be_bytes()
                .iter()
                .chain(field.iter())
            {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
        (hash % (self.jitter_seconds as u64 + 1)) as i64
    }

    fn delay(&self, attempt: i64) -> i64 {
        let mut delay = self.initial_delay_seconds;
        for _ in 1..attempt {
            delay = delay.saturating_mul(2).min(self.max_delay_seconds);
            if delay == self.max_delay_seconds {
                break;
            }
        }
        delay
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderOutcome {
    Delivered,
    Retry,
    // Validated relative delay from a provider; exponential backoff remains a floor.
    RetryAfter(i64),
    Rejected,
}

#[derive(Debug)]
pub(super) enum WorkerError {
    InvalidPolicy,
    Storage(StorageError),
    InvalidTime,
    DispatchBlocked(super::supervised_provider::Diagnostic),
    Cancelled,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct BatchReport {
    pub(super) delivered: usize,
    pub(super) retrying: usize,
    pub(super) failed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FailureKind {
    Cancelled,
    Policy,
    Storage,
    Time,
    DispatchBlocked,
}

pub(super) struct BatchFailure {
    pub(super) committed: BatchReport,
    pub(super) kind: FailureKind,
    pub(super) diagnostic: Option<super::supervised_provider::Diagnostic>,
    pub(super) acceptance_uncertain: bool,
    cause: WorkerError,
}

impl std::fmt::Debug for BatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchFailure")
            .field("committed", &self.committed)
            .field("kind", &self.kind)
            .field("diagnostic", &self.diagnostic)
            .field("acceptance_uncertain", &self.acceptance_uncertain)
            .finish()
    }
}

// Provider sends occur outside SQLite transactions. Stop on storage failure so
// an accepted but unrecorded send is visible, and can be retried with the same ID.
// The future real provider must enforce its own transport timeout.
fn deliver_due(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    send: impl FnMut(&DueDelivery) -> ProviderOutcome,
) -> Result<BatchReport, WorkerError> {
    deliver_due_with_clock(store, cluster, now, limit, policy, send, || Some(now))
}

pub(super) fn unix_time() -> Option<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
    .ok()
}

pub(super) fn deliver_due_with_clock(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    mut send: impl FnMut(&DueDelivery) -> ProviderOutcome,
    clock: impl FnMut() -> Option<i64>,
) -> Result<BatchReport, WorkerError> {
    deliver_due_fallible(
        store,
        cluster,
        now,
        limit,
        policy,
        |item| Ok(send(item)),
        clock,
    )
}

pub(super) fn deliver_due_fallible(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    send: impl FnMut(&DueDelivery) -> Result<ProviderOutcome, super::supervised_provider::Diagnostic>,
    clock: impl FnMut() -> Option<i64>,
) -> Result<BatchReport, WorkerError> {
    deliver_due_detailed(store, cluster, now, limit, policy, send, clock)
        .map_err(|failure| failure.cause)
}

pub(super) fn deliver_due_detailed(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    send: impl FnMut(&DueDelivery) -> Result<ProviderOutcome, super::supervised_provider::Diagnostic>,
    clock: impl FnMut() -> Option<i64>,
) -> Result<BatchReport, BatchFailure> {
    deliver_due_controlled(
        store,
        cluster,
        now,
        limit,
        policy,
        DeliveryHooks {
            send,
            clock,
            cancelled: || false,
        },
    )
}

pub(super) struct DeliveryHooks<S, C, X> {
    pub(super) send: S,
    pub(super) clock: C,
    pub(super) cancelled: X,
}

pub(super) fn deliver_due_controlled<S, C, X>(
    store: &mut Store,
    cluster: &str,
    now: i64,
    limit: usize,
    policy: RetryPolicy,
    mut hooks: DeliveryHooks<S, C, X>,
) -> Result<BatchReport, BatchFailure>
where
    S: FnMut(&DueDelivery) -> Result<ProviderOutcome, super::supervised_provider::Diagnostic>,
    C: FnMut() -> Option<i64>,
    X: FnMut() -> bool,
{
    let mut report = BatchReport::default();
    let mut uncertain = false;
    let result = (|| -> Result<(), WorkerError> {
        if (hooks.cancelled)() {
            return Err(WorkerError::Cancelled);
        }
        if !policy.valid_at(now) {
            return Err(WorkerError::InvalidPolicy);
        }
        let items = store
            .due_deliveries(cluster, now, limit)
            .map_err(WorkerError::Storage)?;
        let mut previous_completion = now;
        for item in items {
            if (hooks.cancelled)() {
                return Err(WorkerError::Cancelled);
            }
            if item.attempt_number > i64::from(policy.max_attempts) {
                store
                    .retire_exhausted(&item, policy.max_attempts)
                    .map_err(WorkerError::Storage)?;
                report.failed += 1;
                continue;
            }
            let provider = (hooks.send)(&item).map_err(WorkerError::DispatchBlocked)?;
            uncertain = true;
            // Clock failure/regression after acceptance must remain visible and replayable.
            let completed_at = (hooks.clock)()
                .filter(|time| *time >= previous_completion)
                .ok_or(WorkerError::InvalidTime)?;
            previous_completion = completed_at;
            let (outcome, retry_at) = match provider {
                ProviderOutcome::Delivered => (DeliveryOutcome::Success, None),
                ProviderOutcome::Rejected => (DeliveryOutcome::TerminalFailure, None),
                retry @ (ProviderOutcome::Retry | ProviderOutcome::RetryAfter(_)) => {
                    let hint = match retry {
                        ProviderOutcome::RetryAfter(seconds) if seconds >= 0 => seconds,
                        ProviderOutcome::RetryAfter(_) => return Err(WorkerError::InvalidPolicy),
                        _ => 0,
                    };
                    let retry_at = if item.attempt_number < i64::from(policy.max_attempts) {
                        Some(
                            completed_at
                                .checked_add(policy.delay(item.attempt_number).max(hint))
                                .and_then(|time| time.checked_add(policy.jitter(&item)))
                                .ok_or(WorkerError::InvalidPolicy)?,
                        )
                    } else {
                        None
                    };
                    (DeliveryOutcome::TransientFailure, retry_at)
                }
            };
            store
                .record_delivery(&item, outcome, completed_at, retry_at)
                .map_err(WorkerError::Storage)?;
            uncertain = false;
            match (outcome, retry_at) {
                (DeliveryOutcome::Success, _) => report.delivered += 1,
                (DeliveryOutcome::TransientFailure, Some(_)) => report.retrying += 1,
                _ => report.failed += 1,
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(report),
        Err(cause) => {
            let (kind, diagnostic) = match &cause {
                WorkerError::Cancelled => (FailureKind::Cancelled, None),
                WorkerError::InvalidPolicy => (FailureKind::Policy, None),
                WorkerError::InvalidTime => (FailureKind::Time, None),
                WorkerError::Storage(_) => (FailureKind::Storage, None),
                WorkerError::DispatchBlocked(diagnostic) => {
                    (FailureKind::DispatchBlocked, Some(*diagnostic))
                }
            };
            Err(BatchFailure {
                committed: report,
                kind,
                diagnostic,
                acceptance_uncertain: uncertain,
                cause,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Reconciler;
    use crate::storage::tests::Directory;
    use std::time::Duration;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_seconds: 5,
            max_delay_seconds: 20,
            jitter_seconds: 0,
            jitter_seed: 0,
        }
    }
    fn open(directory: &Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }
    fn seeded(directory: &Directory, destinations: &[&str]) -> Store {
        let mut store = open(directory);
        let mut reconciler = Reconciler::with_capacity(10);
        let events = reconciler
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        store
            .persist_reconciler(
                &reconciler,
                &events,
                destinations,
                "cluster",
                "2026-10-10T00:00:00",
                10,
            )
            .unwrap();
        store
    }
    fn db(directory: &Directory) -> rusqlite::Connection {
        rusqlite::Connection::open(directory.path()).unwrap()
    }

    #[test]
    fn cancellation_before_batch_never_reads_storage_or_invokes_provider() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a"]);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("DROP TABLE delivery_attempts;").unwrap();
        let failure = deliver_due_controlled(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            DeliveryHooks {
                send: |_: &DueDelivery| panic!("cancelled send"),
                clock: || panic!("cancelled clock"),
                cancelled: || true,
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Cancelled);
        assert_eq!(failure.committed, BatchReport::default());
        assert!(!failure.acceptance_uncertain);
    }

    #[test]
    fn cancellation_after_send_commits_outcome_and_preserves_unsent_attempts_on_reopen() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut cancel = false;
        let cancellation = std::cell::Cell::new(false);
        let failure = deliver_due_controlled(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            DeliveryHooks {
                send: |item: &DueDelivery| {
                    assert_eq!(item.destination, "a");
                    cancellation.set(true);
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(20),
                cancelled: || cancellation.get(),
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Cancelled);
        assert_eq!(failure.committed.delivered, 1);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let mut store = open(&directory);
        let remaining = store.due_deliveries("cluster", 20, 10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            (
                remaining[0].destination.as_str(),
                remaining[0].attempt_number
            ),
            ("b", 1)
        );
        let recovered = deliver_due_controlled(
            &mut store,
            "cluster",
            20,
            10,
            policy(),
            DeliveryHooks {
                send: |item: &DueDelivery| {
                    assert_eq!(item.destination, "b");
                    Ok(ProviderOutcome::Delivered)
                },
                clock: || Some(20),
                cancelled: || cancel,
            },
        )
        .unwrap();
        assert_eq!(recovered.delivered, 1);
        cancel = true;
        let failure = deliver_due_controlled(
            &mut store,
            "cluster",
            20,
            10,
            policy(),
            DeliveryHooks {
                send: |_: &DueDelivery| panic!("cancelled empty batch send"),
                clock: || panic!("cancelled empty batch clock"),
                cancelled: || cancel,
            },
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Cancelled);
    }

    #[test]
    fn cancellation_between_destinations_preserves_committed_retry_deadline() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut checks = 0;
        let failure = deliver_due_controlled(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            DeliveryHooks {
                send: |item: &DueDelivery| {
                    assert_eq!(item.destination, "a");
                    Ok(ProviderOutcome::RetryAfter(60))
                },
                clock: || Some(20),
                cancelled: || {
                    checks += 1;
                    checks == 3
                },
            },
        )
        .unwrap_err();
        assert_eq!(failure.committed.retrying, 1);
        assert_eq!(failure.kind, FailureKind::Cancelled);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let store = open(&directory);
        let before = store.due_deliveries("cluster", 79, 10).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(
            (before[0].destination.as_str(), before[0].attempt_number),
            ("b", 1)
        );
        let due = store.due_deliveries("cluster", 80, 10).unwrap();
        assert_eq!(due.len(), 2);
        assert_eq!(
            due.iter()
                .find(|item| item.destination == "a")
                .unwrap()
                .attempt_number,
            2
        );
    }

    #[test]
    fn mixed_progress_matches_durable_outcomes_after_later_commit_failure() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b", "c", "d"]);
        // Prepare an exhausted retry without inventing a third send.
        for (at, due) in [(10, 11), (11, 12)] {
            let item = store
                .due_deliveries("cluster", at, 10)
                .unwrap()
                .into_iter()
                .find(|item| item.destination == "a")
                .unwrap();
            store
                .record_delivery(&item, DeliveryOutcome::TransientFailure, at, Some(due))
                .unwrap();
        }
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("UPDATE outbox SET next_attempt_at=12 WHERE destination!='a'; CREATE TRIGGER reject_last_commit BEFORE UPDATE ON outbox WHEN OLD.destination='d' BEGIN SELECT RAISE(ABORT, 'fixture-private-cause'); END;").unwrap();
        let mut retry = policy();
        retry.max_attempts = 2;
        let mut called = Vec::new();
        let failure = deliver_due_detailed(
            &mut store,
            "cluster",
            20,
            10,
            retry,
            |item| {
                called.push(item.destination.clone());
                Ok(match item.destination.as_str() {
                    "b" => ProviderOutcome::Retry,
                    "c" => ProviderOutcome::Rejected,
                    "d" => ProviderOutcome::Delivered,
                    _ => panic!("retired destination sent"),
                })
            },
            || Some(20),
        )
        .unwrap_err();
        assert_eq!(called, vec!["b", "c", "d"]);
        assert_eq!(failure.kind, FailureKind::Storage);
        assert_eq!(
            failure.committed,
            BatchReport {
                delivered: 0,
                retrying: 1,
                failed: 2
            }
        );
        assert!(failure.acceptance_uncertain);
        assert!(!format!("{failure:?}").contains("fixture-private-cause"));
        drop(store);
        let mut store = open(&directory);
        let mut statement = db
            .prepare("SELECT destination,status FROM outbox ORDER BY destination")
            .unwrap();
        let status = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            status,
            vec![
                ("a".into(), "failed".into()),
                ("b".into(), "pending".into()),
                ("c".into(), "failed".into()),
                ("d".into(), "pending".into())
            ]
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            4
        );
        let ready = store.due_deliveries("cluster", 24, 10).unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(
            (ready[0].destination.as_str(), ready[0].attempt_number),
            ("d", 1)
        );
        let ready = store.due_deliveries("cluster", 25, 10).unwrap();
        assert_eq!(
            ready
                .iter()
                .map(|item| (item.destination.as_str(), item.attempt_number))
                .collect::<Vec<_>>(),
            vec![("d", 1), ("b", 2)]
        );
        db.execute_batch("DROP TRIGGER reject_last_commit;")
            .unwrap();
        let recovered = deliver_due_detailed(
            &mut store,
            "cluster",
            25,
            10,
            retry,
            |_| Ok(ProviderOutcome::Delivered),
            || Some(25),
        )
        .unwrap();
        assert_eq!(
            recovered,
            BatchReport {
                delivered: 2,
                retrying: 0,
                failed: 0
            }
        );
        assert!(store.due_deliveries("cluster", 25, 10).unwrap().is_empty());
    }

    #[test]
    fn retirement_write_failure_does_not_count_uncommitted_retirement_as_failed() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "z"]);
        let exhausted = store
            .due_deliveries("cluster", 10, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "z")
            .unwrap();
        store
            .record_delivery(&exhausted, DeliveryOutcome::TransientFailure, 10, Some(11))
            .unwrap();
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("UPDATE outbox SET next_attempt_at=11 WHERE destination='a'; CREATE TRIGGER reject_retirement BEFORE UPDATE ON outbox WHEN OLD.destination='z' BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        let mut retry = policy();
        retry.max_attempts = 1;
        let failure = deliver_due_detailed(
            &mut store,
            "cluster",
            20,
            10,
            retry,
            |item| {
                assert_eq!(item.destination, "a");
                Ok(ProviderOutcome::Delivered)
            },
            || Some(20),
        )
        .unwrap_err();
        assert_eq!(
            failure.committed,
            BatchReport {
                delivered: 1,
                retrying: 0,
                failed: 0
            }
        );
        assert_eq!(failure.kind, FailureKind::Storage);
        assert!(!failure.acceptance_uncertain);
        drop(store);
        let mut store = open(&directory);
        assert_eq!(
            store.due_deliveries("cluster", 20, 10).unwrap()[0].attempt_number,
            2
        );
        db.execute_batch("DROP TRIGGER reject_retirement;").unwrap();
        let recovered = deliver_due_detailed(
            &mut store,
            "cluster",
            20,
            10,
            retry,
            |_| panic!("exhausted retry sent"),
            || panic!("retirement completion clock"),
        )
        .unwrap();
        assert_eq!(recovered.failed, 1);
    }

    #[test]
    fn detailed_failure_counts_only_commits_and_marks_unrecorded_send_uncertain() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut completions = [Some(20), None].into_iter();
        let failure = deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            |_| Ok(ProviderOutcome::Delivered),
            || completions.next().unwrap(),
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Time);
        assert_eq!(failure.committed.delivered, 1);
        assert_eq!(failure.committed.retrying + failure.committed.failed, 0);
        assert!(failure.acceptance_uncertain);
        assert_eq!(failure.diagnostic, None);
        drop(store);
        let store = open(&directory);
        assert_eq!(
            store.due_deliveries("cluster", 30, 10).unwrap()[0].destination,
            "b"
        );
    }

    #[test]
    fn detailed_pre_send_failure_reports_zero_commits_without_uncertainty() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a"]);
        let mut invalid = policy();
        invalid.max_attempts = 0;
        let failure = deliver_due_detailed(
            &mut store,
            "cluster",
            10,
            10,
            invalid,
            |_| panic!("invalid send"),
            || panic!("invalid clock"),
        )
        .unwrap_err();
        assert_eq!(failure.kind, FailureKind::Policy);
        assert_eq!(failure.committed, BatchReport::default());
        assert!(!failure.acceptance_uncertain);
    }

    #[test]
    fn blocked_first_dispatch_does_not_sample_completion_clock_or_record_attempt() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let result = deliver_due_fallible(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            |_| {
                super::super::supervised_provider::classify(Err(
                    super::super::transport::Error::Spawn,
                ))
                .into_dispatch()
            },
            || panic!("blocked completion clock"),
        );
        assert!(matches!(
            result,
            Err(WorkerError::DispatchBlocked(
                super::super::supervised_provider::Diagnostic::Spawn
            ))
        ));
        drop(store);
        let store = open(&directory);
        let pending = store.due_deliveries("cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].attempt_number, 1);
    }

    #[test]
    fn later_blocked_dispatch_preserves_earlier_commit_and_pending_identity_after_restart() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b", "c"]);
        let pending = store.due_deliveries("cluster", 10, 10).unwrap();
        let mut calls = 0;
        let mut clocks = 0;
        let result = deliver_due_fallible(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            |item| {
                calls += 1;
                if item.destination == "a" {
                    Ok(ProviderOutcome::Delivered)
                } else {
                    super::super::supervised_provider::classify(Err(
                        super::super::transport::Error::InvalidInput,
                    ))
                    .into_dispatch()
                }
            },
            || {
                clocks += 1;
                Some(20)
            },
        );
        assert!(matches!(
            result,
            Err(WorkerError::DispatchBlocked(
                super::super::supervised_provider::Diagnostic::InvalidInput
            ))
        ));
        assert_eq!((calls, clocks), (2, 1));
        drop(store);
        let mut store = open(&directory);
        let remaining = store.due_deliveries("cluster", 30, 10).unwrap();
        assert_eq!(
            remaining
                .iter()
                .map(|item| item.destination.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        for item in &remaining {
            assert_eq!(item.attempt_number, 1);
            assert_eq!(item.event_id, pending[0].event_id);
        }
        let report = deliver_due_fallible(
            &mut store,
            "cluster",
            30,
            10,
            policy(),
            |_| {
                super::super::supervised_provider::classify(Ok(ProviderOutcome::Delivered))
                    .into_dispatch()
            },
            || Some(30),
        )
        .unwrap();
        assert_eq!(report.delivered, 2);
        assert!(store.due_deliveries("cluster", 30, 10).unwrap().is_empty());
    }

    #[test]
    fn classified_timeout_records_a_retry_and_retains_provider_delay() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let report = deliver_due_fallible(
            &mut store,
            "cluster",
            10,
            10,
            policy(),
            |_| {
                super::super::supervised_provider::classify(Err(
                    super::super::transport::Error::Timeout,
                ))
                .into_dispatch()
            },
            || Some(20),
        )
        .unwrap();
        assert_eq!(report.retrying, 1);
        assert!(store.due_deliveries("cluster", 24, 10).unwrap().is_empty());
        assert_eq!(
            store.due_deliveries("cluster", 25, 10).unwrap()[0].attempt_number,
            2
        );
        deliver_due_fallible(
            &mut store,
            "cluster",
            25,
            10,
            policy(),
            |_| {
                super::super::supervised_provider::classify(Ok(ProviderOutcome::RetryAfter(60)))
                    .into_dispatch()
            },
            || Some(30),
        )
        .unwrap();
        assert!(store.due_deliveries("cluster", 89, 10).unwrap().is_empty());
        assert_eq!(
            store.due_deliveries("cluster", 90, 10).unwrap()[0].attempt_number,
            3
        );
    }

    #[test]
    fn jitter_is_bounded_repeatable_and_spreads_destinations() {
        let directory = Directory::new();
        let store = seeded(&directory, &["primary"]);
        let mut item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let policy = RetryPolicy {
            jitter_seconds: 10,
            jitter_seed: 42,
            ..policy()
        };
        let mut slots = std::collections::HashSet::new();
        for index in 0..50 {
            item.destination = format!("destination-{index}");
            let jitter = policy.jitter(&item);
            assert!((0..=10).contains(&jitter));
            assert_eq!(jitter, policy.jitter(&item));
            slots.insert(jitter);
        }
        assert!(slots.len() > 1);
    }

    #[test]
    fn jitter_extends_provider_floor_and_persisted_due_time_survives_reopen() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let policy = RetryPolicy {
            jitter_seconds: 10,
            jitter_seed: 42,
            ..policy()
        };
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let due = 30 + 60 + policy.jitter(&item);
        deliver_due_with_clock(
            &mut store,
            "cluster",
            10,
            10,
            policy,
            |_| ProviderOutcome::RetryAfter(60),
            || Some(30),
        )
        .unwrap();
        drop(store);
        let store = open(&directory);
        assert!(store
            .due_deliveries("cluster", due - 1, 10)
            .unwrap()
            .is_empty());
        assert_eq!(store.due_deliveries("cluster", due, 10).unwrap().len(), 1);
    }

    #[test]
    fn jitter_overflow_does_not_commit_the_attempt() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        let policy = (0..100)
            .map(|jitter_seed| RetryPolicy {
                jitter_seconds: 10,
                jitter_seed,
                ..policy()
            })
            .find(|policy| policy.jitter(&item) > 0)
            .unwrap();
        assert!(matches!(
            deliver_due_with_clock(
                &mut store,
                "cluster",
                10,
                10,
                policy,
                |_| ProviderOutcome::Retry,
                || Some(i64::MAX - 5)
            ),
            Err(WorkerError::InvalidPolicy)
        ));
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
    }

    #[test]
    fn invalid_jitter_policy_fails_before_send() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        for jitter_seconds in [-1, 21] {
            let invalid = RetryPolicy {
                jitter_seconds,
                ..policy()
            };
            assert!(matches!(
                deliver_due(&mut store, "cluster", 10, 10, invalid, |_| panic!(
                    "invalid policy sent"
                )),
                Err(WorkerError::InvalidPolicy)
            ));
        }
    }

    #[test]
    fn slow_destinations_schedule_from_individual_completion_and_survive_reopen() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut times = [Some(30), Some(50)].into_iter();
        assert_eq!(
            deliver_due_with_clock(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                |_| ProviderOutcome::Retry,
                || times.next().unwrap()
            )
            .unwrap()
            .retrying,
            2
        );
        drop(store);
        let store = open(&directory);
        assert!(store.due_deliveries("cluster", 34, 10).unwrap().is_empty());
        assert_eq!(store.due_deliveries("cluster", 35, 10).unwrap().len(), 1);
        assert_eq!(store.due_deliveries("cluster", 55, 10).unwrap().len(), 2);
        let recorded = db(&directory)
            .prepare("SELECT attempted_at FROM delivery_attempts ORDER BY destination")
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(recorded, vec![30, 50]);
    }

    #[test]
    fn invalid_completion_clock_keeps_accepted_attempt_pending() {
        for time in [None, Some(-1), Some(9)] {
            let directory = Directory::new();
            let mut store = seeded(&directory, &["primary"]);
            assert!(matches!(
                deliver_due_with_clock(
                    &mut store,
                    "cluster",
                    10,
                    10,
                    policy(),
                    |_| ProviderOutcome::Delivered,
                    || time
                ),
                Err(WorkerError::InvalidTime)
            ));
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
        }
    }

    #[test]
    fn completion_overflow_and_midbatch_clock_regression_preserve_prior_commits() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut times = [Some(30), Some(29)].into_iter();
        assert!(matches!(
            deliver_due_with_clock(
                &mut store,
                "cluster",
                10,
                10,
                policy(),
                |_| ProviderOutcome::Delivered,
                || times.next().unwrap()
            ),
            Err(WorkerError::InvalidTime)
        ));
        assert_eq!(store.due_deliveries("cluster", 30, 10).unwrap().len(), 1);
        assert!(matches!(
            deliver_due_with_clock(
                &mut store,
                "cluster",
                30,
                10,
                policy(),
                |_| ProviderOutcome::Retry,
                || Some(i64::MAX)
            ),
            Err(WorkerError::InvalidPolicy)
        ));
        assert_eq!(
            store.due_deliveries("cluster", 30, 10).unwrap()[0].attempt_number,
            1
        );
    }

    #[test]
    fn successes_are_independent_and_not_replayed_after_reopen() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary", "email"]);
        let mut sent = Vec::new();
        let report = deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            assert_eq!(item.event.state.code(), "COMPLETED");
            // Sending must not hold a SQLite write transaction.
            let separate = db(&directory);
            separate
                .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
                .unwrap();
            sent.push((item.event_id.clone(), item.destination.clone()));
            ProviderOutcome::Delivered
        })
        .unwrap();
        assert_eq!(
            report,
            BatchReport {
                delivered: 2,
                ..BatchReport::default()
            }
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].0, sent[1].0);
        drop(store);
        let mut store = open(&directory);
        assert_eq!(
            deliver_due(&mut store, "cluster", 20, 10, policy(), |_| panic!(
                "delivered event replayed"
            ))
            .unwrap(),
            BatchReport::default()
        );
        assert_eq!(
            db(&directory)
                .query_row("SELECT count(*) FROM delivery_attempts", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn retries_obey_due_time_and_keep_identity_and_attempts_across_reopen() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let mut attempts = Vec::new();
        let first = deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            attempts.push((item.event_id.clone(), item.attempt_number));
            ProviderOutcome::Retry
        })
        .unwrap();
        assert_eq!(first.retrying, 1);
        assert_eq!(
            deliver_due(&mut store, "cluster", 14, 10, policy(), |_| panic!(
                "retry too early"
            ))
            .unwrap(),
            BatchReport::default()
        );
        drop(store);
        let mut store = open(&directory);
        deliver_due(&mut store, "cluster", 15, 10, policy(), |item| {
            attempts.push((item.event_id.clone(), item.attempt_number));
            ProviderOutcome::Retry
        })
        .unwrap();
        assert!(store.due_deliveries("cluster", 24, 10).unwrap().is_empty());
        deliver_due(&mut store, "cluster", 25, 10, policy(), |item| {
            attempts.push((item.event_id.clone(), item.attempt_number));
            ProviderOutcome::Delivered
        })
        .unwrap();
        assert_eq!(
            attempts.iter().map(|a| a.1).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(attempts.iter().all(|a| a.0 == attempts[0].0));
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
    }

    #[test]
    fn terminal_failure_does_not_block_another_destination() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary", "email"]);
        let report = deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            if item.destination == "primary" {
                ProviderOutcome::Rejected
            } else {
                ProviderOutcome::Delivered
            }
        })
        .unwrap();
        assert_eq!(
            report,
            BatchReport {
                delivered: 1,
                failed: 1,
                retrying: 0
            }
        );
        let statuses = db(&directory)
            .prepare("SELECT destination,status FROM outbox ORDER BY destination")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            statuses,
            vec![
                ("email".into(), "delivered".into()),
                ("primary".into(), "failed".into())
            ]
        );
    }

    #[test]
    fn retry_exhaustion_and_batch_limits_bound_sends() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["a", "b"]);
        let mut limited = policy();
        limited.max_attempts = 1;
        assert_eq!(
            deliver_due(&mut store, "cluster", 10, 1, limited, |_| {
                ProviderOutcome::Retry
            })
            .unwrap()
            .failed,
            1
        );
        assert_eq!(store.due_deliveries("cluster", 10, 10).unwrap().len(), 1);
        assert_eq!(
            deliver_due(&mut store, "cluster", 10, 1, limited, |_| {
                ProviderOutcome::Retry
            })
            .unwrap()
            .failed,
            1
        );
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
    }

    #[test]
    fn lowered_budget_retires_pending_record_without_fake_send() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        deliver_due(&mut store, "cluster", 10, 10, policy(), |_| {
            ProviderOutcome::Retry
        })
        .unwrap();
        let mut lowered = policy();
        lowered.max_attempts = 1;
        assert_eq!(
            deliver_due(&mut store, "cluster", 15, 10, lowered, |_| panic!(
                "budget exhausted"
            ))
            .unwrap()
            .failed,
            1
        );
        assert_eq!(
            db(&directory)
                .query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn failed_outcome_commit_rolls_back_attempt_and_preserves_replay_id() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let database = db(&directory);
        database.execute_batch("CREATE TRIGGER fail_outcome BEFORE UPDATE ON outbox BEGIN SELECT RAISE(ABORT,'mock failure'); END").unwrap();
        let mut accepted = Vec::new();
        match deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            accepted.push(item.event_id.clone());
            ProviderOutcome::Delivered
        }) {
            Err(WorkerError::Storage(StorageError::Database(_))) => {}
            other => panic!("expected storage failure, got {other:?}"),
        }
        assert_eq!(
            database
                .query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
            1
        );
        database.execute_batch("DROP TRIGGER fail_outcome").unwrap();
        drop(store);
        let mut store = open(&directory);
        deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            accepted.push(item.event_id.clone());
            ProviderOutcome::Delivered
        })
        .unwrap();
        assert_eq!(accepted.len(), 2); // At-least-once: acceptance before failed local commit can replay.
        assert_eq!(accepted[0], accepted[1]);
    }

    #[test]
    fn stale_attempt_and_invalid_retry_do_not_partially_update_state() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        assert!(matches!(
            store.record_delivery(&item, DeliveryOutcome::TransientFailure, 10, Some(10)),
            Err(StorageError::InvalidInput)
        ));
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 10, Some(15))
            .unwrap();
        assert!(matches!(
            store.record_delivery(&item, DeliveryOutcome::Success, 15, None),
            Err(StorageError::Conflict)
        ));
        assert_eq!(
            db(&directory)
                .query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            store.due_deliveries("cluster", 15, 10).unwrap()[0].attempt_number,
            2
        );
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn transport_timeout_remains_pending_across_reopen_and_reuses_id() {
        use crate::transport::{supervise, Error};
        use std::process::Command;
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let mut id = String::new();
        let report = deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            id = item.event_id.clone();
            let mut child = Command::new("python3");
            child.args(["-c", "import time; time.sleep(30)"]);
            assert_eq!(
                supervise(&mut child, vec![], Duration::from_millis(100), 16, 32),
                Err(Error::Timeout)
            );
            ProviderOutcome::Retry // Timeout is uncertain acceptance, never delivery.
        })
        .unwrap();
        assert_eq!(report.retrying, 1);
        drop(store);
        let mut store = open(&directory);
        assert!(store.due_deliveries("cluster", 14, 10).unwrap().is_empty());
        assert_eq!(
            deliver_due(&mut store, "cluster", 15, 10, policy(), |item| {
                assert_eq!(item.event_id, id);
                let mut child = Command::new("python3");
                child.args([
                    "-c",
                    "import sys; sys.stdin.buffer.read(); sys.stdout.write('OK\\n')",
                ]);
                supervise(&mut child, vec![], Duration::from_secs(2), 16, 32).unwrap()
            })
            .unwrap()
            .delivered,
            1
        );
    }

    #[test]
    fn webhook_rate_limit_is_committed_and_acknowledgment_stops_retries() {
        use crate::webhook::{tests::server, Webhook};
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        let (endpoint, handle) = server(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 60\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        deliver_due(&mut store, "cluster", 10, 10, policy(), |item| {
            webhook.send(&item.event_id, b"{}", 10).unwrap()
        })
        .unwrap();
        let first_request = handle.join().unwrap();
        drop(store);
        let mut store = open(&directory);
        assert!(store.due_deliveries("cluster", 69, 10).unwrap().is_empty());
        let (endpoint, handle) = server(
            "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let webhook = Webhook::new(&endpoint, None, Duration::from_secs(1), 16).unwrap();
        assert_eq!(
            deliver_due(&mut store, "cluster", 70, 10, policy(), |item| {
                webhook.send(&item.event_id, b"{}", 70).unwrap()
            })
            .unwrap()
            .delivered,
            1
        );
        let second_request = handle.join().unwrap();
        let id_header = |request: String| {
            request
                .lines()
                .find(|line| line.to_ascii_lowercase().starts_with("idempotency-key:"))
                .unwrap()
                .to_owned()
        };
        assert_eq!(id_header(first_request), id_header(second_request));
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
    }

    #[test]
    fn provider_delay_survives_reopen_and_exceeds_backoff_cap() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        assert_eq!(
            deliver_due(&mut store, "cluster", 10, 10, policy(), |_| {
                ProviderOutcome::RetryAfter(60)
            })
            .unwrap()
            .retrying,
            1
        );
        drop(store);
        let mut store = open(&directory);
        assert!(store.due_deliveries("cluster", 69, 10).unwrap().is_empty());
        let item = store.due_deliveries("cluster", 70, 10).unwrap().remove(0);
        assert_eq!(item.attempt_number, 2);
        deliver_due(&mut store, "cluster", 70, 10, policy(), |retry| {
            assert_eq!(retry.event_id, item.event_id);
            ProviderOutcome::Delivered
        })
        .unwrap();
    }

    #[test]
    fn provider_delay_cannot_shorten_backoff_or_bypass_attempt_budget() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        deliver_due(&mut store, "cluster", 10, 10, policy(), |_| {
            ProviderOutcome::RetryAfter(0)
        })
        .unwrap();
        assert!(store.due_deliveries("cluster", 14, 10).unwrap().is_empty());
        let final_attempt = RetryPolicy {
            max_attempts: 2,
            ..policy()
        };
        assert_eq!(
            deliver_due(&mut store, "cluster", 15, 10, final_attempt, |_| {
                ProviderOutcome::RetryAfter(60)
            })
            .unwrap()
            .failed,
            1
        );
        assert!(store.due_deliveries("cluster", 100, 10).unwrap().is_empty());
    }

    #[test]
    fn invalid_provider_delay_leaves_attempt_uncommitted() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        for hint in [-1, i64::MAX] {
            assert!(matches!(
                deliver_due(&mut store, "cluster", 10, 10, policy(), |_| {
                    ProviderOutcome::RetryAfter(hint)
                }),
                Err(WorkerError::InvalidPolicy)
            ));
            assert_eq!(
                store.due_deliveries("cluster", 10, 10).unwrap()[0].attempt_number,
                1
            );
        }
        assert_eq!(
            db(&directory)
                .query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn invalid_policy_and_time_fail_before_sending_and_backoff_is_capped() {
        let directory = Directory::new();
        let mut store = seeded(&directory, &["primary"]);
        for invalid in [
            RetryPolicy {
                max_attempts: 0,
                ..policy()
            },
            RetryPolicy {
                initial_delay_seconds: 0,
                ..policy()
            },
            RetryPolicy {
                max_delay_seconds: 1,
                ..policy()
            },
        ] {
            assert!(matches!(
                deliver_due(&mut store, "cluster", 10, 10, invalid, |_| panic!(
                    "invalid policy sent"
                )),
                Err(WorkerError::InvalidPolicy)
            ));
        }
        assert!(matches!(
            deliver_due(&mut store, "cluster", i64::MAX, 10, policy(), |_| panic!(
                "overflow sent"
            )),
            Err(WorkerError::InvalidPolicy)
        ));
        assert_eq!(policy().delay(i64::MAX), 20);
        assert!(store.due_deliveries("cluster", -1, 10).is_err());
        assert!(store.due_deliveries("cluster", 10, 0).is_err());
        assert!(store
            .due_deliveries("other-cluster", 10, 10)
            .unwrap()
            .is_empty());
    }
}
