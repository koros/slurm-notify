//! Private caller-driven cycles: no background loop, scheduling default, or CLI.
use super::{
    accounting_window::{self, Policy, Window},
    scheduler::{poll_durable_with, DurablePollError, PollRequest, QueryError},
    storage::{StorageError, Store},
};
use std::{path::Path, process::Command, time::Duration};

pub(super) struct Settings<'a> {
    pub(super) active_program: &'a Path,
    pub(super) accounting_program: &'a Path,
    pub(super) cluster: &'a str,
    pub(super) user: &'a str,
    pub(super) timeout: Duration,
    pub(super) output_limit: usize,
    pub(super) emit_discovered: bool,
    pub(super) capacity: usize,
    pub(super) destinations: &'a [&'a str],
    pub(super) window: Policy,
}

#[derive(Debug)]
pub(super) enum Error {
    InvalidLimit,
    Storage(StorageError),
    Window(accounting_window::Error),
    Poll(DurablePollError),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Cancelled,
    Exhausted,
    Committed { window: Window, events: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
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
pub(super) struct PollReport {
    pub(super) consecutive_failures: u64,
    pub(super) last_failure: Option<Failure>,
    pub(super) checkpoint_age_seconds: Option<i64>,
    pub(super) stale: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ReportError {
    InvalidInput,
    ClockRegressed,
    Storage,
}

#[derive(Debug)]
pub(super) struct HealthReport {
    pub(super) poll: PollReport,
    pub(super) storage: super::storage::StorageHealth,
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

pub(super) struct Maintenance {
    pub(super) delivery_limit: usize,
    pub(super) retry: super::delivery::RetryPolicy,
    pub(super) retain_seconds: i64,
    pub(super) cleanup_limit: usize,
    pub(super) stale_after: i64,
    pub(super) health_sample_limit: usize,
}

pub(super) struct Hooks<Q, S, C, X> {
    pub(super) query: Q,
    pub(super) send: S,
    pub(super) clock: C,
    pub(super) cancelled: X,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StageError {
    Poll,
    Storage,
    Time,
    Policy,
    DispatchBlocked,
}

#[derive(Debug, Default)]
pub(super) struct TickReport {
    pub(super) cancelled: bool,
    pub(super) delivery_failure: Option<super::delivery::BatchFailure>,
    pub(super) preflight: Option<Result<(), StageError>>,
    pub(super) poll: Option<Result<Outcome, StageError>>,
    pub(super) delivery: Option<Result<super::delivery::BatchReport, StageError>>,
    pub(super) cleanup: Option<Result<usize, StageError>>,
    pub(super) health: Option<Result<HealthReport, StageError>>,
}

pub(super) struct Cycles {
    remaining: usize,
    health: PollHealth,
}

fn valid_settings(settings: &Settings<'_>, maintenance: &Maintenance) -> bool {
    let probe = |limit: usize| {
        limit > 0
            && i64::try_from(limit)
                .ok()
                .and_then(|n| n.checked_add(1))
                .is_some()
    };
    let mut destinations = std::collections::HashSet::new();
    !settings.active_program.as_os_str().is_empty()
        && !settings.accounting_program.as_os_str().is_empty()
        && !settings.cluster.is_empty()
        && !settings.user.is_empty()
        && !settings.timeout.is_zero()
        && settings.output_limit > 0
        && settings.output_limit.checked_add(1).is_some()
        && probe(settings.capacity)
        && settings.window.valid()
        && !settings.destinations.is_empty()
        && settings
            .destinations
            .iter()
            .all(|name| !name.is_empty() && destinations.insert(*name))
        && maintenance.retry.valid()
        && probe(maintenance.delivery_limit)
        && probe(maintenance.cleanup_limit)
        && probe(maintenance.health_sample_limit)
        && maintenance.retain_seconds > 0
        && maintenance.stale_after > 0
}

impl Cycles {
    pub(super) fn new(limit: usize) -> Result<Self, Error> {
        if limit == 0 {
            return Err(Error::InvalidLimit);
        }
        Ok(Self {
            remaining: limit,
            health: PollHealth::default(),
        })
    }

    // Existing infallible fixtures delegate to the same orchestration implementation.
    pub(super) fn tick<Q, S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        hooks: Hooks<Q, S, C, X>,
    ) -> TickReport
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        S: FnMut(&super::storage::DueDelivery) -> super::delivery::ProviderOutcome,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let Hooks {
            query,
            mut send,
            clock,
            cancelled,
        } = hooks;
        self.tick_fallible(
            settings,
            maintenance,
            store,
            Hooks {
                query,
                send: |item: &super::storage::DueDelivery| Ok(send(item)),
                clock,
                cancelled,
            },
        )
    }

    // Serialized caller-driven orchestration. No provider call holds a DB transaction.
    pub(super) fn tick_fallible<Q, S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mut hooks: Hooks<Q, S, C, X>,
    ) -> TickReport
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        S: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        use super::delivery::{deliver_due_detailed, FailureKind};
        let mut report = TickReport::default();
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        if !valid_settings(settings, maintenance) {
            report.preflight = Some(Err(StageError::Policy));
            return report;
        }
        let Some(now) = (hooks.clock)().filter(|time| *time >= 0) else {
            report.preflight = Some(Err(StageError::Time));
            return report;
        };
        if accounting_window::plan(now, None, settings.window).is_err() {
            report.preflight = Some(Err(StageError::Time));
            return report;
        }
        if !maintenance.retry.valid_at(now) {
            report.preflight = Some(Err(StageError::Policy));
            return report;
        }
        report.preflight = Some(Ok(()));
        let poll = self.step(settings, store, false, now, &mut hooks.query);
        let stop = matches!(
            &poll,
            Err(Error::Storage(_) | Error::Window(_) | Error::Poll(DurablePollError::Storage(_)))
        );
        report.poll = Some(poll.map_err(|error| match error {
            Error::Storage(_) | Error::Poll(DurablePollError::Storage(_)) => StageError::Storage,
            Error::Window(_) => StageError::Time,
            _ => StageError::Poll,
        }));
        if stop {
            return report;
        }
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        let mut latest = now;
        // Validate every later clock sample, including worker completion samples.
        let mut clock = || {
            let next = (hooks.clock)()?;
            if next < latest {
                return None;
            }
            latest = next;
            Some(next)
        };
        let Some(delivery_now) = clock() else {
            report.delivery = Some(Err(StageError::Time));
            return report;
        };
        let delivery = deliver_due_detailed(
            store,
            settings.cluster,
            delivery_now,
            maintenance.delivery_limit,
            maintenance.retry,
            &mut hooks.send,
            &mut clock,
        );
        let stop = delivery.is_err();
        report.delivery = Some(match delivery {
            Ok(batch) => Ok(batch),
            Err(failure) => {
                let error = match failure.kind {
                    FailureKind::Storage => StageError::Storage,
                    FailureKind::Time => StageError::Time,
                    FailureKind::Policy => StageError::Policy,
                    FailureKind::DispatchBlocked => StageError::DispatchBlocked,
                };
                report.delivery_failure = Some(failure);
                Err(error)
            }
        });
        if stop {
            return report;
        }
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        let Some(cleanup_now) = clock() else {
            report.cleanup = Some(Err(StageError::Time));
            return report;
        };
        let cleanup = store
            .retention_plan(
                settings.cluster,
                cleanup_now,
                maintenance.retain_seconds,
                maintenance.cleanup_limit,
            )
            .and_then(|plan| {
                store.apply_retention(
                    settings.cluster,
                    cleanup_now,
                    maintenance.retain_seconds,
                    maintenance.cleanup_limit,
                    &plan,
                )
            });
        report.cleanup = Some(cleanup.map_err(|_| StageError::Storage));
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        let Some(health_now) = clock() else {
            report.health = Some(Err(StageError::Time));
            return report;
        };
        report.health = Some(
            self.report(
                settings,
                store,
                health_now,
                maintenance.stale_after,
                maintenance.retain_seconds,
                maintenance.health_sample_limit,
            )
            .map_err(|error| match error {
                ReportError::Storage => StageError::Storage,
                ReportError::ClockRegressed => StageError::Time,
                ReportError::InvalidInput => StageError::Policy,
            }),
        );
        report
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

    fn maintenance() -> Maintenance {
        Maintenance {
            delivery_limit: 10,
            retry: super::super::delivery::RetryPolicy {
                max_attempts: 3,
                initial_delay_seconds: 5,
                max_delay_seconds: 20,
                jitter_seconds: 0,
                jitter_seed: 0,
            },
            retain_seconds: 50,
            cleanup_limit: 10,
            stale_after: 60,
            health_sample_limit: 10,
        }
    }

    #[test]
    fn invalid_preflight_policies_never_query_send_sample_clock_or_mutate_state() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        for case in 0..27 {
            let mut settings = settings();
            let mut maintenance = maintenance();
            match case {
                0 => settings.active_program = Path::new(""),
                1 => settings.accounting_program = Path::new(""),
                2 => settings.cluster = "",
                3 => settings.user = "",
                4 => settings.timeout = Duration::ZERO,
                5 => settings.output_limit = 0,
                6 => settings.output_limit = usize::MAX,
                7 => settings.capacity = 0,
                8 => settings.capacity = usize::MAX,
                9 => settings.window.lookback_seconds = 0,
                10 => settings.window.overlap_seconds = 0,
                11 => settings.window.overlap_seconds = settings.window.lookback_seconds,
                12 => settings.destinations = &[],
                13 => settings.destinations = &[""],
                14 => settings.destinations = &["primary", "primary"],
                15 => maintenance.retry.max_attempts = 0,
                16 => maintenance.retry.initial_delay_seconds = 0,
                17 => maintenance.retry.max_delay_seconds = 1,
                18 => maintenance.retry.jitter_seconds = -1,
                19 => maintenance.retry.jitter_seconds = 21,
                20 => maintenance.delivery_limit = 0,
                21 => maintenance.cleanup_limit = 0,
                22 => maintenance.health_sample_limit = 0,
                23 => maintenance.delivery_limit = usize::MAX,
                24 => maintenance.health_sample_limit = usize::MAX,
                25 => maintenance.retain_seconds = 0,
                _ => maintenance.stale_after = 0,
            }
            let report = cycles.tick(
                &settings,
                &maintenance,
                &mut store,
                Hooks {
                    query: |_: &mut Command, _, _| panic!("invalid preflight query"),
                    send: |_: &super::super::storage::DueDelivery| panic!("invalid preflight send"),
                    clock: || panic!("invalid preflight clock"),
                    cancelled: || false,
                },
            );
            assert_eq!(
                report.preflight.unwrap(),
                Err(StageError::Policy),
                "case {case}"
            );
            assert!(
                report.poll.is_none()
                    && report.delivery.is_none()
                    && report.cleanup.is_none()
                    && report.health.is_none()
            );
            assert_eq!(cycles.remaining, 1);
            assert_eq!(cycles.health.consecutive_failures, 0);
            assert_eq!(store.restore("cluster", 10).unwrap().1, None);
        }
    }

    #[test]
    fn preflight_time_and_retry_overflow_reject_before_poll_and_valid_boundaries_run() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        for now in [None, Some(-1), Some(0), Some(i64::MAX)] {
            let report = cycles.tick(
                &settings(),
                &maintenance(),
                &mut store,
                Hooks {
                    query: |_: &mut Command, _, _| panic!("invalid time query"),
                    send: |_: &super::super::storage::DueDelivery| panic!("invalid time send"),
                    clock: || now,
                    cancelled: || false,
                },
            );
            assert_eq!(report.preflight.unwrap(), Err(StageError::Time));
            assert!(report.poll.is_none());
            assert_eq!(cycles.remaining, 1);
        }
        let mut limits = maintenance();
        limits.retry.max_delay_seconds = i64::MAX;
        let report = cycles.tick(
            &settings(),
            &limits,
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("overflow query"),
                send: |_: &super::super::storage::DueDelivery| panic!("overflow send"),
                clock: || Some(7200),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Err(StageError::Policy));
        assert_eq!(cycles.remaining, 1);
        let mut settings = settings();
        settings.capacity = 1;
        settings.output_limit = 1;
        settings.window = Policy {
            lookback_seconds: 2,
            overlap_seconds: 1,
        };
        let mut limits = maintenance();
        limits.delivery_limit = 1;
        limits.cleanup_limit = 1;
        limits.health_sample_limit = 1;
        limits.stale_after = 1;
        limits.retain_seconds = 1;
        limits.retry.max_attempts = 1;
        limits.retry.initial_delay_seconds = 1;
        limits.retry.max_delay_seconds = 1;
        limits.retry.jitter_seconds = 1;
        let report = cycles.tick(
            &settings,
            &limits,
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Ok(String::new()),
                send: |_: &super::super::storage::DueDelivery| panic!("empty queue send"),
                clock: || Some(1),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Ok(()));
        assert!(report.poll.unwrap().is_ok() && report.health.unwrap().is_ok());
        assert_eq!(cycles.remaining, 0);
    }

