//! Read-only resource preflight with bounded, non-job Slurm version probes.
use crate::{config_snapshot, config_validate, installed_config, installed_monitor, scheduler};
use std::{path::Path, process::Command, time::Instant};
pub(super) enum Error {
    Config,
    Permission,
    Programs,
    Probe,
    ClusterMismatch,
}
pub(super) fn run(path: &Path) -> Result<String, Error> {
    let snapshot = config_snapshot::read(path).map_err(|_| Error::Config)?;
    installed_config::validate_bytes(snapshot.bytes()).map_err(|error| match error {
        installed_config::Error::Invalid => Error::Config,
        installed_config::Error::Permission => Error::Permission,
    })?;
    let draft = config_validate::validate(snapshot.bytes()).map_err(|_| Error::Config)?;
    let (active, accounting) = installed_monitor::programs().ok_or(Error::Programs)?;
    let deadline = Instant::now()
        .checked_add(draft.controls.query_timeout)
        .ok_or(Error::Probe)?;
    // Each runner bounds stdout and stderr independently; reserve half the total
    // output budget for each stream and share that budget across both probes.
    let mut remaining = draft.controls.output_limit;
    for program in [&active, &accounting] {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or(Error::Probe)?;
        let limit = remaining / 2;
        if limit == 0 {
            return Err(Error::Probe);
        }
        let output = scheduler::run(Command::new(program).arg("--version"), timeout, limit)
            .map_err(|_| Error::Probe)?;
        if output.trim().is_empty()
            || !output
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace())
        {
            return Err(Error::Probe);
        }
        // A successful query has bounded but discarded stderr. Reserve its entire
        // allowance rather than pretending the captured stdout is the whole output.
        remaining = remaining
            .checked_sub(limit + output.len())
            .ok_or(Error::Probe)?;
    }
    let root = crate::config_validate::object(&draft.value).map_err(|_| Error::Config)?;
    let monitor = crate::config_validate::object(
        crate::config_validate::get(root, "monitor").map_err(|_| Error::Config)?,
    )
    .map_err(|_| Error::Config)?;
    let cluster = crate::config_validate::text(
        crate::config_validate::get(monitor, "cluster_id").map_err(|_| Error::Config)?,
    )
    .map_err(|_| Error::Config)?;
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .ok_or(Error::Probe)?;
    crate::cluster_probe::check(&active, cluster, timeout, remaining / 2).map_err(|error| {
        match error {
            crate::cluster_probe::Error::Mismatch => Error::ClusterMismatch,
            _ => Error::Probe,
        }
    })?;
    Ok(include_str!("../../../contracts/cli/doctor-success.stdout").into())
}
