//! Private protected startup-to-shutdown orchestration; no installed entry point.
use crate::{
    foreground_report::{Report, StartupReport},
    foreground_session::{Error, Session},
    foreground_wait::Signal,
    polling_cycle::TickReport,
};
use std::{path::Path, time::Duration};
use zeroize::Zeroizing;
pub(super) struct Request<'a> {
    pub(super) path: &'a Path,
    pub(super) poll_limit: usize,
    pub(super) tick_limit: usize,
    pub(super) lock_timeout: Duration,
}
pub(super) struct ContinuousRequest<'a> {
    pub(super) path: &'a Path,
    pub(super) poll_batch: usize,
    pub(super) lock_timeout: Duration,
}
pub(super) struct Hooks<I, L, T> {
    pub(super) identity: I,
    pub(super) lookup: L,
    pub(super) tick: T,
}
pub(super) enum Summary {
    Startup(StartupReport),
    Shutdown(Report),
}
impl Summary {
    // Read the retained reason while the signal binding is still owned. A late
    // signal cannot turn a completed failure into a successful cancellation.
    pub(super) fn exit_code(&self, reason: Option<crate::foreground_signals::Reason>) -> u8 {
        let (cancelled, code) = match self {
            Self::Startup(report) => (report.cancelled(), report.exit_code()),
            Self::Shutdown(report) => (report.cancelled(), report.exit_code()),
        };
        if cancelled {
            match reason {
                Some(crate::foreground_signals::Reason::Interrupt) => 130,
                Some(crate::foreground_signals::Reason::Terminate) => 143,
                None => code,
            }
        } else {
            code
        }
    }
    pub(super) fn render(&self) -> String {
        match self {
            Self::Startup(report) => report.render(),
            Self::Shutdown(report) => report.render(),
        }
    }
}
pub(super) fn run<I, L, T>(request: Request<'_>, signal: &Signal, hooks: Hooks<I, L, T>) -> Summary
where
    I: FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    T: FnMut(&mut Session, &Signal) -> Result<TickReport, Error>,
{
    if signal.cancelled().unwrap_or(true) {
        return Summary::Startup(StartupReport::classify(Error::Cancelled));
    }
    // Validate the scheduling budget before configuration/resource/state access.
    if !(1..=1024).contains(&request.tick_limit) {
        return Summary::Startup(StartupReport::classify(Error::InvalidLimit));
    }
    let Hooks {
        identity,
        lookup,
        mut tick,
    } = hooks;
    let mut session = match Session::start_reported(
        request.path,
        request.poll_limit,
        request.lock_timeout,
        identity,
        lookup,
        || signal.cancelled().unwrap_or(true),
    ) {
        Ok(session) => session,
        Err(report) => return Summary::Startup(report),
    };
    let result =
        session.run_interruptible(request.tick_limit, signal, |session| tick(session, signal));
    Summary::Shutdown(Report::classify(&result))
}

// Uses the same startup order, owned session, OS monotonic clock and interruptible
// wait as finite orchestration. Provider helper/identity/resource adapters remain explicit.
pub(super) fn run_continuous<I, L, T>(
    request: ContinuousRequest<'_>,
    signal: &Signal,
    hooks: Hooks<I, L, T>,
) -> Summary
where
    I: FnOnce(usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    L: FnMut(&str, usize) -> Result<Zeroizing<Vec<u8>>, ()>,
    T: FnMut(&mut Session, &Signal) -> Result<TickReport, Error>,
{
    if signal.cancelled().unwrap_or(true) {
        return Summary::Startup(StartupReport::classify(Error::Cancelled));
    }
    if !(1..=1024).contains(&request.poll_batch) {
        return Summary::Startup(StartupReport::classify(Error::InvalidLimit));
    }
    let Hooks {
        identity,
        lookup,
        mut tick,
    } = hooks;
    let mut session = match Session::start_reported(
        request.path,
        request.poll_batch,
        request.lock_timeout,
        identity,
        lookup,
        || signal.cancelled().unwrap_or(true),
    ) {
        Ok(session) => session,
        Err(report) => return Summary::Startup(report),
    };
    let result = session
        .run_continuous_interruptible(request.poll_batch, signal, |session| tick(session, signal));
    Summary::Shutdown(Report::classify_continuous(&result))
}

// The selected executable is retained for the complete owned run; no per-send
// executable discovery or caller-provided helper-path override is used here.
pub(super) fn run_selected_local_continuous(
    request: ContinuousRequest<'_>,
    signal: &Signal,
    programs: (&Path, &Path),
) -> Summary {
    if signal.cancelled().unwrap_or(true) {
        return Summary::Startup(StartupReport::classify(Error::Cancelled));
    }
    if !(1..=1024).contains(&request.poll_batch) {
        return Summary::Startup(StartupReport::classify(Error::InvalidLimit));
    }
    let helpers = match crate::provider_helpers::Selection::current() {
        Ok(helpers) => helpers,
        Err(_) => return Summary::Startup(StartupReport::classify(Error::Preparation)),
    };
    run_local_continuous(
        request,
        signal,
        programs,
        (
            |_: &crate::storage::DueDelivery| helpers.command(crate::provider_helpers::Kind::Smtp),
            |_: &crate::storage::DueDelivery| {
                helpers.command(crate::provider_helpers::Kind::Webhook)
            },
        ),
    )
}

// Explicit scheduler programs and caller-owned trusted helper commands. This binds
// existing OS adapters without adding installed flags, PATH search or helper discovery.
pub(super) fn run_local_continuous<S, W>(
    request: ContinuousRequest<'_>,
    signal: &Signal,
    programs: (&Path, &Path),
    helpers: (S, W),
) -> Summary
where
    S: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
    W: FnMut(&crate::storage::DueDelivery) -> std::process::Command,
{
    if signal.cancelled().unwrap_or(true) {
        return Summary::Startup(StartupReport::classify(Error::Cancelled));
    }
    if !(1..=1024).contains(&request.poll_batch) {
        return Summary::Startup(StartupReport::classify(Error::InvalidLimit));
    }
    if !programs.0.is_absolute() || !programs.1.is_absolute() {
        return Summary::Startup(StartupReport::classify(Error::Preparation));
    }
    let (mut smtp_helper, mut webhook_helper) = helpers;
    run_continuous(
        request,
        signal,
        Hooks {
            identity: crate::effective_user::lookup,
            lookup: crate::environment_resolution::lookup,
            tick: |session: &mut Session, signal: &Signal| {
                session.tick_public_with(
                    programs,
                    crate::polling_cycle::Hooks {
                        query: crate::scheduler::run,
                        send: (
                            crate::environment_resolution::lookup,
                            &mut smtp_helper,
                            &mut webhook_helper,
                        ),
                        clock: crate::delivery::unix_time,
                        cancelled: || signal.cancelled().unwrap_or(true),
                    },
                )
            },
        },
    )
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit test bodies.
    use super::*;
    #[test]
    fn exit_mapping_uses_retained_signals_only_for_cancellation() {
        use crate::{
            foreground_schedule::{Error as ScheduleError, Exit},
            foreground_signals::Reason,
        };
        for (signal, expected) in [
            (None, 1),
            (Some(Reason::Interrupt), 130),
            (Some(Reason::Terminate), 143),
        ] {
            let report = Summary::Shutdown(Report::classify_continuous(&Ok(Exit::Cancelled {
                ticks: 1025,
            })));
            assert_eq!(report.exit_code(signal), expected);
        }
        for (error, expected) in [
            (Error::Cancelled, 1),
            (Error::InvalidLimit, 2),
            (Error::Preparation, 2),
            (Error::Ownership, 1),
            (Error::Cycle, 1),
        ] {
            let report = Summary::Startup(StartupReport::classify(error));
            assert_eq!(report.exit_code(None), expected);
        }
        assert_eq!(
            Summary::Startup(StartupReport::classify(Error::Cancelled))
                .exit_code(Some(Reason::Interrupt)),
            130
        );
        assert_eq!(
            Summary::Startup(StartupReport::classify(Error::Preparation))
                .exit_code(Some(Reason::Terminate)),
            2
        );
        assert_eq!(
            Summary::Shutdown(Report::classify(&Err(ScheduleError::Wait)))
                .exit_code(Some(Reason::Interrupt)),
            1
        );
        assert_eq!(
            Summary::Shutdown(Report::classify(&Ok(Exit::Exhausted { ticks: 1 })))
                .exit_code(Some(Reason::Terminate)),
            0
        );
    }
    #[test]
    fn local_adapter_requires_absolute_programs_and_preserves_startup_precedence() {
        for (cancel, batch, active, accounting, expected) in [
            (true, 0, "relative", "relative", "cancelled"),
            (false, 0, "relative", "relative", "failed_limit"),
            (false, 1, "relative", "/fixture/sacct", "failed_preparation"),
            (
                false,
                1,
                "/fixture/squeue",
                "relative",
                "failed_preparation",
            ),
            (
                false,
                1,
                "/fixture/squeue",
                "/fixture/sacct",
                "failed_preparation",
            ),
        ] {
            let signal = Signal::new();
            if cancel {
                signal.cancel().unwrap();
            }
            let summary = run_local_continuous(
                ContinuousRequest {
                    path: Path::new("/nonexistent-private-config"),
                    poll_batch: batch,
                    lock_timeout: Duration::from_millis(50),
                },
                &signal,
                (Path::new(active), Path::new(accounting)),
                (
                    |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("preflight SMTP")
                    },
                    |_: &crate::storage::DueDelivery| -> std::process::Command {
                        panic!("preflight webhook")
                    },
                ),
            );
            assert_eq!(summary.render(), format!("foreground startup={expected}\n"));
            let selected = run_selected_local_continuous(
                ContinuousRequest {
                    path: Path::new("/nonexistent-private-config"),
                    poll_batch: batch,
                    lock_timeout: Duration::from_millis(50),
                },
                &signal,
                (Path::new(active), Path::new(accounting)),
            );
            assert_eq!(
                selected.render(),
                format!("foreground startup={expected}\n")
            );
        }
    }
    #[test]
    fn continuous_preflight_cancels_and_rejects_budget_before_configuration_or_resources() {
        for (cancel, batch) in [(true, 1), (false, 0), (false, 1025), (false, 1)] {
            let signal = Signal::new();
            if cancel {
                signal.cancel().unwrap();
            }
            let summary = run_continuous(
                ContinuousRequest {
                    path: Path::new("/nonexistent-private-config"),
                    poll_batch: batch,
                    lock_timeout: Duration::from_millis(50),
                },
                &signal,
                Hooks {
                    identity: |_| panic!("preflight identity"),
                    lookup: |_: &str, _| panic!("preflight resources"),
                    tick: |_: &mut Session, _: &Signal| panic!("preflight tick"),
                },
            );
            let expected = if cancel {
                "cancelled"
            } else if batch != 1 {
                "failed_limit"
            } else {
                "failed_preparation"
            };
            assert_eq!(summary.render(), format!("foreground startup={expected}\n"));
        }
    }
}
