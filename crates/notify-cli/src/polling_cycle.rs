//! Private caller-driven cycles: no background loop, scheduling default, or CLI.
use super::{
    accounting_window::{self, Policy, Window},
    scheduler::{poll_durable_with, DurablePollError, PollRequest, QueryError},
    storage::{StorageError, Store},
};
use std::{path::Path, process::Command, time::Duration};

struct Settings<'a> {
    active_program: &'a Path,
    accounting_program: &'a Path,
    cluster: &'a str,
    user: &'a str,
    timeout: Duration,
    output_limit: usize,
    emit_discovered: bool,
    capacity: usize,
    destinations: &'a [&'a str],
    window: Policy,
}

#[derive(Debug)]
enum Error {
    InvalidLimit,
    Storage(StorageError),
    Window(accounting_window::Error),
    Poll(DurablePollError),
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Cancelled,
    Exhausted,
    Committed { window: Window, events: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Storage,
    Window,
    ActiveQuery,
    AccountingQuery,
    Reconciliation,
    InvalidRequest,
}

#[derive(Default)]
struct PollHealth {
    consecutive_failures: u64,
    last_failure: Option<Failure>,
    checkpoint: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
struct PollReport {
    consecutive_failures: u64,
    last_failure: Option<Failure>,
    checkpoint_age_seconds: Option<i64>,
    stale: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum ReportError {
    InvalidInput,
    ClockRegressed,
    Storage,
}

#[derive(Debug)]
struct HealthReport {
    poll: PollReport,
    storage: super::storage::StorageHealth,
}

impl PollHealth {
    fn report(&self, now: i64, stale_after: i64) -> Result<PollReport, ReportError> {
        if now < 0 || stale_after <= 0 {
            return Err(ReportError::InvalidInput);
        }
        let age = self.checkpoint.map(|checkpoint| now - checkpoint);
        if age.is_some_and(|age| age < 0) {
            return Err(ReportError::ClockRegressed);
        }
        Ok(PollReport {
            consecutive_failures: self.consecutive_failures,
            last_failure: self.last_failure,
            checkpoint_age_seconds: age,
            stale: age.map(|age| age >= stale_after).unwrap_or(true),
        })
    }
}

struct Cycles {
    remaining: usize,
    health: PollHealth,
}

impl Cycles {
    fn new(limit: usize) -> Result<Self, Error> {
        if limit == 0 {
            return Err(Error::InvalidLimit);
        }
        Ok(Self {
            remaining: limit,
            health: PollHealth::default(),
        })
    }

    fn report(
        &self,
        settings: &Settings<'_>,
        store: &mut Store,
        now: i64,
        stale_after: i64,
        retain_seconds: i64,
        sample_limit: usize,
    ) -> Result<HealthReport, ReportError> {
        // Refresh the durable checkpoint even before the first restarted cycle.
        self.health.report(now, stale_after)?;
        let (_, checkpoint) = store
            .restore(settings.cluster, settings.capacity)
            .map_err(|_| ReportError::Storage)?;
        let checkpoint = checkpoint
            .as_deref()
            .map(accounting_window::parse_timestamp)
            .transpose()
            .map_err(|_| ReportError::Storage)?;
        let poll = PollHealth {
            checkpoint,
            consecutive_failures: self.health.consecutive_failures,
            last_failure: self.health.last_failure,
        }
        .report(now, stale_after)?;
        let storage = store
            .health(
                settings.cluster,
                now,
                retain_seconds,
                settings.capacity,
                sample_limit,
            )
            .map_err(|_| ReportError::Storage)?;
        Ok(HealthReport { poll, storage })
    }

