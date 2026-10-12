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
    pub(super) event_capacity: usize,
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

// Route inputs are validated after cancellation and before clocks/storage/query.
pub(super) struct Rendering<'a> {
    pub(super) routes: &'a [super::smtp_routes::Route<'a>],
    pub(super) max_messages: usize,
}

pub(super) struct BoundSmtp<'a> {
    pub(super) rendering: Rendering<'a>,
    pub(super) profiles: &'a [super::smtp_provider_revision::Profile<'a>],
    pub(super) permissions: &'a super::smtp_provider_delivery::Permissions<'a>,
    pub(super) timeout: Duration,
}

// Exact current SMTP destination membership; never fall back on an incomplete map.
fn valid_deadlines(
    routes: &[super::smtp_routes::Route<'_>],
    shared: Duration,
    entries: Option<&[(&str, Duration)]>,
) -> bool {
    let valid = |timeout: Duration| {
        !timeout.is_zero()
            && timeout <= Duration::from_secs(30)
            && timeout == Duration::from_millis(timeout.as_millis() as u64)
    };
    match entries {
        None => valid(shared),
        Some(entries) => {
            entries.len() <= 64
                && entries.len() == routes.len()
                && entries.iter().all(|(name, timeout)| {
                    valid(*timeout) && routes.iter().any(|r| r.name == *name)
                })
                && entries
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == entries.len()
        }
    }
}

pub(super) struct MixedBoundSmtp<'a> {
    pub(super) smtp: BoundSmtp<'a>,
    pub(super) ordinary_destinations: &'a [&'a str],
}

