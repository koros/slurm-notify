//! Installed prototype foreground command. No service installation or implicit config.
use std::path::{Path, PathBuf};
const POLL_BATCH: usize = 64;
fn programs() -> Option<(PathBuf, PathBuf)> {
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
        use crate::polling_cycle::StageError;
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
        )?;
        if report.cancelled {
            return Ok(report);
        }
        let fatal = report.preflight.as_ref().is_some_and(Result::is_err)
            || matches!(
                report.poll,
                Some(Err(StageError::Storage
                    | StageError::Time
                    | StageError::Policy))
            )
            || matches!(
                report.delivery,
                Some(Err(StageError::Storage
                    | StageError::Time
                    | StageError::Policy))
            )
            || report.cleanup.as_ref().is_some_and(Result::is_err)
            || report.health.as_ref().is_some_and(Result::is_err);
        if fatal {
            return Err(Error::Cycle);
        }
        if report.poll.as_ref().is_some_and(Result::is_err) {
            eprintln!("Foreground scheduler poll failed; retrying at configured cadence.");
        }
        if report.delivery.as_ref().is_some_and(Result::is_err) {
            eprintln!("Foreground delivery blocked; work remains pending.");
        }
        if !signal.cancelled().unwrap_or(true) { session.prune_revisions()?; }
        if let Some(Ok(health))=&report.health {
            println!("Foreground health: stale={} pending_sampled={} failed_sampled={} sample_truncated={}",
                health.poll.stale,health.storage.pending,health.storage.failed,health.storage.destinations_truncated);
        }
        Ok(report)
    });
    finish(Summary::Shutdown(Report::classify_continuous(&result)))
}
