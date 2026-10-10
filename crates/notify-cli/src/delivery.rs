//! Private mock-provider worker; no HTTP/SMTP transport or delivery defaults.
use super::storage::{DeliveryOutcome, DueDelivery, StorageError, Store};

#[derive(Clone, Copy)]
struct RetryPolicy {
    max_attempts: u32,
    initial_delay_seconds: i64,
    max_delay_seconds: i64,
}

impl RetryPolicy {
    fn valid(&self) -> bool {
        self.max_attempts > 0
            && self.initial_delay_seconds > 0
            && self.max_delay_seconds >= self.initial_delay_seconds
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
enum WorkerError {
    InvalidPolicy,
    Storage(StorageError),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct BatchReport {
    delivered: usize,
    retrying: usize,
    failed: usize,
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
    mut send: impl FnMut(&DueDelivery) -> ProviderOutcome,
) -> Result<BatchReport, WorkerError> {
    if !policy.valid() || now < 0 || now.checked_add(policy.max_delay_seconds).is_none() {
        return Err(WorkerError::InvalidPolicy);
    }
    let items = store
        .due_deliveries(cluster, now, limit)
        .map_err(WorkerError::Storage)?;
    let mut report = BatchReport::default();
    for item in items {
        if item.attempt_number > i64::from(policy.max_attempts) {
            store
                .retire_exhausted(&item, policy.max_attempts)
                .map_err(WorkerError::Storage)?;
            report.failed += 1;
            continue;
        }
        let (outcome, retry_at) = match send(&item) {
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
                        now.checked_add(policy.delay(item.attempt_number).max(hint))
                            .ok_or(WorkerError::InvalidPolicy)?,
                    )
                } else {
                    None
                };
                (DeliveryOutcome::TransientFailure, retry_at)
            }
        };
        store
            .record_delivery(&item, outcome, now, retry_at)
            .map_err(WorkerError::Storage)?;
        match (outcome, retry_at) {
            (DeliveryOutcome::Success, _) => report.delivered += 1,
            (DeliveryOutcome::TransientFailure, Some(_)) => report.retrying += 1,
            _ => report.failed += 1,
        }
    }
    Ok(report)
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
