//! Private typed operational controls; no parser, defaults or installed contract.
use crate::{accounting_window, delivery, polling_cycle};
use std::time::Duration;

pub(super) use crate::control_validation::{Controls, Error};
pub(super) struct Plan {
    pub(super) poll: Duration,
    pub(super) query_timeout: Duration,
    pub(super) window: accounting_window::Policy,
    pub(super) output_limit: usize,
    pub(super) identity_limit: usize,
    pub(super) event_limit: usize,
    pub(super) maintenance: polling_cycle::Maintenance,
    pub(super) revision_cleanup_limit: usize,
}
fn size(value: u64) -> Result<usize, Error> {
    usize::try_from(value).map_err(|_| Error::Limit)
}
pub(super) fn map(input: Controls<'_>) -> Result<Plan, Error> {
    let jitter_seed = crate::control_validation::validate(input)?;
    // Every signed conversion below follows a ceiling well below i64::MAX.
    Ok(Plan {
        poll: Duration::from_secs(input.poll),
        query_timeout: Duration::from_secs(input.query_timeout),
        window: accounting_window::Policy {
            lookback_seconds: input.lookback as i64,
            overlap_seconds: input.overlap as i64,
        },
        output_limit: size(input.output)?,
        identity_limit: size(input.identities)?,
        event_limit: size(input.events)?,
        maintenance: polling_cycle::Maintenance {
            delivery_limit: size(input.batch)?,
            retry: delivery::RetryPolicy {
                max_attempts: input.attempts as u32,
                initial_delay_seconds: input.initial_backoff as i64,
                max_delay_seconds: input.max_backoff as i64,
                jitter_seconds: input.jitter as i64,
                jitter_seed,
            },
            retain_seconds: input.retention as i64,
            cleanup_limit: size(input.cleanup)?,
            stale_after: input.stale as i64,
            health_sample_limit: size(input.health_sample)?,
        },
        revision_cleanup_limit: size(input.revision_cleanup)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Controls<'static> {
        Controls {
            poll: 60,
            query_timeout: 10,
            lookback: 3600,
            overlap: 300,
            output: 1048576,
            identities: 10000,
            events: 20000,
            stale: 180,
            attempts: 5,
            initial_backoff: 5,
            max_backoff: 300,
            jitter: 15,
            jitter_seed: "42",
            batch: 50,
            retention: 604800,
            cleanup: 100,
            health_sample: 100,
            revision_cleanup: 0,
        }
    }
    #[test]
    fn explicit_values_map_without_defaults_or_narrowing() {
        let plan = map(fixture()).unwrap();
        assert_eq!(
            (plan.poll, plan.query_timeout),
            (Duration::from_secs(60), Duration::from_secs(10))
        );
        assert_eq!(
            (plan.window.lookback_seconds, plan.window.overlap_seconds),
            (3600, 300)
        );
        assert_eq!(
            (plan.output_limit, plan.identity_limit, plan.event_limit),
            (1048576, 10000, 20000)
        );
        let maintenance = plan.maintenance;
        assert_eq!(
            (
                maintenance.delivery_limit,
                maintenance.retain_seconds,
                maintenance.cleanup_limit,
                maintenance.stale_after,
                maintenance.health_sample_limit,
                plan.revision_cleanup_limit
            ),
            (50, 604800, 100, 180, 100, 0)
        );
        assert_eq!(
            (
                maintenance.retry.max_attempts,
                maintenance.retry.initial_delay_seconds,
                maintenance.retry.max_delay_seconds,
                maintenance.retry.jitter_seconds,
                maintenance.retry.jitter_seed
            ),
            (5, 5, 300, 15, 42)
        );
        assert!(maintenance.retry.valid_at(100));
        assert!(polling_cycle::Cycles::new(plan.event_limit).is_ok());
        let maximum = map(Controls {
            poll: 86400,
            query_timeout: 300,
            lookback: 604800,
            overlap: 86400,
            output: 1048576,
            identities: 100000,
            events: 200000,
            stale: 86400,
            attempts: 20,
            initial_backoff: 3600,
            max_backoff: 86400,
            jitter: 3600,
            jitter_seed: "18446744073709551615",
            batch: 256,
            retention: 31536000,
            cleanup: 256,
            health_sample: 256,
            revision_cleanup: 256,
        })
        .unwrap();
        assert_eq!(maximum.maintenance.retry.jitter_seed, u64::MAX);
        assert_eq!(
            (maximum.identity_limit, maximum.event_limit),
            (100000, 200000)
        );
    }
    #[test]
    fn out_of_range_controls_fail_before_a_plan_is_returned() {
        type Setter = fn(&mut Controls<'_>, u64);
        let fields: [(Setter, u64, u64); 17] = [
            (|v, n| v.poll = n, 1, 86400),
            (|v, n| v.query_timeout = n, 1, 300),
            (|v, n| v.lookback = n, 2, 604800),
            (|v, n| v.overlap = n, 1, 86400),
            (|v, n| v.output = n, 1024, 1048576),
            (|v, n| v.identities = n, 1, 100000),
            (|v, n| v.events = n, 1, 200000),
            (|v, n| v.stale = n, 1, 86400),
            (|v, n| v.attempts = n, 1, 20),
            (|v, n| v.initial_backoff = n, 1, 3600),
            (|v, n| v.max_backoff = n, 1, 86400),
            (|v, n| v.jitter = n, 0, 3600),
            (|v, n| v.batch = n, 1, 256),
            (|v, n| v.retention = n, 1, 31536000),
            (|v, n| v.cleanup = n, 1, 256),
            (|v, n| v.health_sample = n, 1, 256),
            (|v, n| v.revision_cleanup = n, 0, 256),
        ];
        for (set, low, high) in fields {
            for invalid in [low.checked_sub(1), Some(high + 1), Some(u64::MAX)]
                .into_iter()
                .flatten()
            {
                let mut input = fixture();
                set(&mut input, invalid);
                assert!(matches!(map(input), Err(Error::Limit)));
            }
        }
    }
    #[test]
    fn relationships_and_decimal_seed_are_checked_before_publication() {
        for input in [
            Controls {
                query_timeout: 60,
                ..fixture()
            },
            Controls {
                overlap: 3600,
                ..fixture()
            },
            Controls {
                stale: 59,
                ..fixture()
            },
            Controls {
                max_backoff: 4,
                ..fixture()
            },
            Controls {
                jitter: 301,
                ..fixture()
            },
        ] {
            assert!(matches!(map(input), Err(Error::Relationship)));
        }
        for seed in [
            "",
            "-1",
            "+1",
            "1.0",
            "1e2",
            " 42",
            "42\n",
            "é",
            "18446744073709551616",
            "000000000000000000000",
        ] {
            assert_eq!(
                format!(
                    "{:?}",
                    map(Controls {
                        jitter_seed: seed,
                        ..fixture()
                    })
                    .err()
                    .unwrap()
                ),
                "Seed"
            );
        }
        for (seed, expected) in [("0", 0), ("00042", 42)] {
            assert_eq!(
                map(Controls {
                    jitter_seed: seed,
                    ..fixture()
                })
                .unwrap()
                .maintenance
                .retry
                .jitter_seed,
                expected
            );
        }
        let minimum = map(Controls {
            poll: 2,
            query_timeout: 1,
            lookback: 2,
            overlap: 1,
            output: 1024,
            identities: 1,
            events: 1,
            stale: 2,
            attempts: 1,
            initial_backoff: 1,
            max_backoff: 1,
            jitter: 0,
            jitter_seed: "0",
            batch: 1,
            retention: 1,
            cleanup: 1,
            health_sample: 1,
            revision_cleanup: 0,
        })
        .unwrap();
        assert!(minimum.maintenance.retry.valid_at(0));
    }

    #[test]
    fn mapped_controls_drive_retry_staleness_and_retention_across_reopen() {
        use crate::{delivery::ProviderOutcome, scheduler::QueryError, storage::Store};
        use std::{path::Path, process::Command};
        let input = Controls {
            jitter: 0,
            ..fixture()
        };
        let plan = map(input).unwrap();
        let directory = crate::storage::tests::Directory::new();
        let open = || Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let mut store = open();
        let settings = polling_cycle::Settings {
            active_program: Path::new("/fixture/squeue"),
            accounting_program: Path::new("/fixture/sacct"),
            cluster: "cluster",
            user: "alice",
            timeout: plan.query_timeout,
            output_limit: plan.output_limit,
            emit_discovered: true,
            capacity: plan.identity_limit,
            event_capacity: plan.event_limit,
            destinations: &["primary"],
            window: plan.window,
        };
        let query = |command: &mut Command, timeout: Duration, output: usize| {
            assert!(!timeout.is_zero() && timeout <= Duration::from_secs(10));
            assert_eq!(output, 1048576);
            Ok(if command.get_program() == "/fixture/squeue" {
                ""
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0"
            }
            .into())
        };
        let mut cycles = polling_cycle::Cycles::new(plan.event_limit).unwrap();
        let mut times = [7200, 7200, 7210, 7210, 7210].into_iter();
        let retry = cycles.tick(
            &settings,
            &plan.maintenance,
            &mut store,
            polling_cycle::Hooks {
                query,
                send: |due: &crate::storage::DueDelivery| {
                    assert_eq!(due.attempt_number, 1);
                    ProviderOutcome::Retry
                },
                clock: || times.next(),
                cancelled: || false,
            },
        );
        assert_eq!(retry.delivery.unwrap().unwrap().retrying, 1);
        let health = retry.health.unwrap().unwrap();
        assert_eq!(health.poll.checkpoint_age_seconds, Some(10));
        assert!(!health.poll.stale);
        assert!(store
            .due_deliveries("cluster", 7214, plan.maintenance.delivery_limit)
            .unwrap()
            .is_empty());
        let pending = store
            .due_deliveries("cluster", 7215, plan.maintenance.delivery_limit)
            .unwrap();
        assert_eq!(pending.len(), 1);
        let id = pending[0].event_id.clone();
        assert_eq!(pending[0].attempt_number, 2);
        let checkpoint = store.restore("cluster", plan.identity_limit).unwrap().1;
        drop(store);

        let restored_plan = map(input).unwrap();
        let mut store = open();
        assert_eq!(
            store
                .restore("cluster", restored_plan.identity_limit)
                .unwrap()
                .1,
            checkpoint
        );
        assert_eq!(
            store
                .due_deliveries("cluster", 7215, restored_plan.maintenance.delivery_limit)
                .unwrap()[0]
                .event_id,
            id
        );
        let mut cycles = polling_cycle::Cycles::new(restored_plan.event_limit).unwrap();
        let recovered = cycles.tick(
            &settings,
            &restored_plan.maintenance,
            &mut store,
            polling_cycle::Hooks {
                query: |_: &mut Command, _, _| Err(QueryError::Timeout),
                send: |due: &crate::storage::DueDelivery| {
                    assert_eq!(due.event_id, id);
                    assert_eq!(due.attempt_number, 2);
                    ProviderOutcome::Delivered
                },
                clock: || Some(7400),
                cancelled: || false,
            },
        );
        assert_eq!(
            recovered.poll.unwrap().unwrap_err(),
            polling_cycle::StageError::Poll
        );
        assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 1);
        assert_eq!(recovered.cleanup.unwrap().unwrap(), 0);
        let health = recovered.health.unwrap().unwrap();
        assert_eq!(health.poll.checkpoint_age_seconds, Some(200));
        assert!(health.poll.stale);
        assert_eq!(
            store
                .restore("cluster", restored_plan.identity_limit)
                .unwrap()
                .1,
            checkpoint
        );
        drop(store);

        let mut store = open();
        assert!(store
            .due_deliveries("cluster", 7400, restored_plan.maintenance.delivery_limit)
            .unwrap()
            .is_empty());
        let cleaned = cycles.tick(
            &settings,
            &restored_plan.maintenance,
            &mut store,
            polling_cycle::Hooks {
                query,
                send: |_: &crate::storage::DueDelivery| panic!("completed delivery replayed"),
                clock: || Some(7400 + 604801),
                cancelled: || false,
            },
        );
        assert_eq!(cleaned.cleanup.unwrap().unwrap(), 1);
        assert!(!cleaned.health.unwrap().unwrap().poll.stale);
        assert!(store
            .due_deliveries(
                "cluster",
                7400 + 604801,
                restored_plan.maintenance.delivery_limit
            )
            .unwrap()
            .is_empty());
    }
}
