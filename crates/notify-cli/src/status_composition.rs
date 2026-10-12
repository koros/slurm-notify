//! Private metadata-only status preparation; no installed status/configuration contract.
use crate::{
    config_validate::{self, get, object, text},
    polling_cycle::{Cycles, HealthReport, ReportError, StatusPolicy},
    storage::Store,
};
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Snapshot(crate::config_snapshot::Error),
    Validation(config_validate::Error),
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum InspectionError {
    Cancelled,
    State(crate::state_path::Error),
    Storage,
    UnsupportedVersion,
    Report(ReportError),
}
// Retain only status metadata. No identity, trust, credential or provider resources.
// No Debug/Clone: even cluster/path metadata must not become diagnostic output.
pub(super) struct Plan {
    cluster: Zeroizing<String>,
    path: Zeroizing<String>,
    capacities: (usize, usize),
    stale_after: i64,
    retain_seconds: i64,
    sample_limit: usize,
}
impl Plan {
    pub(super) fn storage_path(&self) -> &str {
        &self.path
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn inspect(
        &self,
        cycles: &Cycles,
        now: i64,
        lock_timeout: std::time::Duration,
    ) -> Result<HealthReport, InspectionError> {
        self.inspect_with(cycles, now, lock_timeout, || false)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn inspect_with(
        &self,
        cycles: &Cycles,
        now: i64,
        lock_timeout: std::time::Duration,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<HealthReport, InspectionError> {
        if cancelled() {
            return Err(InspectionError::Cancelled);
        }
        let parent = crate::state_path::database_parent(self.storage_path())
            .map_err(InspectionError::State)?;
        let mut store = Store::open_status(parent, lock_timeout).map_err(|error| match error {
            crate::storage::StorageError::StatePath(error) => InspectionError::State(error),
            crate::storage::StorageError::UnsupportedVersion => InspectionError::UnsupportedVersion,
            _ => InspectionError::Storage,
        })?;
        if cancelled() {
            return Err(InspectionError::Cancelled);
        }
        let report = self
            .status(cycles, &mut store, now)
            .map_err(InspectionError::Report)?;
        if cancelled() {
            return Err(InspectionError::Cancelled);
        }
        Ok(report)
    }
    pub(super) fn status(
        &self,
        cycles: &Cycles,
        store: &mut Store,
        now: i64,
    ) -> Result<HealthReport, ReportError> {
        cycles.status(
            StatusPolicy {
                cluster: &self.cluster,
                capacities: self.capacities,
                stale_after: self.stale_after,
                retain_seconds: self.retain_seconds,
                sample_limit: self.sample_limit,
            },
            store,
            now,
        )
    }
}
pub(super) fn prepare(bytes: &[u8]) -> Result<Plan, Error> {
    let draft = config_validate::validate(bytes).map_err(Error::Validation)?;
    let extract = || -> Result<_, config_validate::Error> {
        let root = object(&draft.value)?;
        let monitor = object(get(root, "monitor")?)?;
        let storage = object(get(root, "storage")?)?;
        Ok((
            Zeroizing::new(text(get(monitor, "cluster_id")?)?.to_owned()),
            Zeroizing::new(text(get(storage, "path")?)?.to_owned()),
        ))
    };
    let (cluster, path) = extract().map_err(Error::Validation)?;
    Ok(Plan {
        cluster,
        path,
        capacities: (draft.controls.identity_limit, draft.controls.event_limit),
        stale_after: draft.controls.maintenance.stale_after,
        retain_seconds: draft.controls.maintenance.retain_seconds,
        sample_limit: draft.controls.maintenance.health_sample_limit,
    })
}
pub(super) fn prepare_path(path: &std::path::Path) -> Result<Plan, Error> {
    let snapshot = crate::config_snapshot::read(path).map_err(Error::Snapshot)?;
    prepare(snapshot.bytes())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{io::Write, os::unix::fs::OpenOptionsExt, path::Path, time::Duration};
    fn fixture(directory: &crate::storage::tests::Directory) -> Value {
        let parent = directory.path().parent().unwrap().canonicalize().unwrap();
        let mut value: Value = serde_json::from_slice(include_bytes!(
            "../../../docs/proposals/examples/local-mixed-config.json"
        ))
        .unwrap();
        value["storage"]["path"] = json!(parent.join("state").join("state.sqlite3"));
        value["providers"]["research_email"]["trust_der_files"] =
            json!([parent.join("absent-trust.der")]);
        value["providers"]["primary"]["url_env"] = json!("UNAVAILABLE_URL");
        value["providers"]["primary"]["authorization_env"] = json!("UNAVAILABLE_AUTHORIZATION");
        value["providers"]["research_email"]["authentication"]["username_env"] =
            json!("UNAVAILABLE_USERNAME");
        value["providers"]["research_email"]["authentication"]["password_env"] =
            json!("UNAVAILABLE_PASSWORD");
        value
    }
    fn write(path: &Path, bytes: &[u8]) {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    fn inspect_fixture(plan: &Plan) -> Result<HealthReport, InspectionError> {
        let until = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match plan.inspect(&Cycles::new(1).unwrap(), 10, Duration::from_millis(50)) {
                Err(InspectionError::State(crate::state_path::Error::AlreadyOwned))
                    if std::time::Instant::now() < until =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                result => return result,
            }
        }
    }
    fn files(path: &Path) -> Vec<(String, Vec<u8>)> {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_str().unwrap().to_owned(),
                    std::fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
    #[test]
    fn inspection_cancellation_precedes_access_and_releases_acquired_owner() {
        let directory = crate::storage::tests::Directory::new();
        let plan = prepare(&serde_json::to_vec(&fixture(&directory)).unwrap()).unwrap();
        let state = Path::new(plan.storage_path()).parent().unwrap();
        let cycles = Cycles::new(1).unwrap();
        assert!(matches!(
            plan.inspect_with(&cycles, 10, Duration::from_millis(50), || true),
            Err(InspectionError::Cancelled)
        ));
        assert!(!state.exists());
        drop(Store::open_private(state, Duration::from_millis(50)).unwrap());
        let before = files(state);
        for stop in [2, 3] {
            let until = std::time::Instant::now() + Duration::from_secs(1);
            loop {
                let mut calls = 0;
                let result = plan.inspect_with(&cycles, 10, Duration::from_millis(50), || {
                    calls += 1;
                    calls == stop
                });
                match result {
                    Err(InspectionError::State(crate::state_path::Error::AlreadyOwned))
                        if std::time::Instant::now() < until =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    other => {
                        assert!(matches!(other, Err(InspectionError::Cancelled)));
                        break;
                    }
                }
            }
            assert_eq!(files(state), before);
            assert!(inspect_fixture(&plan).is_ok());
        }
    }
    #[test]
    fn inspection_does_not_create_absent_state_and_rejects_concurrent_owner() {
        let directory = crate::storage::tests::Directory::new();
        let source = fixture(&directory);
        let plan = prepare(&serde_json::to_vec(&source).unwrap()).unwrap();
        let state = Path::new(plan.storage_path()).parent().unwrap();
        let cycles = Cycles::new(1).unwrap();
        assert!(matches!(
            plan.inspect(&cycles, 10, Duration::from_millis(50)),
            Err(InspectionError::State(crate::state_path::Error::Io))
        ));
        assert!(!state.exists());
        let owner = Store::open_private(state, Duration::from_millis(50)).unwrap();
        let before = files(state);
        assert!(matches!(
            plan.inspect(&cycles, 10, Duration::from_millis(50)),
            Err(InspectionError::State(
                crate::state_path::Error::AlreadyOwned
            ))
        ));
        assert_eq!(files(state), before);
        drop(owner);
        let report = inspect_fixture(&plan).unwrap();
        assert!(report.poll.stale);
        assert_eq!(files(state), before);
        std::fs::remove_file(state.join(".owner.lock")).unwrap();
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::State(crate::state_path::Error::Io))
        ));
        assert!(!state.join(".owner.lock").exists());
    }
    #[test]
    fn inspection_rejects_unsafe_files_recovery_artifacts_and_old_schema_without_repair() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let directory = crate::storage::tests::Directory::new();
        let source = fixture(&directory);
        let plan = prepare(&serde_json::to_vec(&source).unwrap()).unwrap();
        let state = Path::new(plan.storage_path()).parent().unwrap();
        drop(Store::open_private(state, Duration::from_millis(50)).unwrap());
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::State(
                crate::state_path::Error::UnsafePermissions
            ))
        ));
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700)).unwrap();
        for suffix in ["-journal", "-wal", "-shm"] {
            let sidecar = state.join(format!("state.sqlite3{suffix}"));
            write(&sidecar, b"synthetic recovery artifact");
            let before = files(state);
            assert!(matches!(
                inspect_fixture(&plan),
                Err(InspectionError::State(
                    crate::state_path::Error::UnsupportedState
                ))
            ));
            assert_eq!(files(state), before);
            std::fs::remove_file(sidecar).unwrap();
        }
        let path = state.join("state.sqlite3");
        let saved = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let target = state.parent().unwrap().join("sentinel");
        write(&target, b"synthetic sentinel");
        symlink(&target, &path).unwrap();
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::State(crate::state_path::Error::Io))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"synthetic sentinel");
        std::fs::remove_file(&path).unwrap();
        write(&path, &saved);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.pragma_update(None, "user_version", 3).unwrap();
        drop(db);
        let before = files(state);
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::UnsupportedVersion)
        ));
        assert_eq!(files(state), before);
        let mut wal_header = saved.clone();
        wal_header[18] = 2;
        wal_header[19] = 2;
        std::fs::write(&path, &wal_header).unwrap();
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::State(
                crate::state_path::Error::UnsupportedState
            ))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), wal_header);
        std::fs::write(&path, b"not a SQLite database").unwrap();
        assert!(matches!(
            inspect_fixture(&plan),
            Err(InspectionError::State(
                crate::state_path::Error::UnsupportedState
            ))
        ));
        assert_eq!(std::fs::read(path).unwrap(), b"not a SQLite database");
    }
    #[test]
    fn inspection_reads_pending_health_and_enforces_sqlite_read_only_without_changes() {
        let directory = crate::storage::tests::Directory::new();
        let source = fixture(&directory);
        let plan = prepare(&serde_json::to_vec(&source).unwrap()).unwrap();
        let state = Path::new(plan.storage_path()).parent().unwrap();
        let mut owner = Store::open_private(state, Duration::from_millis(50)).unwrap();
        let mut reconciler = crate::lifecycle::Reconciler::with_capacity(10);
        let events = reconciler
            .observe_snapshot(
                "",
                "research-cluster|1|alice|s|actual|COMPLETED|0:0",
                "research-cluster",
                "alice",
                true,
            )
            .unwrap();
        owner
            .persist_reconciler(
                &reconciler,
                &events,
                &["primary"],
                "research-cluster",
                "1970-01-01T00:00:10",
                10,
            )
            .unwrap();
        drop(owner);
        let before = files(state);
        let report = inspect_fixture(&plan).unwrap();
        assert_eq!(report.poll.checkpoint_age_seconds, Some(0));
        assert!(!report.poll.stale);
        let mut readonly = {
            let until = std::time::Instant::now() + Duration::from_secs(1);
            loop {
                match Store::open_status(state, Duration::from_millis(50)) {
                    Ok(store) => break store,
                    Err(crate::storage::StorageError::StatePath(
                        crate::state_path::Error::AlreadyOwned,
                    )) if std::time::Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("read-only fixture failed: {error:?}"),
                }
            }
        };
        let pending = readonly.due_deliveries("research-cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(readonly
            .persist_reconciler(
                &reconciler,
                &[],
                &["primary"],
                "research-cluster",
                "1970-01-01T00:00:11",
                11
            )
            .is_err());
        drop(readonly);
        assert_eq!(files(state), before);
        assert_eq!(
            inspect_fixture(&plan).unwrap().poll.checkpoint_age_seconds,
            Some(0)
        );
    }
    #[test]
    fn unavailable_resources_do_not_prevent_owned_metadata_preparation() {
        let directory = crate::storage::tests::Directory::new();
        let source = fixture(&directory);
        let mut bytes = serde_json::to_vec(&source).unwrap();
        let plan = prepare(&bytes).unwrap();
        bytes.fill(0);
        assert_eq!(plan.cluster.as_str(), "research-cluster");
        assert_eq!(plan.capacities, (10000, 20000));
        assert_eq!(plan.stale_after, 180);
        assert_eq!(plan.retain_seconds, 604800);
        assert_eq!(plan.sample_limit, 100);
        assert_eq!(
            plan.storage_path(),
            source["storage"]["path"].as_str().unwrap()
        );
        assert!(!Path::new(plan.storage_path()).exists());
        assert!(!Path::new(
            source["providers"]["research_email"]["trust_der_files"][0]
                .as_str()
                .unwrap()
        )
        .exists());
    }
    #[test]
    fn full_draft_and_protected_read_validation_remain_required() {
        use std::os::unix::fs::PermissionsExt;
        let directory = crate::storage::tests::Directory::new();
        let mut source = fixture(&directory);
        source["providers"]["research_email"]["ignored"] = json!(true);
        assert!(matches!(
            prepare(&serde_json::to_vec(&source).unwrap()),
            Err(Error::Validation(_))
        ));
        source["providers"]["research_email"]
            .as_object_mut()
            .unwrap()
            .remove("ignored");
        source["routes"][0]["send_to"] = json!(["missing"]);
        assert!(matches!(
            prepare(&serde_json::to_vec(&source).unwrap()),
            Err(Error::Validation(_))
        ));
        let path = directory
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("status.json");
        assert!(matches!(
            prepare_path(&path),
            Err(Error::Snapshot(crate::config_snapshot::Error::Io))
        ));
        write(&path, b"{");
        assert!(matches!(prepare_path(&path), Err(Error::Validation(_))));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            prepare_path(&path),
            Err(Error::Snapshot(
                crate::config_snapshot::Error::UnsafePermissions
            ))
        ));
    }
    #[test]
    fn protected_metadata_status_reports_pressure_after_reopen_without_provider_readiness() {
        let directory = crate::storage::tests::Directory::new();
        let mut source = fixture(&directory);
        source["monitor"]["retained_identity_limit"] = json!(1);
        source["monitor"]["retained_event_limit"] = json!(1);
        let path = directory
            .path()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .join("status.json");
        write(&path, &serde_json::to_vec(&source).unwrap());
        let plan = prepare_path(&path).unwrap();
        let state = Path::new(plan.storage_path()).parent().unwrap();
        // State acquisition remains an explicit caller action, not status preparation.
        let mut store = Store::open_private(state, Duration::from_millis(50)).unwrap();
        let mut reconciler = crate::lifecycle::Reconciler::with_capacities(1, 2);
        for (status, time) in [("RUNNING", 9), ("COMPLETED", 10)] {
            let events = reconciler
                .observe_snapshot(
                    "",
                    &format!("research-cluster|1|alice|s|actual|{status}|0:0"),
                    "research-cluster",
                    "alice",
                    true,
                )
                .unwrap();
            store
                .persist_reconciler(
                    &reconciler,
                    &events,
                    &["primary"],
                    "research-cluster",
                    "1970-01-01T00:00:10",
                    time,
                )
                .unwrap();
        }
        let pending = store.due_deliveries("research-cluster", 10, 10).unwrap();
        assert_eq!(pending.len(), 2);
        drop(store);
        // Replacing the file cannot alter an already owned status plan.
        let replacement = path.with_extension("replacement");
        write(&replacement, b"invalid replacement");
        std::fs::rename(replacement, &path).unwrap();
        assert!(matches!(prepare_path(&path), Err(Error::Validation(_))));
        let mut store = {
            let until = std::time::Instant::now() + Duration::from_secs(1);
            loop {
                match Store::open_private(state, Duration::from_millis(50)) {
                    Ok(store) => break store,
                    Err(crate::storage::StorageError::StatePath(
                        crate::state_path::Error::AlreadyOwned,
                    )) if std::time::Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("status fixture reopen failed: {error:?}"),
                }
            }
        };
        let cycles = Cycles::new(1).unwrap();
        for (now, stale) in [(10, false), (189, false), (190, true)] {
            let report = plan.status(&cycles, &mut store, now).unwrap();
            assert_eq!(report.poll.stale, stale);
            assert_eq!(report.poll.checkpoint_age_seconds, Some(now - 10));
            assert!(!report.storage.jobs_over_capacity && report.storage.events_over_capacity);
            let printed = format!("{report:?}");
            assert!(
                !printed.contains("alice")
                    && !printed.contains("UNAVAILABLE")
                    && !printed.contains("research-cluster")
            );
        }
        let after = store.due_deliveries("research-cluster", 190, 10).unwrap();
        assert_eq!(after.len(), pending.len());
        for (before, after) in pending.iter().zip(after) {
            assert_eq!(
                (&before.event_id, before.attempt_number),
                (&after.event_id, after.attempt_number)
            );
        }
    }
}