    fn step(
        &mut self,
        settings: &Settings<'_>,
        store: &mut Store,
        cancelled: bool,
        now: i64,
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    ) -> Result<Outcome, Error> {
        let result = self.step_inner(settings, store, cancelled, now, query);
        match &result {
            Ok(Outcome::Committed { .. }) => {
                self.health.checkpoint = Some(now);
                self.health.consecutive_failures = 0;
                self.health.last_failure = None;
            }
            Err(error) => {
                use super::scheduler::PollError;
                let failure = match error {
                    Error::Storage(_) | Error::Poll(DurablePollError::Storage(_)) => {
                        Failure::Storage
                    }
                    Error::Window(_) => Failure::Window,
                    Error::Poll(DurablePollError::Poll(PollError::Active(_))) => {
                        Failure::ActiveQuery
                    }
                    Error::Poll(DurablePollError::Poll(PollError::Accounting(_))) => {
                        Failure::AccountingQuery
                    }
                    Error::Poll(DurablePollError::Poll(PollError::Parse(_))) => {
                        Failure::Reconciliation
                    }
                    _ => Failure::InvalidRequest,
                };
                self.health.consecutive_failures =
                    self.health.consecutive_failures.saturating_add(1);
                self.health.last_failure = Some(failure);
            }
            _ => {}
        }
        result
    }

