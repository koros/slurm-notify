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
    Preparation,
    PreparationLimit,
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
    Preparation,
    PreparationLimit,
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
    Capacity,
    Cancelled,
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
    pub(super) capacity_pressure: Option<super::storage::StorageHealth>,
    pub(super) preflight: Option<Result<(), StageError>>,
    pub(super) poll: Option<Result<Outcome, StageError>>,
    pub(super) delivery: Option<Result<super::delivery::BatchReport, StageError>>,
    pub(super) cleanup: Option<Result<usize, StageError>>,
    pub(super) health: Option<Result<HealthReport, StageError>>,
}

// Separate private fixture hooks: cycle destinations stay authoritative.
pub(super) struct PreparedHooks<'a, Q, P> {
    pub(super) smtp_destinations: &'a [&'a str],
    pub(super) max_messages: usize,
    pub(super) query: Q,
    pub(super) prepare: P,
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
        hooks: Hooks<Q, S, C, X>,
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
        let Hooks {
            mut query,
            send,
            clock,
            cancelled,
        } = hooks;
        self.tick_using(
            settings,
            maintenance,
            store,
            Hooks {
                query: (),
                send,
                clock,
                cancelled,
            },
            true,
            |cycles, store, now| cycles.step(settings, store, false, now, &mut query),
        )
    }

    pub(super) fn tick_prepared_fallible<Q, P, S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        prepared: PreparedHooks<'_, Q, P>,
        hooks: Hooks<(), S, C, X>,
    ) -> TickReport
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        P: FnMut(&super::lifecycle::Event, &str) -> Result<super::smtp_message::Prepared, ()>,
        S: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let PreparedHooks {
            smtp_destinations,
            max_messages,
            mut query,
            mut prepare,
        } = prepared;
        let valid = super::smtp_poll::valid_settings(&super::smtp_poll::Settings {
            destinations: settings.destinations,
            smtp_destinations,
            max_messages,
        });
        self.tick_using(
            settings,
            maintenance,
            store,
            hooks,
            valid,
            |cycles, store, now| {
                cycles.step_prepared(
                    settings,
                    store,
                    false,
                    now,
                    PreparedHooks {
                        smtp_destinations,
                        max_messages,
                        query: &mut query,
                        prepare: &mut prepare,
                    },
                )
            },
        )
    }

    fn tick_using<S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mut hooks: Hooks<(), S, C, X>,
        valid_poll: bool,
        mut poll: impl FnMut(&mut Self, &mut Store, i64) -> Result<Outcome, Error>,
    ) -> TickReport
    where
        S: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        use super::delivery::{deliver_due_controlled, DeliveryHooks, FailureKind};
        let mut report = TickReport::default();
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        if !valid_poll || !valid_settings(settings, maintenance) {
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
        let storage_health = match store.health(
            settings.cluster,
            now,
            maintenance.retain_seconds,
            settings.capacity,
            maintenance.health_sample_limit,
        ) {
            Ok(health) => health,
            Err(_) => {
                report.preflight = Some(Err(StageError::Storage));
                return report;
            }
        };
        if storage_health.jobs_over_capacity || storage_health.events_over_capacity {
            report.preflight = Some(Err(StageError::Capacity));
            report.capacity_pressure = Some(storage_health);
            return report;
        }
        // Exhausted polling still requires a coherent ledger and valid checkpoint
        // before delivery or maintenance; do not rely on step() to restore it.
        let (_, checkpoint) = match store.restore(settings.cluster, settings.capacity) {
            Ok(restored) => restored,
            Err(_) => {
                report.preflight = Some(Err(StageError::Storage));
                return report;
            }
        };
        if accounting_window::plan(now, checkpoint.as_deref(), settings.window).is_err() {
            report.preflight = Some(Err(StageError::Time));
            return report;
        }
        report.preflight = Some(Ok(()));
        let poll = poll(self, store, now);
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
        let delivery = deliver_due_controlled(
            store,
            settings.cluster,
            delivery_now,
            maintenance.delivery_limit,
            maintenance.retry,
            DeliveryHooks {
                send: &mut hooks.send,
                clock: &mut clock,
                cancelled: &mut hooks.cancelled,
            },
        );
        let stop = delivery.is_err();
        report.delivery = Some(match delivery {
            Ok(batch) => Ok(batch),
            Err(failure) => {
                if failure.kind == FailureKind::Cancelled {
                    report.cancelled = true;
                }
                let error = match failure.kind {
                    FailureKind::Cancelled => StageError::Cancelled,
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
        self.step_using(
            settings,
            store,
            cancelled,
            now,
            |request, reconciler, store| {
                poll_durable_with(
                    request,
                    reconciler,
                    store,
                    settings.destinations,
                    now,
                    query,
                )
                .map_err(Error::Poll)
            },
        )
    }

    pub(super) fn step_prepared<Q, P>(
        &mut self,
        settings: &Settings<'_>,
        store: &mut Store,
        cancelled: bool,
        now: i64,
        hooks: PreparedHooks<'_, Q, P>,
    ) -> Result<Outcome, Error>
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        P: FnMut(&super::lifecycle::Event, &str) -> Result<super::smtp_message::Prepared, ()>,
    {
        self.step_using(
            settings,
            store,
            cancelled,
            now,
            |request, reconciler, store| {
                super::smtp_poll::poll_with(
                    request,
                    reconciler,
                    store,
                    &super::smtp_poll::Settings {
                        destinations: settings.destinations,
                        smtp_destinations: hooks.smtp_destinations,
                        max_messages: hooks.max_messages,
                    },
                    now,
                    hooks.query,
                    hooks.prepare,
                )
                .map_err(|error| match error {
                    super::smtp_poll::Error::InvalidInput => Error::Poll(DurablePollError::Poll(
                        super::scheduler::PollError::InvalidRequest,
                    )),
                    super::smtp_poll::Error::Poll(error) => {
                        Error::Poll(DurablePollError::Poll(error))
                    }
                    super::smtp_poll::Error::Storage(error) => {
                        Error::Poll(DurablePollError::Storage(error))
                    }
                    super::smtp_poll::Error::Preparation => Error::Preparation,
                    super::smtp_poll::Error::Limit => Error::PreparationLimit,
                })
            },
        )
    }

    fn step_using(
        &mut self,
        settings: &Settings<'_>,
        store: &mut Store,
        cancelled: bool,
        now: i64,
        poll: impl FnOnce(
            &PollRequest<'_>,
            &mut super::lifecycle::Reconciler,
            &mut Store,
        ) -> Result<Vec<super::lifecycle::Event>, Error>,
    ) -> Result<Outcome, Error> {
        let result = self.step_inner(settings, store, cancelled, now, poll);
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
                    Error::Preparation => Failure::Preparation,
                    Error::PreparationLimit => Failure::PreparationLimit,
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
        poll: impl FnOnce(
            &PollRequest<'_>,
            &mut super::lifecycle::Reconciler,
            &mut Store,
        ) -> Result<Vec<super::lifecycle::Event>, Error>,
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
        let events = poll(&request, &mut reconciler, store)?;
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

    fn smtp_message(
        _: &super::super::lifecycle::Event,
        _: &str,
    ) -> Result<super::super::smtp_message::Prepared, ()> {
        super::super::smtp_message::prepare(
            "s@fixture.invalid",
            "r@fixture.invalid",
            "fixture",
            "done",
            "original\n.\n.dot",
            123,
        )
        .map_err(|_| ())
    }
    fn smtp_query(command: &mut Command, _: Duration, _: usize) -> Result<String, QueryError> {
        Ok(if command.get_program() == "squeue" {
            ""
        } else {
            "cluster|1|alice|s|actual|COMPLETED|0:0"
        }
        .into())
    }
    #[test]
    fn prepared_cycles_preserve_checkpoint_health_on_failure_then_recover_after_reopen() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        assert!(matches!(
            cycles.step_prepared(
                &settings(),
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: smtp_query,
                    prepare: smtp_message
                }
            ),
            Ok(Outcome::Committed { events: 1, .. })
        ));
        let first = store.due_deliveries("cluster", 7200, 10).unwrap().remove(0);
        let checkpoint = store.restore("cluster", 10).unwrap().1.unwrap();
        assert!(matches!(
            cycles.step_prepared(
                &settings(),
                &mut store,
                false,
                7260,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: |command: &mut Command, _, _| Ok(if command.get_program() == "squeue" {
                        ""
                    } else {
                        "cluster|2|alice|s|actual|COMPLETED|0:0"
                    }
                    .into()),
                    prepare: |_: &super::super::lifecycle::Event, _: &str| Err(()),
                }
            ),
            Err(Error::Preparation)
        ));
        let health = cycles
            .report(&settings(), &mut store, 7260, 60, 3600, 10)
            .unwrap();
        assert_eq!(health.poll.last_failure, Some(Failure::Preparation));
        assert_eq!(health.poll.consecutive_failures, 1);
        assert_eq!(health.poll.checkpoint_age_seconds, Some(60));
        assert!(health.poll.stale);
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some(checkpoint.as_str())
        );
        assert_eq!(store.due_deliveries("cluster", 7260, 10).unwrap().len(), 1);
        drop(store);
        let mut store = open(&directory);
        let mut restarted = Cycles::new(1).unwrap();
        assert!(matches!(
            restarted.step_prepared(
                &settings(),
                &mut store,
                false,
                7320,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: |command: &mut Command, _, _| Ok(if command.get_program() == "squeue" {
                        ""
                    } else {
                        "cluster|2|alice|s|actual|COMPLETED|0:0"
                    }
                    .into()),
                    prepare: smtp_message,
                }
            ),
            Ok(Outcome::Committed { events: 1, .. })
        ));
        let health = restarted
            .report(&settings(), &mut store, 7320, 60, 3600, 10)
            .unwrap();
        assert_eq!(health.poll.last_failure, None);
        assert_eq!(health.poll.consecutive_failures, 0);
        assert_eq!(health.poll.checkpoint_age_seconds, Some(0));
        assert!(!health.poll.stale);
        assert_eq!(store.due_deliveries("cluster", 7320, 10).unwrap().len(), 2);
        assert!(
            store
                .frozen_smtp(&first.event_id, &first.destination)
                .unwrap()
                .unwrap()
                .data()
                == smtp_message(&first.event, &first.destination)
                    .unwrap()
                    .data()
        );
    }
    #[test]
    fn prepared_cycle_limit_consumes_budget_but_cancellation_and_exhaustion_do_not_query() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        assert_eq!(
            cycles
                .step_prepared(
                    &settings(),
                    &mut store,
                    true,
                    7200,
                    PreparedHooks {
                        smtp_destinations: &[],
                        max_messages: 0,
                        query: |_: &mut Command, _, _| panic!("cancelled query"),
                        prepare: |_: &super::super::lifecycle::Event, _: &str| panic!(
                            "cancelled preparation"
                        )
                    }
                )
                .unwrap(),
            Outcome::Cancelled
        );
        assert_eq!(cycles.remaining, 1);
        let mut settings = settings();
        settings.destinations = &["a", "b"];
        assert!(matches!(
            cycles.step_prepared(
                &settings,
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["a", "b"],
                    max_messages: 1,
                    query: smtp_query,
                    prepare: |_: &super::super::lifecycle::Event, _: &str| panic!(
                        "limit must precede preparation"
                    )
                }
            ),
            Err(Error::PreparationLimit)
        ));
        assert_eq!(cycles.remaining, 0);
        assert_eq!(cycles.health.last_failure, Some(Failure::PreparationLimit));
        assert!(store.restore("cluster", 10).unwrap().1.is_none());
        assert_eq!(
            cycles
                .step_prepared(
                    &settings,
                    &mut store,
                    false,
                    7260,
                    PreparedHooks {
                        smtp_destinations: &[],
                        max_messages: 0,
                        query: |_: &mut Command, _, _| panic!("exhausted query"),
                        prepare: |_: &super::super::lifecycle::Event, _: &str| panic!(
                            "exhausted preparation"
                        )
                    }
                )
                .unwrap(),
            Outcome::Exhausted
        );
        assert_eq!(cycles.health.consecutive_failures, 1);
    }
    #[test]
    fn prepared_cycle_classifies_invalid_routes_query_and_storage_failures() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(4).unwrap();
        assert!(matches!(
            cycles.step_prepared(
                &settings(),
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["absent"],
                    max_messages: 1,
                    query: |_: &mut Command, _, _| panic!("invalid route queried"),
                    prepare: smtp_message
                }
            ),
            Err(Error::Poll(DurablePollError::Poll(
                super::super::scheduler::PollError::InvalidRequest
            )))
        ));
        assert_eq!(cycles.health.last_failure, Some(Failure::InvalidRequest));
        assert!(cycles
            .step_prepared(
                &settings(),
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: |_: &mut Command, _, _| Err(QueryError::Io),
                    prepare: smtp_message
                }
            )
            .is_err());
        assert_eq!(cycles.health.last_failure, Some(Failure::ActiveQuery));
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("CREATE TRIGGER stop_cycle BEFORE INSERT ON checkpoints BEGIN SELECT RAISE(ABORT,'private-fixture'); END;").unwrap();
        assert!(matches!(
            cycles.step_prepared(
                &settings(),
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: smtp_query,
                    prepare: smtp_message
                }
            ),
            Err(Error::Poll(DurablePollError::Storage(_)))
        ));
        assert_eq!(cycles.health.last_failure, Some(Failure::Storage));
        assert_eq!(cycles.health.consecutive_failures, 3);
        assert!(store.restore("cluster", 10).unwrap().1.is_none());
        assert!(store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .is_empty());
        db.execute_batch("DROP TRIGGER stop_cycle").unwrap();
        cycles
            .step_prepared(
                &settings(),
                &mut store,
                false,
                7260,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: smtp_query,
                    prepare: smtp_message,
                },
            )
            .unwrap();
        assert_eq!(cycles.health.consecutive_failures, 0);
    }

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
    fn prepared_tick_cancellation_preserves_frozen_pending_delivery_after_reopen() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let mut cancellations = 0;
        let report = cycles.tick_prepared_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            PreparedHooks {
                smtp_destinations: &["primary"],
                max_messages: 1,
                query: smtp_query,
                prepare: |event: &super::super::lifecycle::Event, route: &str| {
                    smtp_message(event, route)
                },
            },
            Hooks {
                query: (),
                send: |_: &super::super::storage::DueDelivery| panic!("cancelled before delivery"),
                clock: || Some(7200),
                cancelled: || {
                    cancellations += 1;
                    cancellations > 1
                },
            },
        );
        assert!(report.cancelled);
        assert!(matches!(
            report.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        drop(store);
        let mut store = open(&directory);
        let due = store.due_deliveries("cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 1);
        assert!(store
            .frozen_smtp(&due[0].event_id, &due[0].destination)
            .unwrap()
            .is_some());
        let report = cycles.tick_prepared_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            PreparedHooks {
                smtp_destinations: &["primary"],
                max_messages: 1,
                query: |_: &mut Command, _, _| panic!("exhausted"),
                prepare: |_: &super::super::lifecycle::Event, _: &str| panic!("exhausted"),
            },
            Hooks {
                query: (),
                send: |_: &super::super::storage::DueDelivery| {
                    Ok(super::super::delivery::ProviderOutcome::Delivered)
                },
                clock: || Some(7201),
                cancelled: || false,
            },
        );
        assert!(matches!(report.poll, Some(Ok(Outcome::Exhausted))));
        assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
        assert!(report.health.unwrap().is_ok());
    }

    #[test]
    fn prepared_tick_rejects_invalid_routes_before_maintenance() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let report = cycles.tick_prepared_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            PreparedHooks {
                smtp_destinations: &["unknown"],
                max_messages: 1,
                query: |_: &mut Command, _, _| panic!("invalid policy"),
                prepare: |_: &super::super::lifecycle::Event, _: &str| panic!("invalid policy"),
            },
            Hooks {
                query: (),
                send: |_: &super::super::storage::DueDelivery| panic!("invalid policy"),
                clock: || panic!("invalid policy"),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight, Some(Err(StageError::Policy)));
        assert_eq!(cycles.remaining, 1);
    }

    #[test]
    fn prepared_tick_failure_still_delivers_earlier_pending_work() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        cycles
            .step_prepared(
                &settings(),
                &mut store,
                false,
                7200,
                PreparedHooks {
                    smtp_destinations: &["primary"],
                    max_messages: 1,
                    query: smtp_query,
                    prepare: smtp_message,
                },
            )
            .unwrap();
        let report = cycles.tick_prepared_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            PreparedHooks {
                smtp_destinations: &["primary"],
                max_messages: 1,
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        String::new()
                    } else {
                        "cluster|2|alice|s|actual|COMPLETED|0:0".into()
                    })
                },
                prepare: |_: &super::super::lifecycle::Event, _: &str| Err(()),
            },
            Hooks {
                query: (),
                send: |_: &super::super::storage::DueDelivery| {
                    Ok(super::super::delivery::ProviderOutcome::Delivered)
                },
                clock: || Some(7260),
                cancelled: || false,
            },
        );
        assert_eq!(report.poll.unwrap().unwrap_err(), StageError::Poll);
        assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
        let health = report.health.unwrap().unwrap();
        assert_eq!(health.poll.last_failure, Some(Failure::Preparation));
        assert_eq!(health.poll.checkpoint_age_seconds, Some(60));
        drop(store);
        let mut store = open(&directory);
        assert!(store
            .due_deliveries("cluster", 7260, 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("1970-01-01T02:00:00")
        );
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
    fn missing_checkpoint_allows_exhausted_maintenance_with_unknown_poll_age() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        cycles
            .step(&settings(), &mut store, false, 7200, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    String::new()
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0".into()
                })
            })
            .unwrap();
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute("DELETE FROM checkpoints", []).unwrap();
        let report = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("exhausted missing-checkpoint query"),
                send: |_: &super::super::storage::DueDelivery| {
                    super::super::delivery::ProviderOutcome::Delivered
                },
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Ok(()));
        assert_eq!(report.poll.unwrap().unwrap(), Outcome::Exhausted);
        assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
        let health = report.health.unwrap().unwrap();
        assert_eq!(health.poll.checkpoint_age_seconds, None);
        assert!(health.poll.stale);
        assert_eq!(store.restore("cluster", 10).unwrap().1, None);
    }

    #[test]
    fn cancellation_precedes_static_validation_and_storage_health_reads() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute_batch("DROP TABLE delivery_attempts;").unwrap();
        let mut invalid = settings();
        invalid.capacity = 0;
        let mut cycles = Cycles::new(1).unwrap();
        let report = cycles.tick(
            &invalid,
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("cancelled query"),
                send: |_: &super::super::storage::DueDelivery| panic!("cancelled send"),
                clock: || panic!("cancelled clock"),
                cancelled: || true,
            },
        );
        assert!(report.cancelled && report.preflight.is_none() && report.poll.is_none());
        assert_eq!(cycles.remaining, 1);
        assert_eq!(cycles.health.consecutive_failures, 0);
    }

    #[test]
    fn exhausted_polling_rejects_bad_checkpoints_and_recovers_without_queries() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        cycles
            .step(&settings(), &mut store, false, 7200, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    String::new()
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0".into()
                })
            })
            .unwrap();
        assert_eq!(cycles.remaining, 0);
        drop(store);
        let mut store = open(&directory);
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        for bad in [
            "1970-01-01T02:10:00",
            "malformed-fixture",
            "2026-02-30T01:00:00",
            "1969-12-31T23:59:59",
        ] {
            db.execute("UPDATE checkpoints SET history_through=?1", [bad])
                .unwrap();
            let report = cycles.tick(
                &settings(),
                &maintenance(),
                &mut store,
                Hooks {
                    query: |_: &mut Command, _, _| panic!("exhausted checkpoint query"),
                    send: |_: &super::super::storage::DueDelivery| {
                        panic!("invalid checkpoint send")
                    },
                    clock: || Some(7300),
                    cancelled: || false,
                },
            );
            assert_eq!(report.preflight.unwrap(), Err(StageError::Time));
            assert!(
                report.poll.is_none()
                    && report.delivery.is_none()
                    && report.cleanup.is_none()
                    && report.health.is_none()
            );
            assert_eq!(cycles.remaining, 0);
            assert_eq!(cycles.health.consecutive_failures, 0);
            assert_eq!(
                store.restore("cluster", 10).unwrap().1.as_deref(),
                Some(bad)
            );
            assert_eq!(
                store.due_deliveries("cluster", 7300, 10).unwrap()[0].attempt_number,
                1
            );
        }
        db.execute(
            "UPDATE checkpoints SET history_through='1970-01-01T02:00:00'",
            [],
        )
        .unwrap();
        let report = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("exhausted recovery query"),
                send: |item: &super::super::storage::DueDelivery| {
                    assert_eq!(item.attempt_number, 1);
                    super::super::delivery::ProviderOutcome::Delivered
                },
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Ok(()));
        assert_eq!(report.poll.unwrap().unwrap(), Outcome::Exhausted);
        assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
        assert!(report.health.unwrap().is_ok());
    }

    #[test]
    fn exhausted_polling_rejects_inconsistent_ledger_before_sending() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        cycles
            .step(&settings(), &mut store, false, 7200, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    String::new()
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0".into()
                })
            })
            .unwrap();
        let db = rusqlite::Connection::open(directory.path()).unwrap();
        db.execute("UPDATE events SET event_type='FAILED'", [])
            .unwrap();
        let report = cycles.tick(
            &settings(),
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("inconsistent ledger query"),
                send: |_: &super::super::storage::DueDelivery| panic!("inconsistent ledger send"),
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Err(StageError::Storage));
        assert!(report.poll.is_none() && report.delivery.is_none() && report.cleanup.is_none());
        assert_eq!(
            db.query_row("SELECT count(*) FROM delivery_attempts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(cycles.remaining, 0);
    }

    #[test]
    fn capacity_preflight_preserves_pending_history_and_recovers_with_explicit_limit() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        cycles
            .step(&settings(), &mut store, false, 7200, |command, _, _| {
                Ok(if command.get_program() == "squeue" {
                    String::new()
                } else {
                    "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0"
                        .into()
                })
            })
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        let mut smaller = settings();
        smaller.capacity = 1;
        let report = cycles.tick(
            &smaller,
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("pressure query"),
                send: |_: &super::super::storage::DueDelivery| panic!("pressure send"),
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(report.preflight.unwrap(), Err(StageError::Capacity));
        let pressure = report.capacity_pressure.unwrap();
        assert!(pressure.jobs_over_capacity && pressure.events_over_capacity);
        assert!(report.poll.is_none() && report.delivery.is_none() && report.cleanup.is_none());
        assert_eq!(cycles.remaining, 1);
        assert_eq!(cycles.health.consecutive_failures, 0);
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("1970-01-01T02:00:00")
        );
        assert_eq!(store.due_deliveries("cluster", 7300, 10).unwrap().len(), 2);
        smaller.capacity = 2;
        let recovered = cycles.tick(
            &smaller,
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| Ok(String::new()),
                send: |item: &super::super::storage::DueDelivery| {
                    assert_eq!(item.attempt_number, 1);
                    super::super::delivery::ProviderOutcome::Delivered
                },
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        assert_eq!(recovered.preflight.unwrap(), Ok(()));
        assert_eq!(recovered.delivery.unwrap().unwrap().delivered, 2);
        assert_eq!(store.restore("cluster", 2).unwrap().0.emitted.len(), 2);
    }

    #[test]
    fn event_history_pressure_is_visible_even_when_job_count_fits() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(3).unwrap();
        for (state, now) in [("RUNNING", 7200), ("COMPLETED", 7260)] {
            cycles
                .step(&settings(), &mut store, false, now, |command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        String::new()
                    } else {
                        format!("cluster|1|alice|s|actual|{state}|0:0")
                    })
                })
                .unwrap();
        }
        let mut smaller = settings();
        smaller.capacity = 1;
        let blocked = cycles.tick(
            &smaller,
            &maintenance(),
            &mut store,
            Hooks {
                query: |_: &mut Command, _, _| panic!("ledger pressure query"),
                send: |_: &super::super::storage::DueDelivery| panic!("ledger pressure send"),
                clock: || Some(7300),
                cancelled: || false,
            },
        );
        let pressure = blocked.capacity_pressure.unwrap();
        assert!(!pressure.jobs_over_capacity && pressure.events_over_capacity);
        assert_eq!(blocked.preflight.unwrap(), Err(StageError::Capacity));
        assert_eq!(store.restore("cluster", 2).unwrap().0.emitted.len(), 2);
        assert_eq!(cycles.remaining, 1);
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
