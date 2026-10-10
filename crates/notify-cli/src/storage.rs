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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    StatePath(super::state_path::Error),
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

#[derive(Clone, Debug)]
pub(super) struct DueDelivery {
    pub(super) event_id: String,
    pub(super) destination: String,
    pub(super) event: super::lifecycle::Event,
    pub(super) attempt_number: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeliveryOutcome {
    Success,
    TransientFailure,
    TerminalFailure,
}

pub(super) struct Store {
    db: Connection,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _owner: Option<super::state_path::Guard>,
}

// Advisory read-only plan; IDs must be revalidated transactionally before deletion.
#[derive(Debug, PartialEq, Eq)]
struct RetentionCandidate {
    event_id: String,
    destination: String,
    completed_at: i64,
}

impl Store {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(super) fn open_private(
        directory: &Path,
        lock_timeout: Duration,
    ) -> Result<Self, StorageError> {
        let owner =
            super::state_path::Guard::acquire(directory).map_err(StorageError::StatePath)?;
        let db = Connection::open_with_flags(
            &owner.database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        let mut store = Self::initialize(db, lock_timeout)?;
        store._owner = Some(owner);
        Ok(store)
    }

    // Caller owns a private test directory. Secure production creation remains pending.
    pub(super) fn open(path: &Path, lock_timeout: Duration) -> Result<Self, StorageError> {
        Self::initialize(Connection::open(path)?, lock_timeout)
    }

    fn initialize(db: Connection, lock_timeout: Duration) -> Result<Self, StorageError> {
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
        Ok(Self {
            db,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            _owner: None,
        })
    }

    // Preserve events/jobs/checkpoints: they are the restart deduplication ledger.
    // Missing completion evidence is protected, including failed rows without attempt history.
    fn retention_plan(
        &self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        limit: usize,
    ) -> Result<Vec<RetentionCandidate>, StorageError> {
        if cluster.is_empty() || now < 0 || retain_seconds <= 0 || limit == 0 {
            return Err(StorageError::InvalidInput);
        }
        let limit = i64::try_from(limit).map_err(|_| StorageError::InvalidInput)?;
        let Some(cutoff) = now.checked_sub(retain_seconds).filter(|n| *n >= 0) else {
            return Ok(Vec::new());
        };
        let mut statement = self.db.prepare(
            "SELECT o.event_id,o.destination,MAX(a.attempted_at) AS completed_at
            FROM outbox o JOIN events e ON e.event_id=o.event_id
            JOIN delivery_attempts a ON a.event_id=o.event_id AND a.destination=o.destination
            WHERE e.cluster=?1 AND o.status IN ('delivered','failed')
            AND a.outcome IN ('success','terminal_failure')
            AND a.attempt_number=(SELECT MAX(last.attempt_number) FROM delivery_attempts last
                WHERE last.event_id=o.event_id AND last.destination=o.destination)
            GROUP BY o.event_id,o.destination HAVING completed_at < ?2
            ORDER BY completed_at,o.event_id,o.destination LIMIT ?3",
        )?;
        let rows = statement.query_map(params![cluster, cutoff, limit], |row| {
            Ok(RetentionCandidate {
                event_id: row.get(0)?,
                destination: row.get(1)?,
                completed_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub(super) fn due_deliveries(
        &self,
        cluster: &str,
        now: i64,
        limit: usize,
    ) -> Result<Vec<DueDelivery>, StorageError> {
        use super::lifecycle::{Event as LifecycleEvent, Identity, State};
        if cluster.is_empty() || now < 0 || limit == 0 {
            return Err(StorageError::InvalidInput);
        }
        let limit = i64::try_from(limit).map_err(|_| StorageError::InvalidInput)?;
        let mut statement = self.db.prepare("SELECT o.event_id,o.destination,e.job_id,e.submitted_at,
            e.started_at,e.event_type,COALESCE((SELECT MAX(a.attempt_number) FROM delivery_attempts a
                WHERE a.event_id=o.event_id AND a.destination=o.destination),0)+1
            FROM outbox o JOIN events e ON e.event_id=o.event_id
            WHERE e.cluster=?1 AND o.status='pending' AND o.next_attempt_at<=?2
            ORDER BY o.next_attempt_at,o.event_id,o.destination LIMIT ?3")?;
        let rows = statement.query_map(params![cluster, now, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        let mut deliveries = Vec::new();
        for row in rows {
            let (id, destination, job, submit, start, code, attempt_number) = row?;
            let event = LifecycleEvent {
                identity: Identity {
                    cluster: cluster.into(),
                    job,
                    submit,
                    start,
                },
                state: State::parse(&code),
            };
            if event_id(&event) != id
                || event.state.code() != code
                || (!event.state.terminal() && event.state != State::Running)
                || attempt_number <= 0
            {
                return Err(StorageError::Conflict);
            }
            deliveries.push(DueDelivery {
                event_id: id,
                destination,
                event,
                attempt_number,
            });
        }
        Ok(deliveries)
    }

    pub(super) fn record_delivery(
        &mut self,
        item: &DueDelivery,
        outcome: DeliveryOutcome,
        attempted_at: i64,
        retry_at: Option<i64>,
    ) -> Result<(), StorageError> {
        if attempted_at < 0
            || item.attempt_number <= 0
            || retry_at.map(|time| time <= attempted_at).unwrap_or(false)
            || (outcome != DeliveryOutcome::TransientFailure && retry_at.is_some())
        {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: Option<(i64, i64)> = tx
            .query_row(
                "SELECT next_attempt_at,
            COALESCE((SELECT MAX(attempt_number) FROM delivery_attempts
                WHERE event_id=?1 AND destination=?2),0)+1
            FROM outbox WHERE event_id=?1 AND destination=?2 AND status='pending'",
                params![item.event_id, item.destination],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if !matches!(pending,Some((due,attempt)) if due<=attempted_at && attempt==item.attempt_number)
        {
            return Err(StorageError::Conflict);
        }
        let code = match outcome {
            DeliveryOutcome::Success => "success",
            DeliveryOutcome::TransientFailure => "transient_failure",
            DeliveryOutcome::TerminalFailure => "terminal_failure",
        };
        tx.execute(
            "INSERT INTO delivery_attempts VALUES (?1,?2,?3,?4,?5)",
            params![
                item.event_id,
                item.destination,
                item.attempt_number,
                attempted_at,
                code
            ],
        )?;
        let status = match (outcome, retry_at) {
            (DeliveryOutcome::Success, _) => "delivered",
            (DeliveryOutcome::TransientFailure, Some(_)) => "pending",
            _ => "failed",
        };
        tx.execute(
            "UPDATE outbox SET status=?1,next_attempt_at=?2 WHERE event_id=?3 AND destination=?4",
            params![
                status,
                retry_at.unwrap_or(attempted_at),
                item.event_id,
                item.destination
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    // A lowered retry budget can retire already exhausted rows without inventing
    // an attempt that was never sent. One delivery owner remains required.
    pub(super) fn retire_exhausted(
        &mut self,
        item: &DueDelivery,
        max_attempts: u32,
    ) -> Result<(), StorageError> {
        if max_attempts == 0 || item.attempt_number <= i64::from(max_attempts) {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE outbox SET status='failed' WHERE event_id=?1 AND destination=?2
            AND status='pending' AND (SELECT COALESCE(MAX(attempt_number),0) FROM delivery_attempts
                WHERE event_id=?1 AND destination=?2)=?3",
            params![item.event_id, item.destination, item.attempt_number - 1],
        )?;
        if changed != 1 {
            return Err(StorageError::Conflict);
        }
        tx.commit()?;
        Ok(())
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

    fn retention_fixture(store: &mut Store) {
        use super::super::lifecycle::Reconciler;
        let mut reconciler = Reconciler::with_capacity(10);
        let events = reconciler
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        store
            .persist_reconciler(
                &reconciler,
                &events,
                &["done", "pending", "failed", "unsent", "retired"],
                "cluster",
                "2026-10-10T01:00:00",
                10,
            )
            .unwrap();
        for item in store.due_deliveries("cluster", 10, 10).unwrap() {
            match item.destination.as_str() {
                "done" => store
                    .record_delivery(&item, DeliveryOutcome::Success, 20, None)
                    .unwrap(),
                "failed" => store
                    .record_delivery(&item, DeliveryOutcome::TerminalFailure, 30, None)
                    .unwrap(),
                "retired" => {
                    store
                        .record_delivery(&item, DeliveryOutcome::TransientFailure, 40, Some(60))
                        .unwrap();
                }
                "unsent" => {
                    store.db.execute("UPDATE outbox SET status='failed' WHERE event_id=?1 AND destination=?2", params![item.event_id, item.destination]).unwrap();
                }
                _ => {}
            }
        }
        let retired = store
            .due_deliveries("cluster", 60, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "retired")
            .unwrap();
        store.retire_exhausted(&retired, 1).unwrap();
    }

    #[test]
    fn retention_plan_is_bounded_scoped_and_protects_pending_and_unsent() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let plan = store.retention_plan("cluster", 100, 50, 1).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].destination, "done");
        assert_eq!(plan[0].completed_at, 20);
        let plan = store.retention_plan("cluster", 100, 50, 10).unwrap();
        assert_eq!(
            plan.iter()
                .map(|c| c.destination.as_str())
                .collect::<Vec<_>>(),
            vec!["done", "failed"]
        );
        assert!(store
            .retention_plan("other", 100, 50, 10)
            .unwrap()
            .is_empty());
        assert_eq!(store.due_deliveries("cluster", 100, 10).unwrap().len(), 1);
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
    }

    #[test]
    fn retention_plan_preserves_restart_deduplication_and_strict_age_boundary() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        assert!(store
            .retention_plan("cluster", 70, 50, 10)
            .unwrap()
            .is_empty());
        let before = store.retention_plan("cluster", 100, 50, 10).unwrap();
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        assert_eq!(
            before,
            store.retention_plan("cluster", 100, 50, 10).unwrap()
        );
        let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(checkpoint.as_deref(), Some("2026-10-10T01:00:00"));
        assert!(reconciler
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true
            )
            .unwrap()
            .is_empty());
    }

    #[test]
    fn retention_invalid_policy_and_epoch_window_never_select_records() {
        let directory = Directory::new();
        let store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        for (cluster, now, age, limit) in [
            ("", 100, 50, 1),
            ("cluster", -1, 50, 1),
            ("cluster", 100, 0, 1),
            ("cluster", 100, -1, 1),
            ("cluster", 100, 50, 0),
        ] {
            assert!(matches!(
                store.retention_plan(cluster, now, age, limit),
                Err(StorageError::InvalidInput)
            ));
        }
        assert!(store
            .retention_plan("cluster", 10, i64::MAX, 1)
            .unwrap()
            .is_empty());
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
