//! Installed prototype foreground command. No service installation or implicit config.
use std::path::{Path, PathBuf};
const POLL_BATCH: usize = 64;
pub(super) fn programs() -> Option<(PathBuf, PathBuf)> {
    use std::os::unix::fs::PermissionsExt;
    let value = std::env::var_os("PATH")?;
    if value.as_encoded_bytes().len() > 4096 {
        return None;
    }
    let paths: Vec<_> = std::env::split_paths(&value).collect();
    if paths.is_empty() || paths.len() > 64 || paths.iter().any(|p| !p.is_absolute()) {
        return None;
    }
    let find = |name: &str| {
        paths.iter().find_map(|dir| {
            let path = dir.join(name);
            let metadata = path.metadata().ok()?;
            (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .then(|| path.canonicalize().ok())
                .flatten()
        })
    };
    Some((find("squeue")?, find("sacct")?))
}
// Only fixed stage/category tokens; never format a raw error or report.
fn stage_reason(error: crate::polling_cycle::StageError) -> &'static str {
    use crate::polling_cycle::StageError;
    match error {
        StageError::Capacity => "capacity",
        StageError::Cancelled => "cancelled",
        StageError::Poll => "scheduler",
        StageError::Storage => "storage",
        StageError::Time => "time",
        StageError::Policy => "policy",
        StageError::DispatchBlocked => "dispatch_blocked",
        StageError::ClusterIdentity => "cluster_identity",
    }
}
fn fatal_stage(report: &crate::polling_cycle::TickReport) -> Option<(&'static str, &'static str)> {
    use crate::polling_cycle::StageError;
    let stages = [
        ("preflight", stage_error(report.preflight.as_ref()), true),
        ("poll", stage_error(report.poll.as_ref()), false),
        ("delivery", stage_error(report.delivery.as_ref()), false),
        ("cleanup", stage_error(report.cleanup.as_ref()), true),
        ("health", stage_error(report.health.as_ref()), true),
    ];
    stages.into_iter().find_map(|(stage, error, always)| {
        let error = error?;
        (always
            || matches!(
                error,
                StageError::Storage
                    | StageError::Time
                    | StageError::Policy
                    | StageError::ClusterIdentity
            ))
        .then_some((stage, stage_reason(error)))
    })
}
fn stage_error<T>(
    result: Option<&Result<T, crate::polling_cycle::StageError>>,
) -> Option<crate::polling_cycle::StageError> {
    result.and_then(|result| result.as_ref().err().copied())
}
fn schedule_reason(error: &crate::foreground_schedule::Error) -> &'static str {
    use crate::foreground_schedule::Error;
    match error {
        Error::Policy => "policy",
        Error::Clock => "clock",
        Error::ClockRegressed => "clock_regressed",
        Error::DeadlineOverflow => "deadline_overflow",
        Error::Wait => "wait",
        Error::NoProgress => "no_progress",
        Error::WakeLimit => "wake_limit",
        Error::Tick => "cycle",
    }
}
pub(super) fn run(path: &Path) -> u8 {
    use crate::{
        foreground_report::{Report, StartupReport},
        foreground_runtime::Summary,
        foreground_session::{Error, Session},
    };
    let signal = crate::foreground_wait::Signal::new();
    let binding = match crate::foreground_signals::bind(&signal) {
        Ok(binding) => binding,
        Err(_) => {
            eprintln!("Foreground signal handling unavailable.");
            return 1;
        }
    };
    let finish = |summary: Summary| {
        let code = summary.exit_code(binding.reason());
        let message = match code {
            130 => "Foreground monitoring stopped by SIGINT.",
            143 => "Foreground monitoring stopped by SIGTERM.",
            2 => "Configuration invalid or required resource unavailable.",
            _ if matches!(summary, Summary::Startup(_)) => {
                "Foreground monitoring could not acquire or initialize safe state."
            }
            _ => "Foreground monitoring stopped after a runtime or state failure.",
        };
        eprintln!("{message}");
        code
    };
    let plan = match crate::mixed_config_composition::prepare_path(
        path,
        crate::effective_user::lookup,
        crate::environment_resolution::lookup,
    ) {
        Ok(plan) => plan,
        Err(_) => {
            return finish(Summary::Startup(StartupReport::classify(
                Error::Preparation,
            )))
        }
    };
    if !plan.api_permissions_approved() {
        eprintln!("API email delivery permission is not approved for the current profile.");
        return 3;
    }
    if plan.webhook_origins().is_none() {
        return finish(Summary::Startup(StartupReport::classify(
            Error::Preparation,
        )));
    }
    let Some((active, accounting)) = programs() else {
        eprintln!("Required Slurm executables unavailable in absolute PATH directories.");
        return 2;
    };
    let helpers = match crate::provider_helpers::Selection::current() {
        Ok(helpers) => helpers,
        Err(_) => {
            return finish(Summary::Startup(StartupReport::classify(
                Error::Preparation,
            )))
        }
    };
    let timeout = plan.query_timeout();
    if let Err(error) =
        crate::cluster_probe::check(&active, plan.cluster(), timeout, plan.output_limit() / 2)
    {
        eprintln!(
            "{}",
            if error == crate::cluster_probe::Error::Mismatch {
                crate::cluster_probe::MISMATCH
            } else {
                "Slurm cluster identity probe failed; no monitoring state was opened."
            }
        );
        return if error == crate::cluster_probe::Error::Mismatch {
            2
        } else {
            1
        };
    }
    let cycles =
        crate::polling_cycle::Cycles::new(POLL_BATCH).expect("fixed valid internal poll batch");
    let mut session =
        match Session::start_prepared(plan, cycles, timeout, || signal.cancelled().unwrap_or(true))
        {
            Ok(session) => session,
            Err(error) => return finish(Summary::Startup(StartupReport::classify(error))),
        };
    eprintln!("Foreground monitoring started.");
    let result = session.run_continuous_interruptible(POLL_BATCH, &signal, |session| {
        let report = session.tick_public_with(
            (&active, &accounting),
            crate::polling_cycle::Hooks {
                query: crate::scheduler::run,
                send: (
                    crate::environment_resolution::lookup,
                    |_: &crate::storage::DueDelivery| {
                        helpers.command(crate::provider_helpers::Kind::Smtp)
                    },
                    |_: &crate::storage::DueDelivery| {
                        helpers.command(crate::provider_helpers::Kind::Webhook)
                    },
                ),
                clock: crate::delivery::unix_time,
                cancelled: || signal.cancelled().unwrap_or(true),
            },
        ).inspect_err(|_| {
            eprintln!("Foreground failure: stage=cycle reason=preparation.");
        })?;
        if report.cancelled {
            return Ok(report);
        }
        if let Some((stage, reason)) = fatal_stage(&report) {
            eprintln!("Foreground failure: stage={stage} reason={reason}.");
            if reason == "cluster_identity" {
                eprintln!("Configured monitor.cluster_id must match Slurm ClusterName; inspect scontrol show config and correct the configuration while stopped. State was not committed for this poll.");
            }
            return Err(Error::Cycle);
        }
        if report.poll.as_ref().is_some_and(Result::is_err) {
            eprintln!("Foreground scheduler poll failed; retrying at configured cadence.");
        }
        if report.delivery.as_ref().is_some_and(Result::is_err) {
            eprintln!("Foreground delivery blocked; work remains pending.");
        }
        if !signal.cancelled().unwrap_or(true) {
            session.prune_revisions().inspect_err(|_| {
                eprintln!("Foreground failure: stage=revision_cleanup reason=storage.");
            })?;
        }
        if let Some(Ok(health))=&report.health {
            println!("Foreground health: stale={} pending_sampled={} failed_sampled={} sample_truncated={}",
                health.poll.stale,health.storage.pending,health.storage.failed,health.storage.destinations_truncated);
        }
        Ok(report)
    });
    if let Err(error) = &result {
        eprintln!(
            "Foreground scheduling failure: reason={}.",
            schedule_reason(error)
        );
    }
    finish(Summary::Shutdown(Report::classify_continuous(&result)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polling_cycle::{StageError, TickReport};
    #[test]
    fn diagnostics_preserve_retry_and_first_fatal_stage_precedence() {
        let mut report = TickReport::default();
        assert_eq!(fatal_stage(&report), None);
        report.poll = Some(Err(StageError::Poll));
        report.delivery = Some(Err(StageError::DispatchBlocked));
        assert_eq!(fatal_stage(&report), None);
        report.health = Some(Err(StageError::Storage));
        assert_eq!(fatal_stage(&report), Some(("health", "storage")));
        report.cleanup = Some(Err(StageError::Time));
        assert_eq!(fatal_stage(&report), Some(("cleanup", "time")));
        report.delivery = Some(Err(StageError::Policy));
        assert_eq!(fatal_stage(&report), Some(("delivery", "policy")));
        report.poll = Some(Err(StageError::Storage));
        assert_eq!(fatal_stage(&report), Some(("poll", "storage")));
        report.poll = Some(Err(StageError::ClusterIdentity));
        assert_eq!(fatal_stage(&report), Some(("poll", "cluster_identity")));
        report.preflight = Some(Err(StageError::Capacity));
        assert_eq!(fatal_stage(&report), Some(("preflight", "capacity")));
        assert_eq!(stage_error(Some(&Ok::<_, StageError>(()))), None);
        for error in [
            StageError::Capacity,
            StageError::Cancelled,
            StageError::Poll,
            StageError::Storage,
            StageError::Time,
            StageError::Policy,
            StageError::DispatchBlocked,
        ] {
            let token = stage_reason(error);
            assert!(
                token.len() <= 16 && token.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            );
        }
        use crate::foreground_schedule::Error;
        for error in [
            Error::Policy,
            Error::Clock,
            Error::ClockRegressed,
            Error::DeadlineOverflow,
            Error::Wait,
            Error::NoProgress,
            Error::WakeLimit,
            Error::Tick,
        ] {
            let token = schedule_reason(&error);
            assert!(
                token.len() <= 20 && token.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            );
        }
    }
}
