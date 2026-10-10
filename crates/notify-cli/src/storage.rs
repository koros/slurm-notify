//! Private, test-only SQLite operations. No supported file path or storage API.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

const MIGRATION: &str = include_str!("../../notify-storage/migrations/0001.sql");

#[derive(Debug)]
pub(super) enum StorageError {
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

// Length-prefixed UTF-8 fields are unambiguous without hashing or a new dependency.
// Internal prototype ID only: not a published webhook/event contract.
fn event_id(event: &super::lifecycle::Event) -> String {
    let mut id = String::from("internal-v1:");
    for field in [
        &event.identity.cluster,
        &event.identity.job,
        &event.identity.submit,
        &event.identity.start,
        event.state.code(),
    ] {
        id.push_str(&format!("{}:{field}", field.len()));
    }
    id
}

pub(super) struct Store {
    db: Connection,
}

impl Store {
    // Caller owns a private test directory. Secure production creation remains pending.
    pub(super) fn open(path: &Path, lock_timeout: Duration) -> Result<Self, StorageError> {
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

    pub(super) fn persist_reconciler(
        &mut self,
        candidate: &super::lifecycle::Reconciler,
        deliverable: &[super::lifecycle::Event],
        destinations: &[&str],
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        let jobs: Vec<_> = candidate
            .latest
            .iter()
            .map(|(identity, state)| Job {
                cluster: &identity.cluster,
                id: &identity.job,
                submitted: &identity.submit,
                started: &identity.start,
                state: state.code(),
            })
            .collect();
        let ledger: Vec<_> = candidate.emitted.iter().collect();
        let ids: Vec<_> = ledger.iter().map(|event| event_id(event)).collect();
        let events: Vec<_> = ledger
            .iter()
            .zip(&ids)
            .map(|(event, id)| Event {
                id,
                job: Job {
                    cluster: &event.identity.cluster,
                    id: &event.identity.job,
                    submitted: &event.identity.submit,
                    started: &event.identity.start,
                    state: event.state.code(),
                },
                kind: event.state.code(),
                destinations: if deliverable.contains(event) {
                    destinations
                } else {
                    &[]
                },
            })
            .collect();
        self.record_poll(&jobs, &events, cluster, through, observed_at)
    }

    // A read transaction gives jobs/events/checkpoint one consistent snapshot.
    // LIMIT bounds restored rows; an overflow fails rather than discarding history.
    pub(super) fn restore(
        &mut self,
        cluster: &str,
        capacity: usize,
    ) -> Result<(super::lifecycle::Reconciler, Option<String>), StorageError> {
        use super::lifecycle::{Event as LifecycleEvent, Identity, Reconciler, State};
        let limit = i64::try_from(capacity)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(StorageError::InvalidInput)?;
        let tx = self.db.transaction()?;
        let mut restored = Reconciler::with_capacity(capacity);
        let mut statement = tx.prepare(
            "SELECT job_id,submitted_at,started_at,state FROM jobs WHERE cluster=?1 LIMIT ?2",
        )?;
        let rows = statement.query_map(params![cluster, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (job, submit, start, code) = row?;
            let state = State::parse(&code);
            if state.code() != code {
                return Err(StorageError::Conflict);
            }
            restored.latest.insert(
                Identity {
                    cluster: cluster.into(),
                    job,
                    submit,
                    start,
                },
                state,
            );
        }
        drop(statement);
        let mut statement = tx.prepare("SELECT event_id,job_id,submitted_at,started_at,event_type FROM events WHERE cluster=?1 LIMIT ?2")?;
        let rows = statement.query_map(params![cluster, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (id, job, submit, start, code) = row?;
            let state = State::parse(&code);
            let event = LifecycleEvent {
                identity: Identity {
                    cluster: cluster.into(),
                    job,
                    submit,
                    start,
                },
                state,
            };
            if (!event.state.terminal() && event.state != State::Running)
                || event.state.code() != code
                || event_id(&event) != id
                || !restored.latest.contains_key(&event.identity)
            {
                return Err(StorageError::Conflict);
            }
            restored.emitted.insert(event);
        }
        drop(statement);
        if restored.latest.len() > capacity || restored.emitted.len() > capacity {
            return Err(StorageError::InvalidInput);
        }
        let checkpoint = tx
            .query_row(
                "SELECT history_through FROM checkpoints WHERE cluster=?1",
                [cluster],
                |row| row.get(0),
            )
            .optional()?;
        tx.commit()?;
        Ok((restored, checkpoint))
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
        let previous: Option<String> = tx
            .query_row(
                "SELECT history_through FROM checkpoints WHERE cluster=?1",
                [cluster],
                |row| row.get(0),
            )
            .optional()?;
        if previous
            .as_deref()
            .map(|checkpoint| checkpoint > through)
            .unwrap_or(false)
        {
            return Err(StorageError::Conflict);
        }
        // Promotion removes only a pending identity. Keep historical actual
        // attempts and their event references for restart deduplication.
        for job in jobs.iter().filter(|job| !job.started.is_empty()) {
            tx.execute(
                "DELETE FROM jobs WHERE cluster=?1 AND job_id=?2 AND submitted_at=?3
                AND started_at='' AND state='PENDING'",
                params![job.cluster, job.id, job.submitted],
            )?;
        }
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
pub(super) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    pub(crate) struct Directory(std::path::PathBuf);
    impl Directory {
        pub(crate) fn new() -> Self {
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
        pub(crate) fn path(&self) -> std::path::PathBuf {
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
    fn event_identity_encoding_is_deterministic_and_unambiguous() {
        use super::super::lifecycle::{Event as LifecycleEvent, Identity, State};
        let first = LifecycleEvent {
            identity: Identity {
                cluster: "a:b".into(),
                job: "c".into(),
                submit: "s".into(),
                start: "t".into(),
            },
            state: State::Running,
        };
        let second = LifecycleEvent {
            identity: Identity {
                cluster: "a".into(),
                job: "b:c".into(),
                submit: "s".into(),
                start: "t".into(),
            },
            state: State::Running,
        };
        assert_eq!(event_id(&first), event_id(&first.clone()));
        assert_ne!(event_id(&first), event_id(&second));
        let mut terminal = first.clone();
        terminal.state = State::Completed;
        assert_ne!(event_id(&first), event_id(&terminal));
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