    // Every attempted cycle consumes the finite budget, including failures.
    // Caller supplies cadence and checks cancellation before starting each cycle.
    // An in-flight query retains the existing scheduler deadline, not cancellation.
    fn step_inner(
        &mut self,
        settings: &Settings<'_>,
        store: &mut Store,
        cancelled: bool,
        now: i64,
        query: impl FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
    ) -> Result<Outcome, Error> {
        if cancelled {
            return Ok(Outcome::Cancelled);
        }
        if self.remaining == 0 {
            return Ok(Outcome::Exhausted);
        }
        self.remaining -= 1;
        // Durable state is authoritative after failed commits or a reopened owner.
        let (mut reconciler, checkpoint) = store
            .restore(settings.cluster, settings.capacity)
            .map_err(Error::Storage)?;
        let window = accounting_window::plan(now, checkpoint.as_deref(), settings.window)
            .map_err(Error::Window)?;
        self.health.checkpoint = checkpoint
            .as_deref()
            .map(accounting_window::parse_timestamp)
            .transpose()
            .map_err(Error::Window)?;
        let request = PollRequest {
            active_program: settings.active_program,
            accounting_program: settings.accounting_program,
            cluster: settings.cluster,
            user: settings.user,
            history_start: &window.start,
            history_end: &window.end,
            timeout: settings.timeout,
            output_limit: settings.output_limit,
            emit_discovered: settings.emit_discovered,
        };
        let events = poll_durable_with(
            &request,
            &mut reconciler,
            store,
            settings.destinations,
            now,
            query,
        )
        .map_err(Error::Poll)?;
        Ok(Outcome::Committed {
            window,
            events: events.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tests::Directory;

    fn settings() -> Settings<'static> {
        Settings {
            active_program: Path::new("squeue"),
            accounting_program: Path::new("sacct"),
            cluster: "cluster",
            user: "alice",
            timeout: Duration::from_secs(1),
            output_limit: 4096,
            emit_discovered: true,
            capacity: 10,
            destinations: &["primary"],
            window: Policy {
                lookback_seconds: 3600,
                overlap_seconds: 300,
            },
        }
    }

    fn open(directory: &Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }

    #[test]
    fn poll_health_tracks_failures_staleness_and_recovery_without_private_details() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(5).unwrap();
        assert!(
            cycles
                .report(&settings(), &mut store, 7200, 60, 50, 10)
                .unwrap()
                .poll
                .stale
        );
        cycles
            .step(&settings(), &mut store, false, 7200, |_, _, _| {
                Ok(String::new())
            })
            .unwrap();
        assert!(!cycles.health.report(7259, 60).unwrap().stale);
        assert!(cycles.health.report(7260, 60).unwrap().stale);
        assert!(cycles
            .step(&settings(), &mut store, false, 7260, |_, _, _| Err(
                QueryError::Timeout
            ))
            .is_err());
        assert!(cycles
            .step(&settings(), &mut store, false, 7320, |command, _, _| {
                if command.get_program() == "squeue" {
                    Ok(String::new())
                } else {
                    Err(QueryError::MissingCommand)
                }
            })
            .is_err());
        let health = cycles
            .report(&settings(), &mut store, 7320, 60, 50, 10)
            .unwrap();
        assert_eq!(health.poll.consecutive_failures, 2);
        assert_eq!(health.poll.last_failure, Some(Failure::AccountingQuery));
        assert_eq!(health.poll.checkpoint_age_seconds, Some(120));
        let text = format!("{health:?}");
        assert!(!text.contains("alice") && !text.contains("cluster") && !text.contains("sacct"));
        assert_eq!(
            health.storage,
            store.health("cluster", 7320, 50, 10, 10).unwrap()
        );
        cycles
            .step(&settings(), &mut store, true, 7320, |_, _, _| {
                panic!("cancelled query")
            })
            .unwrap();
        assert_eq!(cycles.health.report(7320, 60).unwrap(), health.poll);
        cycles
            .step(&settings(), &mut store, false, 7380, |_, _, _| {
                Ok(String::new())
            })
            .unwrap();
        let recovered = cycles.health.report(7380, 60).unwrap();
        assert_eq!(recovered.consecutive_failures, 0);
        assert_eq!(recovered.last_failure, None);
        assert!(!recovered.stale);
    }

    #[test]
    fn restarted_failure_uses_durable_checkpoint_and_commit_failure_is_not_success() {
        let directory = Directory::new();
        let mut store = open(&directory);
        Cycles::new(1)
            .unwrap()
            .step(&settings(), &mut store, false, 7200, |_, _, _| {
                Ok(String::new())
            })
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        let mut restarted = Cycles::new(2).unwrap();
        let initial = restarted
            .report(&settings(), &mut store, 7300, 60, 50, 10)
            .unwrap();
        assert_eq!(initial.poll.checkpoint_age_seconds, Some(100));
        assert_eq!(initial.poll.consecutive_failures, 0);
        assert!(restarted
            .step(&settings(), &mut store, false, 7300, |_, _, _| Err(
                QueryError::Timeout
            ))
            .is_err());
        assert_eq!(
            restarted
                .health
                .report(7300, 60)
                .unwrap()
                .checkpoint_age_seconds,
            Some(100)
        );
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER reject_health_commit BEFORE UPDATE ON checkpoints BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        assert!(restarted
            .step(&settings(), &mut store, false, 7360, |_, _, _| Ok(
                String::new()
            ))
            .is_err());
        let report = restarted.health.report(7360, 60).unwrap();
        assert_eq!(report.consecutive_failures, 2);
        assert_eq!(report.last_failure, Some(Failure::Storage));
        assert_eq!(report.checkpoint_age_seconds, Some(160));
        restarted
            .step(&settings(), &mut store, false, 7400, |_, _, _| {
                panic!("exhausted query")
            })
            .unwrap();
        assert_eq!(restarted.health.report(7360, 60).unwrap(), report);
    }

    #[test]
    fn health_rejects_clock_regression_and_invalid_policy_and_saturates_failures() {
        let mut health = PollHealth {
            checkpoint: Some(7200),
            consecutive_failures: 0,
            last_failure: None,
        };
        assert_eq!(health.report(7199, 60), Err(ReportError::ClockRegressed));
        assert_eq!(health.report(-1, 60), Err(ReportError::InvalidInput));
        assert_eq!(health.report(7200, 0), Err(ReportError::InvalidInput));
        health.checkpoint = None;
        assert_eq!(
            health.report(7200, 60).unwrap().checkpoint_age_seconds,
            None
        );
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        cycles.health.consecutive_failures = u64::MAX;
        assert!(cycles
            .step(&settings(), &mut store, false, 7200, |_, _, _| Err(
                QueryError::Timeout
            ))
            .is_err());
        assert_eq!(cycles.health.consecutive_failures, u64::MAX);
        assert_eq!(
            cycles
                .report(&settings(), &mut store, 7200, 60, 50, 0)
                .unwrap_err(),
            ReportError::Storage
        );
    }

    #[test]
    fn cancellation_and_exhaustion_do_not_query_or_advance_checkpoint() {
        assert!(matches!(Cycles::new(0), Err(Error::InvalidLimit)));
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let unused =
            |_: &mut Command, _, _| -> Result<String, QueryError> { panic!("unexpected query") };
        assert_eq!(
            cycles
                .step(&settings(), &mut store, true, 7200, unused)
                .unwrap(),
            Outcome::Cancelled
        );
        assert_eq!(cycles.remaining, 1);
        assert_eq!(store.restore("cluster", 10).unwrap().1, None);
        assert!(matches!(
            cycles
                .step(&settings(), &mut store, false, 7200, |_, _, _| Ok(
                    String::new()
                ))
                .unwrap(),
            Outcome::Committed { events: 0, .. }
        ));
        assert_eq!(
            cycles
                .step(&settings(), &mut store, false, 7300, unused)
                .unwrap(),
            Outcome::Exhausted
        );
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("1970-01-01T02:00:00")
        );
    }