fn prepared_error(error: super::smtp_poll::Error) -> Error {
    match error {
        super::smtp_poll::Error::InvalidInput => Error::Poll(DurablePollError::Poll(
            super::scheduler::PollError::InvalidRequest,
        )),
        super::smtp_poll::Error::Poll(error) => Error::Poll(DurablePollError::Poll(error)),
        super::smtp_poll::Error::Storage(error) => Error::Poll(DurablePollError::Storage(error)),
        super::smtp_poll::Error::Preparation => Error::Preparation,
        super::smtp_poll::Error::Limit => Error::PreparationLimit,
    }
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
        && probe(settings.event_capacity)
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
            || Some(()),
            |cycles, store, now, _| cycles.step(settings, store, false, now, &mut query),
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
            || valid.then_some(()),
            |cycles, store, now, _| {
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

    pub(super) fn tick_rendered_fallible<Q, S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        rendering: Rendering<'_>,
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
            || {
                super::smtp_routes::Plan::new(
                    settings.destinations,
                    rendering.routes,
                    rendering.max_messages,
                )
                .ok()
            },
            |cycles, store, now, plan| {
                cycles.step_using(settings, store, false, now, |request, reconciler, store| {
                    plan.poll_with(request, reconciler, store, now, &mut query)
                        .map_err(prepared_error)
                })
            },
        )
    }

    // SMTP-only private composition; helper selection and resource lookup stay caller-owned.
    pub(super) fn tick_bound_smtp<Q, R, H, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        bound: BoundSmtp<'_>,
        hooks: Hooks<Q, (R, H), C, X>,
    ) -> TickReport
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        R: FnMut(
            &super::smtp_provider_revision::Descriptor,
        ) -> Result<
            super::smtp_provider_delivery::Resolved,
            super::smtp_provider_delivery::Error,
        >,
        H: FnMut(&super::storage::DueDelivery) -> Command,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        self.tick_mixed_bound_smtp(
            settings,
            maintenance,
            store,
            MixedBoundSmtp {
                smtp: bound,
                ordinary_destinations: &[],
            },
            Hooks {
                query: hooks.query,
                send: (
                    hooks.send.0,
                    hooks.send.1,
                    |_: &super::storage::DueDelivery| {
                        Err(super::supervised_provider::Diagnostic::SmtpProviderDenied)
                    },
                ),
                clock: hooks.clock,
                cancelled: hooks.cancelled,
            },
        )
    }

    // Explicit mixed routing; durable SMTP artifacts prohibit ordinary dispatch.
    pub(super) fn tick_mixed_bound_smtp<Q, R, H, O, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mixed: MixedBoundSmtp<'_>,
        hooks: Hooks<Q, (R, H, O), C, X>,
    ) -> TickReport
    where
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        R: FnMut(
            &super::smtp_provider_revision::Descriptor,
        ) -> Result<
            super::smtp_provider_delivery::Resolved,
            super::smtp_provider_delivery::Error,
        >,
        H: FnMut(&super::storage::DueDelivery) -> Command,
        O: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        self.tick_selected_mixed_bound_smtp(
            settings,
            maintenance,
            store,
            mixed,
            hooks,
            |_: &super::lifecycle::Event| settings.destinations.to_vec(),
        )
    }

    // Event-specific private routing; selection applies only to newly emitted events.
    pub(super) fn tick_selected_mixed_bound_smtp<'s, Q, R, H, O, C, X, S>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mixed: MixedBoundSmtp<'_>,
        hooks: Hooks<Q, (R, H, O), C, X>,
        select: S,
    ) -> TickReport
    where
        S: FnMut(&super::lifecycle::Event) -> Vec<&'s str>,
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        R: FnMut(
            &super::smtp_provider_revision::Descriptor,
        ) -> Result<
            super::smtp_provider_delivery::Resolved,
            super::smtp_provider_delivery::Error,
        >,
        H: FnMut(&super::storage::DueDelivery) -> Command,
        O: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        self.tick_selected_mixed_bound_smtp_deadlines(
            settings,
            maintenance,
            store,
            (mixed, None),
            hooks,
            select,
        )
    }

    // Event-specific private routing; selection applies only to newly emitted events.
    pub(super) fn tick_selected_mixed_bound_smtp_deadlines<'s, Q, R, H, O, C, X, S>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mixed: (MixedBoundSmtp<'_>, Option<&[(&str, Duration)]>),
        hooks: Hooks<Q, (R, H, O), C, X>,
        mut select: S,
    ) -> TickReport
    where
        S: FnMut(&super::lifecycle::Event) -> Vec<&'s str>,
        Q: FnMut(&mut Command, Duration, usize) -> Result<String, QueryError>,
        R: FnMut(
            &super::smtp_provider_revision::Descriptor,
        ) -> Result<
            super::smtp_provider_delivery::Resolved,
            super::smtp_provider_delivery::Error,
        >,
        H: FnMut(&super::storage::DueDelivery) -> Command,
        O: FnMut(
            &super::storage::DueDelivery,
        ) -> Result<
            super::delivery::ProviderOutcome,
            super::supervised_provider::Diagnostic,
        >,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        let (mixed, deadlines) = mixed;
        let bound = mixed.smtp;
        let ordinary = mixed.ordinary_destinations;
        let Hooks {
            mut query,
            send,
            clock,
            cancelled,
        } = hooks;
        self.tick_using_delivery(
            settings,
            maintenance,
            store,
            Hooks {
                query: (),
                send,
                clock,
                cancelled,
            },
            || {
                if settings.destinations.len() > 64
                    || bound.rendering.routes.len() > 64
                    || ordinary.len() > 64
                    || bound.rendering.routes.len().checked_add(ordinary.len())
                        != Some(settings.destinations.len())
                    || ordinary.iter().any(|name| {
                        name.is_empty()
                            || name.len() > 64
                            || !name
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                            || !settings.destinations.contains(name)
                            || bound
                                .rendering
                                .routes
                                .iter()
                                .any(|route| route.name == *name)
                    })
                    || ordinary
                        .iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        != ordinary.len()
                    || !valid_deadlines(bound.rendering.routes, bound.timeout, deadlines)
                {
                    return None;
                }
                let plan = super::smtp_routes::Plan::new(
                    settings.destinations,
                    bound.rendering.routes,
                    bound.rendering.max_messages,
                )
                .ok()?;
                super::smtp_routes::ProviderPlan::new(&plan, bound.profiles).ok()?;
                Some(plan)
            },
            |cycles, store, now, plan| {
                let provider = super::smtp_routes::ProviderPlan::new(plan, bound.profiles)
                    .map_err(|_| Error::Preparation)?;
                cycles.step_using(settings, store, false, now, |request, reconciler, store| {
                    provider
                        .poll_selected_with(
                            request,
                            reconciler,
                            store,
                            now,
                            &mut query,
                            &mut select,
                        )
                        .map_err(prepared_error)
                })
            },
            |store, now, (resolve, helper, ordinary_send), clock, cancelled| {
                super::smtp_provider_delivery::deliver_due_mixed(
                    store,
                    settings.cluster,
                    now,
                    maintenance.delivery_limit,
                    maintenance.retry,
                    super::smtp_provider_delivery::MixedHooks {
                        ordinary_destinations: ordinary,
                        ordinary_send,
                        smtp: super::smtp_provider_delivery::Hooks {
                            permissions: bound.permissions,
                            resolve,
                            send: |item: &super::storage::DueDelivery,
                                   _: &super::smtp_revision::Descriptor,
                                   message: &super::storage::FrozenMessage,
                                   resolved| {
                                let timeout = match deadlines {
                                    None => bound.timeout,
                                    Some(entries) => entries.iter().find(|(name, _)| *name == item.destination)
                                        .map(|(_, timeout)| *timeout)
                                        .ok_or(super::supervised_provider::Diagnostic::SmtpProviderResolution(
                                            super::smtp_provider_delivery::Error::Profile))?,
                                };
                                super::smtp_bound_transport::dispatch(
                                    &mut helper(item),
                                    resolved,
                                    message,
                                    timeout,
                                )
                            },
                            clock,
                            cancelled,
                        },
                    },
                )
            },
        )
    }

    fn tick_using<V, S, C, X>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        hooks: Hooks<(), S, C, X>,
        validate_poll: impl FnOnce() -> Option<V>,
        poll: impl FnMut(&mut Self, &mut Store, i64, &V) -> Result<Outcome, Error>,
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
        self.tick_using_delivery(
            settings,
            maintenance,
            store,
            hooks,
            validate_poll,
            poll,
            |store, now, send, clock, cancelled| {
                super::delivery::deliver_due_controlled(
                    store,
                    settings.cluster,
                    now,
                    maintenance.delivery_limit,
                    maintenance.retry,
                    super::delivery::DeliveryHooks {
                        send,
                        clock,
                        cancelled,
                    },
                )
            },
        )
    }

    #[allow(clippy::too_many_arguments)] // Shared stage engine keeps poll and delivery policies explicit.
    fn tick_using_delivery<V, S, C, X, D>(
        &mut self,
        settings: &Settings<'_>,
        maintenance: &Maintenance,
        store: &mut Store,
        mut hooks: Hooks<(), S, C, X>,
        validate_poll: impl FnOnce() -> Option<V>,
        mut poll: impl FnMut(&mut Self, &mut Store, i64, &V) -> Result<Outcome, Error>,
        mut deliver: D,
    ) -> TickReport
    where
        D: FnMut(
            &mut Store,
            i64,
            &mut S,
            &mut dyn FnMut() -> Option<i64>,
            &mut X,
        ) -> Result<super::delivery::BatchReport, super::delivery::BatchFailure>,
        C: FnMut() -> Option<i64>,
        X: FnMut() -> bool,
    {
        use super::delivery::FailureKind;
        let mut report = TickReport::default();
        if (hooks.cancelled)() {
            report.cancelled = true;
            return report;
        }
        if !valid_settings(settings, maintenance) {
            report.preflight = Some(Err(StageError::Policy));
            return report;
        }
        let Some(context) = validate_poll() else {
            report.preflight = Some(Err(StageError::Policy));
            return report;
        };
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
        let storage_health = match store.health_with_capacities(
            settings.cluster,
            now,
            maintenance.retain_seconds,
            (settings.capacity, settings.event_capacity),
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
        let (_, checkpoint) = match store.restore_with_capacities(
            settings.cluster,
            (settings.capacity, settings.event_capacity),
        ) {
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
        let poll = poll(self, store, now, &context);
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
        let delivery = deliver(
            store,
            delivery_now,
            &mut hooks.send,
            &mut clock,
            &mut hooks.cancelled,
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
            .restore_with_capacities(
                settings.cluster,
                (settings.capacity, settings.event_capacity),
            )
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
            .health_with_capacities(
                settings.cluster,
                now,
                retain_seconds,
                (settings.capacity, settings.event_capacity),
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
                .map_err(prepared_error)
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
            .restore_with_capacities(
                settings.cluster,
                (settings.capacity, settings.event_capacity),
            )
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
    #[test]
    fn deadline_map_requires_exact_destinations_and_bounded_milliseconds() {
        let routes = [render_route(crate::smtp_render::JobVisibility::Hidden)];
        for timeout in [
            Duration::ZERO,
            Duration::from_nanos(1),
            Duration::from_secs(31),
        ] {
            assert!(!super::valid_deadlines(&routes, timeout, None));
            assert!(!super::valid_deadlines(
                &routes,
                Duration::from_secs(1),
                Some(&[("primary", timeout)])
            ));
        }
        let good = Duration::from_millis(1);
        assert!(super::valid_deadlines(&routes, good, None));
        assert!(super::valid_deadlines(
            &routes,
            Duration::ZERO,
            Some(&[("primary", good)])
        ));
        for entries in [
            vec![],
            vec![("unknown", good)],
            vec![("primary", good), ("primary", good)],
        ] {
            assert!(!super::valid_deadlines(&routes, good, Some(&entries)));
        }
        assert!(!super::valid_deadlines(
            &routes,
            good,
            Some(&vec![("primary", good); 65])
        ));
        let mut second = render_route(crate::smtp_render::JobVisibility::Hidden);
        second.name = "secondary";
        let routes = [
            render_route(crate::smtp_render::JobVisibility::Hidden),
            second,
        ];
        assert!(!super::valid_deadlines(
            &routes,
            good,
            Some(&[("primary", good), ("primary", good)])
        ));
        assert!(super::valid_deadlines(
            &routes,
            good,
            Some(&[("secondary", Duration::from_secs(30)), ("primary", good)])
        ));
    }
    use super::*;
    use crate::storage::tests::Directory;

    #[test]
    #[cfg(unix)]
    fn bound_tick_preflight_cancellation_denial_and_retry_survive_restart() {
        use crate::{
            smtp_provider_delivery::{Permission, Permissions, Resolved},
            smtp_provider_revision::{self, Profile, TlsMode},
        };
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let trust = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "primary",
            relay: "127.0.0.1",
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let profiles = [profile];
        let routes = [render_route(crate::smtp_render::JobVisibility::Hidden)];
        let denied = Permissions::new(&[]).unwrap();
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        for (cancelled, timeout) in [(true, Duration::ZERO), (false, Duration::ZERO)] {
            let report = cycles.tick_bound_smtp(&settings(), &maintenance(), &mut store,
                BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout },
                Hooks { query: |_: &mut Command, _, _| panic!("preflight queried"),
                    send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("preflight resolved") },
                        |_: &crate::storage::DueDelivery| -> Command { panic!("preflight spawned") }),
                    clock: || panic!("preflight clock"), cancelled: || cancelled });
            assert_eq!(report.cancelled, cancelled);
            assert_eq!(
                report.preflight,
                if cancelled {
                    None
                } else {
                    Some(Err(StageError::Policy))
                }
            );
        }
        let mut samples = 0;
        let report = cycles.tick_bound_smtp(&settings(), &maintenance(), &mut store,
            BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout: Duration::from_secs(1) },
            Hooks { query: smtp_query,
                send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("cancelled resolution") },
                    |_: &crate::storage::DueDelivery| -> Command { panic!("cancelled spawn") }),
                clock: || Some(7200), cancelled: || { samples += 1; samples > 1 } });
        assert!(report.cancelled);
        assert!(matches!(
            report.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        drop(store);
        let mut store = open(&directory);
        let item = store.due_deliveries("cluster", 7200, 10).unwrap().remove(0);
        let frozen = store
            .frozen_smtp(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let rendering = store
            .bound_smtp_revision(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let provider = store
            .bound_smtp_provider(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "primary",
            rendering: rendering.token(),
            provider: provider.token(),
        }];
        let allowed = Permissions::new(&entries).unwrap();
        for (permissions, now, reply) in [
            (&denied, 7200, ""),
            (&allowed, 7200, "SMTP_SUBMIT FINAL TEMP NA"),
            (&allowed, 7205, "SMTP_SUBMIT FINAL ACCEPTED RA"),
        ] {
            let mut cycles = Cycles::new(1).unwrap();
            cycles.remaining = 0;
            let report = cycles.tick_bound_smtp(
                &settings(),
                &maintenance(),
                &mut store,
                BoundSmtp {
                    rendering: Rendering {
                        routes: &routes,
                        max_messages: 1,
                    },
                    profiles: &profiles,
                    permissions,
                    timeout: Duration::from_secs(1),
                },
                Hooks {
                    query: |_: &mut Command, _, _| panic!("exhausted query"),
                    send: (
                        |bound: &crate::smtp_provider_revision::Descriptor| {
                            Resolved::new(
                                bound,
                                &profiles[0],
                                &[&root],
                                "fixture-user",
                                "fixture-secret",
                            )
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(due.event_id, item.event_id);
                            assert!(!reply.is_empty());
                            let mut command = Command::new("/bin/sh");
                            command.args([
                                "-c",
                                "cat >/dev/null; printf '%s\n' \"$1\"",
                                "fixture",
                                reply,
                            ]);
                            command
                        },
                    ),
                    clock: || Some(now),
                    cancelled: || false,
                },
            );
            assert!(matches!(report.poll, Some(Ok(Outcome::Exhausted))));
            if reply.is_empty() {
                assert!(matches!(
                    report.delivery,
                    Some(Err(StageError::DispatchBlocked))
                ));
                assert!(report.cleanup.is_none());
            } else if now == 7200 {
                assert_eq!(report.delivery.unwrap().unwrap().retrying, 1);
            } else {
                assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
            }
            drop(store);
            store = open(&directory);
            assert_eq!(
                store
                    .frozen_smtp(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .data(),
                frozen.data()
            );
        }
        assert!(store
            .due_deliveries("cluster", 7300, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn mixed_tick_keeps_ordinary_progress_and_bound_retry_across_restart() {
        use crate::{
            smtp_provider_delivery::{Permission, Permissions, Resolved},
            smtp_provider_revision::{self, Profile, TlsMode},
        };
        let mut mixed_settings = settings();
        mixed_settings.destinations = &["primary", "aaa"];
        let ordinary_calls = std::cell::Cell::new(0);
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let trust = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "primary",
            relay: "127.0.0.1",
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let profiles = [profile];
        let routes = [render_route(crate::smtp_render::JobVisibility::Hidden)];
        let denied = Permissions::new(&[]).unwrap();
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        for ordinary in [
            &["primary"][..],
            &["aaa", "aaa"][..],
            &["missing"][..],
            &[][..],
        ] {
            let report = cycles.tick_mixed_bound_smtp(&mixed_settings, &maintenance(), &mut store,
                MixedBoundSmtp { ordinary_destinations: ordinary, smtp: BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout: Duration::from_secs(1) } },
                Hooks { query: |_: &mut Command, _, _| panic!("invalid mixed query"),
                    send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("invalid mixed resolve") },
                        |_: &crate::storage::DueDelivery| -> Command { panic!("invalid mixed spawn") },
                        |_: &crate::storage::DueDelivery| panic!("invalid mixed ordinary")),
                    clock: || panic!("invalid mixed clock"), cancelled: || false });
            assert_eq!(report.preflight, Some(Err(StageError::Policy)));
        }
        let mut samples = 0;
        let report = cycles.tick_mixed_bound_smtp(&mixed_settings, &maintenance(), &mut store,
            MixedBoundSmtp { ordinary_destinations: &["aaa"], smtp: BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout: Duration::from_secs(1) } },
            Hooks { query: smtp_query,
                send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("cancelled resolution") },
                    |_: &crate::storage::DueDelivery| -> Command { panic!("cancelled spawn") }, |_: &crate::storage::DueDelivery| panic!("cancelled ordinary")),
                clock: || Some(7200), cancelled: || { samples += 1; samples > 1 } });
        assert!(report.cancelled);
        assert!(matches!(
            report.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        drop(store);
        let mut store = open(&directory);
        let item = store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "primary")
            .unwrap();
        let frozen = store
            .frozen_smtp(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let rendering = store
            .bound_smtp_revision(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let provider = store
            .bound_smtp_provider(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "primary",
            rendering: rendering.token(),
            provider: provider.token(),
        }];
        let allowed = Permissions::new(&entries).unwrap();
        for (permissions, now, reply) in [
            (&denied, 7200, ""),
            (&allowed, 7200, "SMTP_SUBMIT FINAL TEMP NA"),
            (&allowed, 7205, "SMTP_SUBMIT FINAL ACCEPTED RA"),
        ] {
            let mut cycles = Cycles::new(1).unwrap();
            cycles.remaining = 0;
            let report = cycles.tick_mixed_bound_smtp(
                &mixed_settings,
                &maintenance(),
                &mut store,
                MixedBoundSmtp {
                    ordinary_destinations: &["aaa"],
                    smtp: BoundSmtp {
                        rendering: Rendering {
                            routes: &routes,
                            max_messages: 1,
                        },
                        profiles: &profiles,
                        permissions,
                        timeout: Duration::from_secs(1),
                    },
                },
                Hooks {
                    query: |_: &mut Command, _, _| panic!("exhausted query"),
                    send: (
                        |bound: &crate::smtp_provider_revision::Descriptor| {
                            Resolved::new(
                                bound,
                                &profiles[0],
                                &[&root],
                                "fixture-user",
                                "fixture-secret",
                            )
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(due.event_id, item.event_id);
                            assert!(!reply.is_empty());
                            let mut command = Command::new("/bin/sh");
                            command.args([
                                "-c",
                                "cat >/dev/null; printf '%s\n' \"$1\"",
                                "fixture",
                                reply,
                            ]);
                            command
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(due.destination, "aaa");
                            ordinary_calls.set(ordinary_calls.get() + 1);
                            Ok(crate::delivery::ProviderOutcome::Delivered)
                        },
                    ),
                    clock: || Some(now),
                    cancelled: || false,
                },
            );
            assert!(matches!(report.poll, Some(Ok(Outcome::Exhausted))));
            if reply.is_empty() {
                assert!(matches!(
                    report.delivery,
                    Some(Err(StageError::DispatchBlocked))
                ));
                assert!(report.cleanup.is_none());
                assert_eq!(report.delivery_failure.unwrap().committed.delivered, 1);
            } else if now == 7200 {
                assert_eq!(report.delivery.unwrap().unwrap().retrying, 1);
            } else {
                assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
            }
            drop(store);
            store = open(&directory);
            assert_eq!(
                store
                    .frozen_smtp(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .data(),
                frozen.data()
            );
        }
        assert!(store
            .due_deliveries("cluster", 7300, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn selected_mixed_tick_freezes_routes_and_recovers_after_reopen() {
        use crate::{
            smtp_provider_delivery::{Permission, Permissions, Resolved},
            smtp_provider_revision::{self, Profile, TlsMode},
        };
        let mut mixed_settings = settings();
        mixed_settings.destinations = &["primary", "aaa"];
        let ordinary_calls = std::cell::Cell::new(0);
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let trust = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "primary",
            relay: "127.0.0.1",
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let profiles = [profile];
        let routes = [render_route(crate::smtp_render::JobVisibility::Hidden)];
        let denied = Permissions::new(&[]).unwrap();
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let selection = crate::route_selection::Plan::new(
            mixed_settings.destinations,
            &[
                crate::route_selection::Rule {
                    events: &["JOB_COMPLETED"],
                    destinations: &["primary"],
                },
                crate::route_selection::Rule {
                    events: &["JOB_FAILED"],
                    destinations: &["aaa"],
                },
            ],
        )
        .unwrap();
        let mut samples = 0;
        let report = cycles.tick_selected_mixed_bound_smtp(&mixed_settings, &maintenance(), &mut store,
            MixedBoundSmtp { ordinary_destinations: &["aaa"], smtp: BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout: Duration::from_secs(1) } },
            Hooks { query: |command: &mut Command, _, _| Ok(if command.get_program() == "squeue" { "" } else {
                "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|TIMEOUT|1:0"
            }.into()),
                send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("cancelled resolution") },
                    |_: &crate::storage::DueDelivery| -> Command { panic!("cancelled spawn") }, |_: &crate::storage::DueDelivery| panic!("cancelled ordinary")),
                clock: || Some(7200), cancelled: || { samples += 1; samples > 1 } }, |event| selection.destinations(&event.state));
        assert!(report.cancelled);
        assert!(matches!(
            report.poll,
            Some(Ok(Outcome::Committed { events: 3, .. }))
        ));
        drop(store);
        let mut store = open(&directory);
        let due = store.due_deliveries("cluster", 7200, 10).unwrap();
        assert_eq!(due.len(), 2);
        assert!(due
            .iter()
            .any(|item| item.destination == "aaa" && item.event.identity.job == "2"));
        assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 3);
        let item = store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "primary")
            .unwrap();
        let frozen = store
            .frozen_smtp(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let rendering = store
            .bound_smtp_revision(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let provider = store
            .bound_smtp_provider(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "primary",
            rendering: rendering.token(),
            provider: provider.token(),
        }];
        let allowed = Permissions::new(&entries).unwrap();
        for (permissions, now, reply) in [
            (&denied, 7200, ""),
            (&allowed, 7200, "SMTP_SUBMIT FINAL TEMP NA"),
            (&allowed, 7205, "SMTP_SUBMIT FINAL ACCEPTED RA"),
        ] {
            let mut cycles = Cycles::new(1).unwrap();
            let report = cycles.tick_selected_mixed_bound_smtp(
                &mixed_settings,
                &maintenance(),
                &mut store,
                MixedBoundSmtp {
                    ordinary_destinations: &["aaa"],
                    smtp: BoundSmtp {
                        rendering: Rendering {
                            routes: &routes,
                            max_messages: 1,
                        },
                        profiles: &profiles,
                        permissions,
                        timeout: Duration::from_secs(1),
                    },
                },
                Hooks {
                    query: |command: &mut Command, _, _| Ok(if command.get_program() == "squeue" { "" } else {
                        "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|FAILED|1:0\ncluster|3|alice|s|actual|TIMEOUT|1:0"
                    }.into()),
                    send: (
                        |bound: &crate::smtp_provider_revision::Descriptor| {
                            Resolved::new(
                                bound,
                                &profiles[0],
                                &[&root],
                                "fixture-user",
                                "fixture-secret",
                            )
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(due.event_id, item.event_id);
                            assert!(!reply.is_empty());
                            let mut command = Command::new("/bin/sh");
                            command.args([
                                "-c",
                                "cat >/dev/null; printf '%s\n' \"$1\"",
                                "fixture",
                                reply,
                            ]);
                            command
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(due.destination, "aaa");
                            assert_eq!(due.event.identity.job, "2");
                            ordinary_calls.set(ordinary_calls.get() + 1);
                            Ok(crate::delivery::ProviderOutcome::Delivered)
                        },
                    ),
                    clock: || Some(now),
                    cancelled: || false,
                },
                // An edited selector cannot retarget or backfill persisted events.
                |_: &crate::lifecycle::Event| vec!["aaa"],
            );
            assert!(matches!(
                report.poll,
                Some(Ok(Outcome::Committed { events: 0, .. }))
            ));
            if reply.is_empty() {
                assert!(matches!(
                    report.delivery,
                    Some(Err(StageError::DispatchBlocked))
                ));
                assert!(report.cleanup.is_none());
                assert_eq!(report.delivery_failure.unwrap().committed.delivered, 0);
            } else if now == 7200 {
                let delivered = report.delivery.unwrap().unwrap();
                assert_eq!((delivered.delivered, delivered.retrying), (1, 1));
            } else {
                assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
            }
            drop(store);
            store = open(&directory);
            assert_eq!(
                store
                    .bound_smtp_revision(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .token(),
                rendering.token()
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .token(),
                provider.token()
            );
            let pending = store.due_deliveries("cluster", 7300, 10).unwrap();
            if !reply.is_empty() && now == 7200 {
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].attempt_number, 2);
                assert_eq!(pending[0].event_id, item.event_id);
            }
            assert_eq!(
                store
                    .frozen_smtp(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .data(),
                frozen.data()
            );
        }
        assert_eq!(ordinary_calls.get(), 1);
        assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 3);
        assert!(store
            .due_deliveries("cluster", 7300, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn mixed_mid_batch_cancel_and_resolution_blocks_preserve_exact_pending_work() {
        use crate::{
            smtp_provider_delivery::{Permission, Permissions, Resolved},
            smtp_provider_revision::{self, Profile, TlsMode},
        };
        let mut mixed_settings = settings();
        mixed_settings.destinations = &["primary", "aaa"];
        let ordinary_calls = std::cell::Cell::new(0);
        let root = rcgen::generate_simple_self_signed(vec!["smtp.fixture.invalid".into()])
            .unwrap()
            .cert
            .der()
            .to_vec();
        let trust = smtp_provider_revision::trust_fingerprint(&[&root]).unwrap();
        let profile = Profile {
            destination: "primary",
            relay: "127.0.0.1",
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &trust,
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        };
        let profiles = [profile];
        let routes = [render_route(crate::smtp_render::JobVisibility::Hidden)];
        let denied = Permissions::new(&[]).unwrap();
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let mut samples = 0;
        let report = cycles.tick_mixed_bound_smtp(&mixed_settings, &maintenance(), &mut store,
            MixedBoundSmtp { ordinary_destinations: &["aaa"], smtp: BoundSmtp { rendering: Rendering { routes: &routes, max_messages: 1 }, profiles: &profiles, permissions: &denied, timeout: Duration::from_secs(1) } },
            Hooks { query: smtp_query,
                send: (|_: &crate::smtp_provider_revision::Descriptor| -> Result<Resolved, crate::smtp_provider_delivery::Error> { panic!("cancelled resolution") },
                    |_: &crate::storage::DueDelivery| -> Command { panic!("cancelled spawn") }, |_: &crate::storage::DueDelivery| panic!("cancelled ordinary")),
                clock: || Some(7200), cancelled: || { samples += 1; samples > 1 } });
        assert!(report.cancelled);
        assert!(matches!(
            report.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        drop(store);
        let mut store = open(&directory);
        let item = store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "primary")
            .unwrap();
        let frozen = store
            .frozen_smtp(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let rendering = store
            .bound_smtp_revision(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let provider = store
            .bound_smtp_provider(&item.event_id, "primary")
            .unwrap()
            .unwrap();
        let entries = [Permission {
            cluster: "cluster",
            destination: "primary",
            rendering: rendering.token(),
            provider: provider.token(),
        }];
        let allowed = Permissions::new(&entries).unwrap();
        let cancel = std::cell::Cell::new(false);
        let resolve_calls = std::cell::Cell::new(0);
        let helper_calls = std::cell::Cell::new(0);
        let clock_calls = std::cell::Cell::new(0);
        for mode in 0..=5 {
            cancel.set(false);
            clock_calls.set(0);
            let mut cycles = Cycles::new(1).unwrap();
            cycles.remaining = 0;
            let report = cycles.tick_mixed_bound_smtp(
                &mixed_settings,
                &maintenance(),
                &mut store,
                MixedBoundSmtp {
                    ordinary_destinations: &["aaa"],
                    smtp: BoundSmtp {
                        rendering: Rendering {
                            routes: &routes,
                            max_messages: 1,
                        },
                        profiles: &profiles,
                        permissions: &allowed,
                        timeout: Duration::from_secs(1),
                    },
                },
                Hooks {
                    query: |_: &mut Command, _, _| panic!("exhausted query"),
                    send: (
                        |bound: &crate::smtp_provider_revision::Descriptor| {
                            resolve_calls.set(resolve_calls.get() + 1);
                            match mode {
                                1 => Err(crate::smtp_provider_delivery::Error::Trust),
                                2 => Err(crate::smtp_provider_delivery::Error::Profile),
                                3 => Err(crate::smtp_provider_delivery::Error::Credentials),
                                4 => {
                                    let changed_profile = Profile {
                                        port: 588,
                                        ..profiles[0]
                                    };
                                    let changed =
                                        smtp_provider_revision::Descriptor::new(&changed_profile)
                                            .unwrap();
                                    Resolved::new(
                                        &changed,
                                        &changed_profile,
                                        &[&root],
                                        "fixture-user",
                                        "fixture-secret",
                                    )
                                }
                                5 => Resolved::new(
                                    bound,
                                    &profiles[0],
                                    &[&root],
                                    "fixture-user",
                                    "fixture-secret",
                                ),
                                _ => panic!("cancelled before resolution"),
                            }
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(mode, 5);
                            assert_eq!(due.attempt_number, 1);
                            helper_calls.set(helper_calls.get() + 1);
                            let mut command = Command::new("/bin/sh");
                            command.args([
                                "-c",
                                "cat >/dev/null; printf 'SMTP_SUBMIT FINAL ACCEPTED RA\n'",
                            ]);
                            command
                        },
                        |due: &crate::storage::DueDelivery| {
                            assert_eq!(mode, 0);
                            assert_eq!(due.destination, "aaa");
                            ordinary_calls.set(ordinary_calls.get() + 1);
                            cancel.set(true);
                            Ok(crate::delivery::ProviderOutcome::Delivered)
                        },
                    ),
                    clock: || {
                        clock_calls.set(clock_calls.get() + 1);
                        Some(7200)
                    },
                    cancelled: || cancel.get(),
                },
            );
            assert!(matches!(report.poll, Some(Ok(Outcome::Exhausted))));
            if mode == 0 {
                assert!(report.cancelled);
                assert!(matches!(report.delivery, Some(Err(StageError::Cancelled))));
                assert_eq!(report.delivery_failure.unwrap().committed.delivered, 1);
                assert_eq!(clock_calls.get(), 3);
                assert_eq!(resolve_calls.get(), 0);
            } else if mode < 5 {
                assert!(!report.cancelled);
                assert!(matches!(
                    report.delivery,
                    Some(Err(StageError::DispatchBlocked))
                ));
                let failure = report.delivery_failure.unwrap();
                let expected = match mode {
                    1 => crate::smtp_provider_delivery::Error::Trust,
                    3 => crate::smtp_provider_delivery::Error::Credentials,
                    _ => crate::smtp_provider_delivery::Error::Profile,
                };
                assert_eq!(
                    failure.diagnostic,
                    Some(crate::supervised_provider::Diagnostic::SmtpProviderResolution(expected))
                );
                assert_eq!(failure.committed.delivered, 0);
                assert!(!failure.acceptance_uncertain);
                assert_eq!(clock_calls.get(), 2);
            } else {
                assert_eq!(report.delivery.unwrap().unwrap().delivered, 1);
                assert!(report.cleanup.is_some() && report.health.is_some());
            }
            if mode < 5 {
                assert!(report.cleanup.is_none() && report.health.is_none());
            }
            drop(store);
            store = open(&directory);
            let due = store.due_deliveries("cluster", 7200, 10).unwrap();
            if mode < 5 {
                assert_eq!(due.len(), 1);
                assert_eq!(due[0].destination, "primary");
                assert_eq!(due[0].attempt_number, 1);
            } else {
                assert!(due.is_empty());
            }
            assert_eq!(
                store
                    .frozen_smtp(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .data(),
                frozen.data()
            );
            assert_eq!(
                store
                    .bound_smtp_provider(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .token(),
                provider.token()
            );
            assert_eq!(
                store
                    .bound_smtp_revision(&item.event_id, "primary")
                    .unwrap()
                    .unwrap()
                    .token(),
                rendering.token()
            );
        }
        assert_eq!(ordinary_calls.get(), 1);
        assert_eq!(resolve_calls.get(), 5);
        assert_eq!(helper_calls.get(), 1);
    }

    fn smtp_message(
        event: &super::super::lifecycle::Event,
        _: &str,
    ) -> Result<super::super::smtp_message::Prepared, ()> {
        super::super::smtp_render::prepare(
            event,
            &super::super::smtp_render::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                identity: "fixture",
                timestamp: 123,
            },
            super::super::smtp_render::JobVisibility::Hidden,
        )
        .map_err(|_| ())
    }

    #[test]
    fn renderer_failure_rolls_back_poll_and_reopen_can_prepare_redacted_snapshot() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        let result = cycles.step_prepared(
            &settings(),
            &mut store,
            false,
            7200,
            PreparedHooks {
                smtp_destinations: &["primary"],
                max_messages: 1,
                query: smtp_query,
                prepare: |event: &super::super::lifecycle::Event, _: &str| {
                    super::super::smtp_render::prepare(
                        event,
                        &super::super::smtp_render::Metadata {
                            sender: "s@fixture.invalid",
                            recipient: "r@fixture.invalid",
                            identity: "invalid\r\nfixture",
                            timestamp: 123,
                        },
                        super::super::smtp_render::JobVisibility::Hidden,
                    )
                    .map_err(|_| ())
                },
            },
        );
        assert!(matches!(result, Err(Error::Preparation)));
        assert_eq!(cycles.health.last_failure, Some(Failure::Preparation));
        drop(store);
        let mut store = open(&directory);
        let restored = store.restore("cluster", 10).unwrap();
        assert!(restored.0.emitted.is_empty());
        assert!(restored.1.is_none());
        assert!(store
            .due_deliveries("cluster", 7200, 10)
            .unwrap()
            .is_empty());
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
        let item = store.due_deliveries("cluster", 7260, 10).unwrap().remove(0);
        let frozen = store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .unwrap();
        let text = std::str::from_utf8(frozen.data()).unwrap();
        assert!(text.contains("State: COMPLETED") && text.contains("Job: [redacted]"));
        assert!(!text.contains("cluster") && !text.contains("actual"));
        assert_eq!(cycles.health.last_failure, None);
        drop(store);
        let store = open(&directory);
        let reopened = store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .unwrap();
        assert!(reopened.data() == frozen.data());
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
            event_capacity: 10,
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

    fn render_route(
        visibility: super::super::smtp_render::JobVisibility,
    ) -> super::super::smtp_routes::Route<'static> {
        super::super::smtp_routes::Route {
            name: "primary",
            visibility,
            metadata: super::super::smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "primary",
            },
        }
    }

    #[test]
    fn rendered_tick_cancels_before_validation_and_rejects_policy_even_when_exhausted() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(1).unwrap();
        let mut routes = [render_route(
            super::super::smtp_render::JobVisibility::Hidden,
        )];
        routes[0].metadata.recipient = "private-invalid-fixture";
        for (cancelled, remaining) in [(true, 1), (false, 1), (false, 0)] {
            cycles.remaining = remaining;
            let report = cycles.tick_rendered_fallible(
                &settings(),
                &maintenance(),
                &mut store,
                Rendering {
                    routes: &routes,
                    max_messages: 1,
                },
                Hooks {
                    query: |_: &mut Command, _, _| panic!("invalid policy queried scheduler"),
                    send: |_: &super::super::storage::DueDelivery| {
                        panic!("invalid policy delivered")
                    },
                    clock: || panic!("invalid policy sampled clock"),
                    cancelled: || cancelled,
                },
            );
            assert_eq!(report.cancelled, cancelled);
            assert_eq!(
                report.preflight,
                if cancelled {
                    None
                } else {
                    Some(Err(StageError::Policy))
                }
            );
            assert!(report.poll.is_none() && report.cleanup.is_none() && report.health.is_none());
            assert_eq!(cycles.remaining, remaining);
            assert_eq!(cycles.health.consecutive_failures, 0);
        }
        assert!(store.restore("cluster", 10).unwrap().1.is_none());
    }

    #[test]
    fn rendered_tick_restart_failure_keeps_frozen_disclosure_and_delivers_prior_work() {
        use super::super::smtp_render::JobVisibility;
        let directory = Directory::new();
        let mut store = open(&directory);
        let mut cycles = Cycles::new(2).unwrap();
        let hidden = [render_route(JobVisibility::Hidden)];
        let mut cancellations = 0;
        let first = cycles.tick_rendered_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            Rendering {
                routes: &hidden,
                max_messages: 1,
            },
            Hooks {
                query: smtp_query,
                send: |_: &super::super::storage::DueDelivery| panic!("cancelled before send"),
                clock: || Some(7200),
                cancelled: || {
                    cancellations += 1;
                    cancellations > 1
                },
            },
        );
        assert!(first.cancelled);
        assert!(matches!(
            first.poll,
            Some(Ok(Outcome::Committed { events: 1, .. }))
        ));
        let item = store.due_deliveries("cluster", 7200, 10).unwrap().remove(0);
        let snapshot = store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        let disclosed = [render_route(JobVisibility::Identifier)];
        let oversized = "1".repeat(33);
        let mut sent = 0;
        let second = cycles.tick_rendered_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            Rendering {
                routes: &disclosed,
                max_messages: 1,
            },
            Hooks {
                query: |command: &mut Command, _, _| {
                    Ok(if command.get_program() == "squeue" {
                        String::new()
                    } else {
                        format!("cluster|{oversized}|alice|s|actual|COMPLETED|0:0")
                    })
                },
                send: |due: &super::super::storage::DueDelivery| {
                    sent += 1;
                    assert_eq!(due.event_id, item.event_id);
                    let reader = open(&directory);
                    let kept = reader
                        .frozen_smtp(&due.event_id, &due.destination)
                        .unwrap()
                        .unwrap();
                    assert!(kept.data() == snapshot.data());
                    assert!(std::str::from_utf8(kept.data())
                        .unwrap()
                        .contains("Job: [redacted]"));
                    Ok(super::super::delivery::ProviderOutcome::Delivered)
                },
                clock: || Some(7260),
                cancelled: || false,
            },
        );
        assert_eq!(sent, 1);
        assert_eq!(second.poll.unwrap().unwrap_err(), StageError::Poll);
        assert_eq!(second.delivery.unwrap().unwrap().delivered, 1);
        let health = second.health.unwrap().unwrap();
        assert_eq!(health.poll.last_failure, Some(Failure::Preparation));
        assert_eq!(health.poll.checkpoint_age_seconds, Some(60));
        assert_eq!(cycles.remaining, 0);
        drop(store);
        let mut store = open(&directory);
        assert_eq!(store.restore("cluster", 10).unwrap().0.emitted.len(), 1);
        let exhausted = cycles.tick_rendered_fallible(
            &settings(),
            &maintenance(),
            &mut store,
            Rendering {
                routes: &disclosed,
                max_messages: 1,
            },
            Hooks {
                query: |_: &mut Command, _, _| panic!("exhausted query"),
                send: |_: &super::super::storage::DueDelivery| panic!("duplicate send"),
                clock: || Some(7261),
                cancelled: || false,
            },
        );
        assert!(matches!(exhausted.poll, Some(Ok(Outcome::Exhausted))));
        assert_eq!(
            exhausted.delivery.unwrap().unwrap(),
            super::super::delivery::BatchReport::default()
        );
        assert!(exhausted.health.unwrap().is_ok());
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
        smaller.event_capacity = 1;
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
        smaller.event_capacity = 2;
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
        smaller.event_capacity = 1;
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
        for case in 0..29 {
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
                26 => maintenance.stale_after = 0,
                27 => settings.event_capacity = 0,
                _ => settings.event_capacity = usize::MAX,
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