    #[test]
    fn orchestration_delivers_despite_scheduler_failure_and_reopens_cleanly() {
        use super::super::delivery::ProviderOutcome;
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(3).unwrap();
        let first = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        String::new()
                    } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0".into()
                    })
                },
                send: |_: &super::super::storage::DueDelivery| ProviderOutcome::Retry,
                clock: || Some(7200),
                cancelled: || false,
            },
        );
        assert_eq!(first.delivery.unwrap().unwrap().retrying, 1);
        assert_eq!(first.cleanup.unwrap().unwrap(), 0);
        assert!(first.health.unwrap().is_ok());
        drop(store);
        let mut store = open(&directory);
        let second = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Err(QueryError::Timeout),
                send: |_: &super::super::storage::DueDelivery| ProviderOutcome::Delivered,
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(second.poll.unwrap().unwrap_err(), StageError::Poll);
        assert_eq!(second.delivery.unwrap().unwrap().delivered, 1);
        let health = second.health.unwrap().unwrap();
        assert_eq!(health.poll.consecutive_failures, 1);
        let third = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Ok(String::new()),
                send: |_: &super::super::storage::DueDelivery| panic!("duplicate send"),
                clock: || Some(7400),
                cancelled: || false,
            },
        );
        assert_eq!(third.cleanup.unwrap().unwrap(), 1);
        assert_eq!(third.health.unwrap().unwrap().poll.consecutive_failures, 0);
        assert!(store
            .due_deliveries("cluster", 7400, 10)
            .unwrap()
            .is_empty());
        assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
    }

    #[test]
    fn orchestration_cancellation_between_stages_preserves_committed_work() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        let early = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("cancelled query"),
                send: |_: &super::super::storage::DueDelivery| panic!("cancelled send"),
                clock: || panic!("cancelled clock"),
                cancelled: || true,
            },
        );
        assert!(early.cancelled && early.poll.is_none());
        assert_eq!(cycles.remaining, 2);
        let mut checks = 0;
        let between = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Ok(String::new()),
                send: |_: &super::super::storage::DueDelivery| panic!("cancelled send"),
                clock: || Some(7200),
                cancelled: || {
                    checks += 1;
                    checks == 2
                },
            },
        );
        assert!(between.cancelled && between.poll.unwrap().is_ok());
        assert!(
            between.delivery.is_none() && between.cleanup.is_none() && between.health.is_none()
        );
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("1970-01-01T02:00:00")
        );
    }

    #[test]
    fn orchestration_stops_after_uncertain_send_commit_and_regressed_clock() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER reject_send BEFORE UPDATE ON outbox BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        let mut cycles = Cycles::new(2).unwrap();
        let mut sent = 0;
        let report = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        String::new()
                    } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0".into()
                    })
                },
                send: |_: &super::super::storage::DueDelivery| {
                    sent += 1;
                    super::super::delivery::ProviderOutcome::Delivered
                },
                clock: || Some(7200),
                cancelled: || false,
            },
        );
        assert_eq!(sent, 1);
        assert_eq!(report.delivery.unwrap().unwrap_err(), StageError::Storage);
        assert!(report.cleanup.is_none() && report.health.is_none());
        assert_eq!(
            store.due_deliveries("cluster", 7200, 10).unwrap()[0].attempt_number,
            1
        );
        db.execute_batch("DROP TRIGGER reject_send;").unwrap();
        let mut times = [Some(7300), Some(7299)].into_iter();
        let report = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Ok(String::new()),
                send: |_: &super::super::storage::DueDelivery| panic!("regressed send"),
                clock: || times.next().unwrap(),
                cancelled: || false,
            },
        );
        assert_eq!(report.delivery.unwrap().unwrap_err(), StageError::Time);
        assert!(report.cleanup.is_none());
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
