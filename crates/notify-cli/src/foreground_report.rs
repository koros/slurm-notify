//! Private bounded shutdown summaries; no installed output or exit-code contract.
use crate::foreground_schedule::{Error, Exit};
pub(super) const MAX_BYTES: usize = 96;
#[derive(Debug, PartialEq, Eq)]
enum Reason {
    Policy,
    Clock,
    ClockRegressed,
    DeadlineOverflow,
    Wait,
    NoProgress,
    WakeLimit,
    Tick,
    InvalidCount,
}
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Cancelled,
    Exhausted,
    Failed(Reason),
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    outcome: Outcome,
    ticks: Option<u16>,
}
impl Report {
    pub(super) fn classify(result: &Result<Exit, Error>) -> Self {
        match result {
            Ok(Exit::Cancelled { ticks }) if *ticks <= 1024 => Self {
                outcome: Outcome::Cancelled,
                ticks: Some(*ticks as u16),
            },
            Ok(Exit::Exhausted { ticks }) if (1..=1024).contains(ticks) => Self {
                outcome: Outcome::Exhausted,
                ticks: Some(*ticks as u16),
            },
            Ok(_) => Self {
                outcome: Outcome::Failed(Reason::InvalidCount),
                ticks: None,
            },
            Err(error) => Self {
                outcome: Outcome::Failed(match error {
                    Error::Policy => Reason::Policy,
                    Error::Clock => Reason::Clock,
                    Error::ClockRegressed => Reason::ClockRegressed,
                    Error::DeadlineOverflow => Reason::DeadlineOverflow,
                    Error::Wait => Reason::Wait,
                    Error::NoProgress => Reason::NoProgress,
                    Error::WakeLimit => Reason::WakeLimit,
                    Error::Tick => Reason::Tick,
                }),
                ticks: None,
            },
        }
    }
    pub(super) fn render(&self) -> String {
        let token = match &self.outcome {
            Outcome::Cancelled => "cancelled",
            Outcome::Exhausted => "exhausted",
            Outcome::Failed(reason) => match reason {
                Reason::Policy => "failed_policy",
                Reason::Clock => "failed_clock",
                Reason::ClockRegressed => "failed_clock_regressed",
                Reason::DeadlineOverflow => "failed_deadline_overflow",
                Reason::Wait => "failed_wait",
                Reason::NoProgress => "failed_no_progress",
                Reason::WakeLimit => "failed_wake_limit",
                Reason::Tick => "failed_tick",
                Reason::InvalidCount => "failed_invalid_count",
            },
        };
        let ticks = self
            .ticks
            .map(|ticks| ticks.to_string())
            .unwrap_or_else(|| "unknown".into());
        let rendered = format!("foreground outcome={token} ticks={ticks}\n");
        debug_assert!(rendered.len() <= MAX_BYTES);
        rendered
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_exhaustion_retain_only_valid_completed_call_counts() {
        for ticks in [0, 1, 1024] {
            assert_eq!(
                Report::classify(&Ok(Exit::Cancelled { ticks })).render(),
                format!("foreground outcome=cancelled ticks={ticks}\n")
            );
        }
        for ticks in [1, 1024] {
            assert_eq!(
                Report::classify(&Ok(Exit::Exhausted { ticks })).render(),
                format!("foreground outcome=exhausted ticks={ticks}\n")
            );
        }
        for exit in [
            Exit::Exhausted { ticks: 0 },
            Exit::Exhausted { ticks: 1025 },
            Exit::Cancelled { ticks: usize::MAX },
        ] {
            assert_eq!(
                Report::classify(&Ok(exit)).render(),
                "foreground outcome=failed_invalid_count ticks=unknown\n"
            );
        }
    }
    #[test]
    fn failures_have_static_reasons_and_unknown_counts_with_bounded_ascii_output() {
        for (error, reason) in [
            (Error::Policy, "policy"),
            (Error::Clock, "clock"),
            (Error::ClockRegressed, "clock_regressed"),
            (Error::DeadlineOverflow, "deadline_overflow"),
            (Error::Wait, "wait"),
            (Error::NoProgress, "no_progress"),
            (Error::WakeLimit, "wake_limit"),
            (Error::Tick, "tick"),
        ] {
            let report = Report::classify(&Err(error));
            let output = report.render();
            assert_eq!(
                output,
                format!("foreground outcome=failed_{reason} ticks=unknown\n")
            );
            assert!(output.is_ascii() && output.len() <= MAX_BYTES);
            assert_eq!(output.bytes().filter(|byte| *byte == b'\n').count(), 1);
            assert!(!format!("{report:?}").contains("research"));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
enum StartupOutcome {
    Cancelled,
    Limit,
    Preparation,
    Ownership,
    InvalidPhase,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
pub(super) struct StartupReport {
    outcome: StartupOutcome,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl StartupReport {
    pub(super) fn classify(error: crate::foreground_session::Error) -> Self {
        use crate::foreground_session::Error;
        Self {
            outcome: match error {
                Error::Cancelled => StartupOutcome::Cancelled,
                Error::InvalidLimit => StartupOutcome::Limit,
                Error::Preparation => StartupOutcome::Preparation,
                Error::Ownership => StartupOutcome::Ownership,
                Error::Closed | Error::Cycle | Error::Status => StartupOutcome::InvalidPhase,
            },
        }
    }
    pub(super) fn render(&self) -> String {
        let token = match self.outcome {
            StartupOutcome::Cancelled => "cancelled",
            StartupOutcome::Limit => "failed_limit",
            StartupOutcome::Preparation => "failed_preparation",
            StartupOutcome::Ownership => "failed_ownership",
            StartupOutcome::InvalidPhase => "failed_invalid_phase",
        };
        format!("foreground startup={token}\n")
    }
}
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod startup_tests {
    use super::*;
    use crate::foreground_session::Error;
    #[test]
    fn startup_failure_classes_are_bounded_static_and_phase_specific() {
        for (error, token) in [
            (Error::Cancelled, "cancelled"),
            (Error::InvalidLimit, "failed_limit"),
            (Error::Preparation, "failed_preparation"),
            (Error::Ownership, "failed_ownership"),
            (Error::Closed, "failed_invalid_phase"),
            (Error::Cycle, "failed_invalid_phase"),
            (Error::Status, "failed_invalid_phase"),
        ] {
            let report = StartupReport::classify(error);
            let output = report.render();
            assert_eq!(output, format!("foreground startup={token}\n"));
            assert!(output.is_ascii() && output.len() <= MAX_BYTES);
            assert_eq!(output.bytes().filter(|byte| *byte == b'\n').count(), 1);
        }
    }
}