    #[test]
    fn failures_consume_budget_and_restart_uses_last_committed_window() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(3).unwrap();
        let fixture = "cluster|1|alice|s|actual|COMPLETED|0:0";
        let query = |command: &mut Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                String::new()
            } else {
                fixture.into()
            })
        };
        assert!(matches!(
            cycles
                .step(&settings(), &mut store, false, 7200, query)
                .unwrap(),
            Outcome::Committed { events: 1, .. }
        ));
        assert!(matches!(
            cycles.step(&settings(), &mut store, false, 7260, |_, _, _| Err(
                QueryError::Timeout
            )),
            Err(Error::Poll(DurablePollError::Poll(_)))
        ));
        assert_eq!(cycles.remaining, 1);
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("1970-01-01T02:00:00")
        );
        drop(store);
        let mut store = open(&directory);
        match cycles
            .step(&settings(), &mut store, false, 7320, query)
            .unwrap()
        {
            Outcome::Committed { window, events } => {
                assert_eq!(events, 0);
                assert_eq!(window.start, "1970-01-01T01:55:00");
                assert!(!window.catch_up_gap);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(store.due_deliveries("cluster", 7320, 10).unwrap().len(), 1);
    }

    #[test]
    fn downtime_gap_is_visible_and_regressed_clock_never_queries() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(3).unwrap();
        cycles
            .step(&settings(), &mut store, false, 7200, |_, _, _| {
                Ok(String::new())
            })
            .unwrap();
        assert!(matches!(
            cycles.step(&settings(), &mut store, false, 7199, |_, _, _| panic!(
                "regressed query"
            )),
            Err(Error::Window(accounting_window::Error::ClockRegressed))
        ));
        match cycles
            .step(&settings(), &mut store, false, 14400, |_, _, _| {
                Ok(String::new())
            })
            .unwrap()
        {
            Outcome::Committed { window, .. } => {
                assert!(window.catch_up_gap);
                assert!(window.overlap_clipped);
                assert_eq!(window.start, "1970-01-01T03:00:00");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn failed_commit_preserves_checkpoint_and_retry_has_one_event() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER reject_checkpoint BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        let mut cycles = Cycles::new(2).unwrap();
        let query = |command: &mut Command, _, _| {
            Ok(if command.get_program() == "squeue" {
                String::new()
            } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0".into()
            })
        };
        assert!(matches!(
            cycles.step(&settings(), &mut store, false, 7200, query),
            Err(Error::Poll(DurablePollError::Storage(_)))
        ));
        assert_eq!(store.restore("cluster", 10).unwrap().1, None);
        assert!(store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .is_empty());
        db.execute_batch("DROP TRIGGER reject_checkpoint;").unwrap();
        assert!(matches!(
            cycles
                .step(&settings(), &mut store, false, 7260, query)
                .unwrap(),
            Outcome::Committed { events: 1, .. }
        ));
        assert_eq!(store.due_deliveries("cluster", 7260, 10).unwrap().len(), 1);
    }

    #[test]
    fn corrupt_storage_is_reported_before_queries() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("DROP TABLE checkpoints;").unwrap();
        assert!(matches!(
            Cycles::new(1)
                .unwrap()
                .step(&settings(), &mut store, false, 7200, |_, _, _| panic!(
                    "storage failure query"
                )),
            Err(Error::Storage(StorageError::Database(_)))
        ));
    }
}
