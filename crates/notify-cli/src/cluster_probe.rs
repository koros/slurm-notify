//! Optional sibling scontrol config probe; no job data, state or provider activity.
use std::{path::Path, process::Command, time::Duration};
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Probe,
    Mismatch,
}
pub(super) const MISMATCH: &str="Slurm cluster identity mismatch; monitor.cluster_id must match the local ClusterName from scontrol show config. No monitoring state was opened.";
pub(super) fn check(
    active: &Path,
    expected: &str,
    timeout: Duration,
    limit: usize,
) -> Result<bool, Error> {
    use std::os::unix::fs::PermissionsExt;
    let path = active.parent().ok_or(Error::Probe)?.join("scontrol");
    match path.metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {}
        _ => return Err(Error::Probe),
    }
    let output = crate::scheduler::run(
        Command::new(path.canonicalize().map_err(|_| Error::Probe)?)
            .args(["show", "config"])
            .env_remove("SLURM_CLUSTERS"),
        timeout,
        limit,
    )
    .map_err(|_| Error::Probe)?;
    let mut names = output.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "ClusterName").then_some(value.trim())
    });
    let name = names.next().ok_or(Error::Probe)?;
    if names.next().is_some()
        || name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Probe);
    }
    // Slurm accounting stores cluster names in lower case.
    if name.to_ascii_lowercase() != expected {
        return Err(Error::Mismatch);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn optional_probe_matches_identity_and_bounds_failures_without_job_data() {
        let directory = crate::storage::tests::Directory::new();
        let root = directory.path().parent().unwrap().canonicalize().unwrap();
        let active = root.join("squeue");
        let control = root.join("scontrol");
        assert_eq!(
            check(&active, "cluster", Duration::from_secs(1), 1024),
            Ok(false)
        );
        let write = |body: &str| {
            std::fs::write(&control, body).unwrap();
            std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o700)).unwrap();
        };
        write("#!/bin/sh\nprintf 'ClusterName = ClUsTeR\\n'\n");
        assert_eq!(
            check(&active, "cluster", Duration::from_secs(1), 1024),
            Ok(true)
        );
        assert_eq!(
            check(&active, "wrong", Duration::from_secs(1), 1024),
            Err(Error::Mismatch)
        );
        for body in [
            "#!/bin/sh\nprintf 'other config\\n'\n",
            "#!/bin/sh\nprintf 'ClusterName = cluster\\nClusterName = cluster\\n'\n",
            "#!/bin/sh\nprintf 'ClusterName = unsafe/name\\n'\n",
            "#!/bin/sh\nexit 7\n",
        ] {
            write(body);
            assert_eq!(
                check(&active, "cluster", Duration::from_secs(1), 1024),
                Err(Error::Probe)
            );
        }
        write("#!/bin/sh\nprintf 'ClusterName = cluster\\n'\n");
        assert_eq!(
            check(&active, "cluster", Duration::from_secs(1), 4),
            Err(Error::Probe)
        );
        std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            check(&active, "cluster", Duration::from_secs(1), 1024),
            Err(Error::Probe)
        );
    }
}
