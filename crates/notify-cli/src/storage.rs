//! Private, test-only SQLite operations. No supported file path or storage API.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

const MIGRATION: &str = include_str!("../../notify-storage/migrations/0001.sql");

#[derive(Debug)]
enum StorageError {
    Database(rusqlite::Error),
    UnsupportedVersion,
    Conflict,
    InvalidInput,
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone)]
struct Job<'a> {
    cluster: &'a str,
    id: &'a str,
    submitted: &'a str,
    started: &'a str,
    state: &'a str,
}

struct Event<'a> {
    id: &'a str,
    job: Job<'a>,
    kind: &'a str,
    destinations: &'a [&'a str],
}

struct Store {
    db: Connection,
}

impl Store {
    // Caller owns a private test directory. Secure production creation remains pending.
    fn open(path: &Path, lock_timeout: Duration) -> Result<Self, StorageError> {
        let db = Connection::open(path)?;
        db.busy_timeout(lock_timeout)?;
        db.pragma_update(None, "foreign_keys", true)?;
        let enabled: i64 = db.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
        if enabled != 1 {
            return Err(StorageError::InvalidInput);
        }
        let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                if let Err(error) = db.execute_batch(MIGRATION) {
                    let _ = db.execute_batch("ROLLBACK");
                    return Err(error.into());
                }
            }
            1 => {}
            _ => return Err(StorageError::UnsupportedVersion),
        }
        Ok(Self { db })
    }

    // observed_at is Unix seconds in this private experiment. Network delivery
    // is outside this transaction. Replay does not silently change frozen routes.
    fn record_poll(
        &mut self,
        jobs: &[Job<'_>],
        events: &[Event<'_>],
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        if observed_at < 0
            || cluster.is_empty()
            || through.is_empty()
            || jobs.iter().any(|job| job.cluster != cluster)
            || events.iter().any(|event| event.job.cluster != cluster)
        {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for job in jobs {
            tx.execute(
                "INSERT INTO jobs VALUES (?1,?2,?3,?4,?5,?6)
                ON CONFLICT(cluster,job_id,submitted_at,started_at)
                DO UPDATE SET state=excluded.state, observed_at=excluded.observed_at",
                params![
                    job.cluster,
                    job.id,
                    job.submitted,
                    job.started,
                    job.state,
                    observed_at
                ],
            )?;
        }
        for event in events {
            let job = &event.job;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT event_id FROM events WHERE cluster=?1 AND job_id=?2
                AND submitted_at=?3 AND started_at=?4 AND event_type=?5",
                    params![job.cluster, job.id, job.submitted, job.started, event.kind],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                if id != event.id {
                    return Err(StorageError::Conflict);
                }
                continue;
            }
            tx.execute(
                "INSERT INTO events VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    event.id,
                    job.cluster,
                    job.id,
                    job.submitted,
                    job.started,
                    event.kind,
                    observed_at
                ],
            )?;
            for destination in event.destinations {
                tx.execute(
                    "INSERT INTO outbox VALUES (?1,?2,'pending',?3)",
                    params![event.id, destination, observed_at],
                )?;
            }
        }
        tx.execute(
            "INSERT INTO checkpoints VALUES (?1,?2) ON CONFLICT(cluster)
            DO UPDATE SET history_through=excluded.history_through",
            params![cluster, through],
        )?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "slurm-notify-storage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self(path)
        }
        fn path(&self) -> std::path::PathBuf {
            self.0.join("state.sqlite")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn job() -> Job<'static> {
        Job {
            cluster: "cluster",
            id: "1",
            submitted: "submit",
            started: "start",
            state: "COMPLETED",
        }
    }
    fn event(destinations: &'static [&'static str]) -> Event<'static> {
        Event {
            id: "event-1",
            job: job(),
            kind: "JOB_COMPLETED",
            destinations,
        }
    }
    fn open(directory: &Directory) -> Store {
        Store::open(&directory.path(), Duration::from_millis(50)).unwrap()
    }
    fn count(store: &Store, table: &str) -> i64 {
        store
            .db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn commit_reopen_and_replay_keep_one_event_and_frozen_routes() {
        let directory = Directory::new();
        let mut store = open(&directory);
        store
            .record_poll(
                &[job()],
                &[event(&["primary", "email"])],
                "cluster",
                "window1",
                10,
            )
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        store
            .record_poll(&[job()], &[event(&["new-route"])], "cluster", "window2", 11)
            .unwrap();
        assert_eq!(count(&store, "jobs"), 1);
        assert_eq!(count(&store, "events"), 1);
        assert_eq!(count(&store, "outbox"), 2);
        assert_eq!(
            store
                .db
                .query_row("SELECT history_through FROM checkpoints", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "window2"
        );
    }

    #[test]
    fn failed_fanout_rolls_back_all_poll_state() {
        let directory = Directory::new();
        let mut store = open(&directory);
        assert!(matches!(
            store.record_poll(
                &[job()],
                &[event(&["primary", ""])],
                "cluster",
                "window",
                10
            ),
            Err(StorageError::Database(_))
        ));
        for table in ["jobs", "events", "outbox", "checkpoints"] {
            assert_eq!(count(&store, table), 0);
        }
        store
            .record_poll(&[job()], &[event(&["primary"])], "cluster", "window", 10)
            .unwrap();
    }

    #[test]
    fn conflicting_event_id_rolls_back_checkpoint() {
        let directory = Directory::new();
        let mut store = open(&directory);
        store
            .record_poll(&[job()], &[event(&[])], "cluster", "old", 10)
            .unwrap();
        let mut conflicting = event(&[]);
        conflicting.id = "different";
        assert!(matches!(
            store.record_poll(&[job()], &[conflicting], "cluster", "new", 11),
            Err(StorageError::Conflict)
        ));
        assert_eq!(
            store
                .db
                .query_row("SELECT history_through FROM checkpoints", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "old"
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT observed_at FROM jobs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            10
        );
        assert_eq!(count(&store, "outbox"), 0); // suppressed event persists without delivery
    }

    #[test]
    fn future_version_is_not_reset() {
        let directory = Directory::new();
        let store = open(&directory);
        store.db.pragma_update(None, "user_version", 2).unwrap();
        drop(store);
        assert!(matches!(
            Store::open(&directory.path(), Duration::from_millis(50)),
            Err(StorageError::UnsupportedVersion)
        ));
        let db = Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn bounded_lock_wait_reports_busy_without_partial_commit() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let other = open(&directory);
        other.db.execute_batch("BEGIN IMMEDIATE").unwrap();
        let result = store.record_poll(&[job()], &[event(&[])], "cluster", "window", 10);
        match result {
            Err(StorageError::Database(rusqlite::Error::SqliteFailure(error, _))) => {
                assert_eq!(error.code, rusqlite::ErrorCode::DatabaseBusy)
            }
            _ => panic!("expected busy error"),
        }
        other.db.execute_batch("ROLLBACK").unwrap();
        assert_eq!(count(&store, "jobs"), 0);
    }

    #[test]
    fn invalid_input_and_orphan_events_leave_state_empty() {
        let directory = Directory::new();
        let mut store = open(&directory);
        assert!(matches!(
            store.record_poll(&[job()], &[], "different", "window", 10),
            Err(StorageError::InvalidInput)
        ));
        assert!(store
            .record_poll(&[], &[event(&[])], "cluster", "window", 10)
            .is_err());
        assert_eq!(count(&store, "events"), 0);
        assert_eq!(count(&store, "checkpoints"), 0);
    }
}
