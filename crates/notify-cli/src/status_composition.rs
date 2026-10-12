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
