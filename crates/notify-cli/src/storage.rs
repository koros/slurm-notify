//! Private, test-only SQLite operations. No supported file path or storage API.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

const PROVIDER_MIGRATION: &str = include_str!("../../notify-storage/migrations/0004.sql");
const REVISION_MIGRATION: &str = include_str!("../../notify-storage/migrations/0003.sql");
const SMTP_MIGRATION: &str = include_str!("../../notify-storage/migrations/0002.sql");

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

// Frozen values never implement Debug/Clone and never contain relay credentials.
pub(super) struct FrozenMessage {
    sender: String,
    recipient: String,
    data: zeroize::Zeroizing<Vec<u8>>,
}
impl FrozenMessage {
    pub(super) fn envelope(&self) -> (&str, &str) {
        (&self.sender, &self.recipient)
    }
    pub(super) fn data(&self) -> &[u8] {
        &self.data
    }
}

// No Debug/Clone: this batch borrows validated private message buffers.
pub(super) struct SmtpPreparation<'a> {
    pub(super) event: &'a super::lifecycle::Event,
    pub(super) destination: &'a str,
    pub(super) message: &'a super::smtp_message::Prepared,
}

pub(super) struct PollPreparation<'a> {
    pub(super) candidate: &'a super::lifecycle::Reconciler,
    pub(super) deliverable: &'a [super::lifecycle::Event],
    pub(super) destinations: &'a [&'a str],
    pub(super) smtp_destinations: &'a [&'a str],
    pub(super) messages: &'a [SmtpPreparation<'a>],
}

pub(super) struct SmtpRevisions<'a> {
    pub(super) rendering: &'a [super::smtp_revision::Descriptor],
    pub(super) providers: &'a [super::smtp_provider_revision::Descriptor],
}

struct SmtpPlan<'a> {
    providers: std::collections::HashMap<&'a str, &'a super::smtp_provider_revision::Descriptor>,
    revisions: std::collections::HashMap<&'a str, &'a super::smtp_revision::Descriptor>,
    destinations: &'a [&'a str],
    messages: std::collections::HashMap<(String, &'a str), &'a super::smtp_message::Prepared>,
}

pub(super) struct Store {
    db: Connection,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _owner: Option<super::state_path::Guard>,
}

// Advisory read-only plan; IDs must be revalidated transactionally before deletion.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct RetentionCandidate {
    event_id: String,
    destination: String,
    completed_at: i64,
    attempt_number: i64,
    status: String,
}

// Counts describe a bounded sample; truncation explicitly prevents exact totals.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct StorageHealth {
    jobs_sampled: usize,
    events_sampled: usize,
    pub(super) jobs_over_capacity: bool,
    pub(super) events_over_capacity: bool,
    destinations_sampled: usize,
    destinations_truncated: bool,
    pending: usize,
    due: usize,
    delivered: usize,
    failed: usize,
    cleanup_eligible: usize,
    completion_unproven: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HealthError {
    InvalidInput,
    Database,
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
            1..=4 => {}
            _ => return Err(StorageError::UnsupportedVersion),
        }
        if version < 2 {
            if let Err(error) = db.execute_batch(SMTP_MIGRATION) {
                let _ = db.execute_batch("ROLLBACK");
                return Err(error.into());
            }
        }
        if version < 3 {
            if let Err(error) = db.execute_batch(REVISION_MIGRATION) {
                let _ = db.execute_batch("ROLLBACK");
                return Err(error.into());
            }
        }
        if version < 4 {
            if let Err(error) = db.execute_batch(PROVIDER_MIGRATION) {
                let _ = db.execute_batch("ROLLBACK");
                return Err(error.into());
            }
        }
        Ok(Self {
            db,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            _owner: None,
        })
    }

    // Constant-size report; read transaction keeps all samples consistent.
    // LIMIT bounds inspected rows, not SQLite wall time or underlying scan work.
    pub(super) fn health(
        &mut self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        capacity: usize,
        sample_limit: usize,
    ) -> Result<StorageHealth, HealthError> {
        self.health_with_capacities(
            cluster,
            now,
            retain_seconds,
            (capacity, capacity),
            sample_limit,
        )
    }
    pub(super) fn health_with_capacities(
        &mut self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        capacities: (usize, usize),
        sample_limit: usize,
    ) -> Result<StorageHealth, HealthError> {
        self.health_snapshot_using(
            cluster,
            now,
            retain_seconds,
            capacities,
            sample_limit,
            false,
        )
        .map(|(health, _)| health)
    }
    pub(super) fn health_snapshot(
        &mut self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        capacities: (usize, usize),
        sample_limit: usize,
    ) -> Result<(StorageHealth, Option<String>), HealthError> {
        self.health_snapshot_using(cluster, now, retain_seconds, capacities, sample_limit, true)
    }
    fn health_snapshot_using(
        &mut self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        capacities: (usize, usize),
        sample_limit: usize,
        include_checkpoint: bool,
    ) -> Result<(StorageHealth, Option<String>), HealthError> {
        let (capacity, event_capacity) = capacities;
        if cluster.is_empty()
            || now < 0
            || retain_seconds <= 0
            || capacity == 0
            || event_capacity == 0
            || sample_limit == 0
        {
            return Err(HealthError::InvalidInput);
        }
        let capacity_probe = i64::try_from(capacity)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(HealthError::InvalidInput)?;
        let event_probe = i64::try_from(event_capacity)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(HealthError::InvalidInput)?;
        let sample_probe = i64::try_from(sample_limit)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(HealthError::InvalidInput)?;
        let cutoff = now.checked_sub(retain_seconds).unwrap_or(-1);
        let report = (|| -> Result<(StorageHealth, Option<String>), rusqlite::Error> {
            let tx = self.db.transaction()?;
            let jobs: usize = tx.query_row(
                "SELECT count(*) FROM (SELECT 1 FROM jobs WHERE cluster=?1 LIMIT ?2)",
                params![cluster, capacity_probe],
                |r| {
                    let count: i64 = r.get(0)?;
                    usize::try_from(count)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, count))
                },
            )?;
            let events: usize = tx.query_row(
                "SELECT count(*) FROM (SELECT 1 FROM events WHERE cluster=?1 LIMIT ?2)",
                params![cluster, event_probe],
                |r| {
                    let count: i64 = r.get(0)?;
                    usize::try_from(count)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, count))
                },
            )?;
            let mut health = StorageHealth {
                jobs_sampled: jobs,
                events_sampled: events,
                jobs_over_capacity: jobs > capacity,
                events_over_capacity: events > event_capacity,
                destinations_sampled: 0,
                destinations_truncated: false,
                pending: 0,
                due: 0,
                delivered: 0,
                failed: 0,
                cleanup_eligible: 0,
                completion_unproven: 0,
            };
            let mut statement = tx.prepare(
                "SELECT o.status,o.next_attempt_at,a.attempted_at,a.outcome
                FROM events e JOIN outbox o ON o.event_id=e.event_id
                LEFT JOIN delivery_attempts a ON a.event_id=o.event_id AND a.destination=o.destination
                    AND a.attempt_number=(SELECT MAX(last.attempt_number) FROM delivery_attempts last
                        WHERE last.event_id=o.event_id AND last.destination=o.destination)
                WHERE e.cluster=?1 ORDER BY o.event_id,o.destination LIMIT ?2")?;
            let rows = statement.query_map(params![cluster, sample_probe], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            for row in rows {
                let (status, due_at, attempted_at, outcome) = row?;
                if health.destinations_sampled == sample_limit {
                    health.destinations_truncated = true;
                    break;
                }
                health.destinations_sampled += 1;
                match status.as_str() {
                    "pending" => {
                        health.pending += 1;
                        if due_at <= now {
                            health.due += 1;
                        }
                    }
                    "delivered" | "failed" => {
                        if status == "delivered" {
                            health.delivered += 1;
                        } else {
                            health.failed += 1;
                        }
                        match (attempted_at, outcome.as_deref()) {
                            (Some(time), Some("success" | "terminal_failure")) => {
                                if time < cutoff {
                                    health.cleanup_eligible += 1;
                                }
                            }
                            _ => health.completion_unproven += 1,
                        }
                    }
                    _ => return Err(rusqlite::Error::InvalidQuery),
                }
            }
            drop(statement);
            let checkpoint = if include_checkpoint {
                tx
                .query_row(
                    "SELECT length(history_through),CASE WHEN length(history_through)=19 THEN history_through ELSE '' END FROM checkpoints WHERE cluster=?1",
                    [cluster],
                    |row| {
                        if row.get::<_, i64>(0)? != 19 { return Err(rusqlite::Error::InvalidQuery); }
                        row.get(1)
                    },
                )
                .optional()?
            } else {
                None
            };
            tx.commit()?;
            Ok((health, checkpoint))
        })();
        report.map_err(|_| HealthError::Database)
    }

    // Preserve events/jobs/checkpoints: they are the restart deduplication ledger.
    // Missing completion evidence is protected, including failed rows without attempt history.
    pub(super) fn retention_plan(
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
            "SELECT o.event_id,o.destination,MAX(a.attempted_at) AS completed_at,a.attempt_number,o.status
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
                attempt_number: row.get(3)?,
                status: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub(super) fn apply_retention(
        &mut self,
        cluster: &str,
        now: i64,
        retain_seconds: i64,
        limit: usize,
        candidates: &[RetentionCandidate],
    ) -> Result<usize, StorageError> {
        if cluster.is_empty()
            || now < 0
            || retain_seconds <= 0
            || limit == 0
            || candidates.len() > limit
        {
            return Err(StorageError::InvalidInput);
        }
        let cutoff = now.checked_sub(retain_seconds).unwrap_or(-1);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut seen = std::collections::HashSet::new();
        for candidate in candidates {
            if !seen.insert((&candidate.event_id, &candidate.destination)) {
                return Err(StorageError::InvalidInput);
            }
            let current: Option<(i64, i64, String)> = tx
                .query_row(
                    "SELECT a.attempted_at,a.attempt_number,o.status
                FROM outbox o JOIN events e ON e.event_id=o.event_id
                JOIN delivery_attempts a ON a.event_id=o.event_id AND a.destination=o.destination
                WHERE e.cluster=?1 AND o.event_id=?2 AND o.destination=?3
                AND o.status IN ('delivered','failed')
                AND a.outcome IN ('success','terminal_failure')
                AND a.attempt_number=(SELECT MAX(last.attempt_number) FROM delivery_attempts last
                    WHERE last.event_id=o.event_id AND last.destination=o.destination)",
                    params![cluster, candidate.event_id, candidate.destination],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            if current
                != Some((
                    candidate.completed_at,
                    candidate.attempt_number,
                    candidate.status.clone(),
                ))
                || candidate.completed_at < 0
                || candidate.completed_at >= cutoff
            {
                return Err(StorageError::Conflict);
            }
            tx.execute(
                "DELETE FROM delivery_attempts WHERE event_id=?1 AND destination=?2",
                params![candidate.event_id, candidate.destination],
            )?;
            if tx.execute(
                "DELETE FROM outbox WHERE event_id=?1 AND destination=?2",
                params![candidate.event_id, candidate.destination],
            )? != 1
            {
                return Err(StorageError::Conflict);
            }
        }
        tx.commit()?;
        Ok(candidates.len())
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

    // Separate committed preparation before first network attempt. Existing
    // snapshots are immutable; identical repeated preparation is idempotent.
    pub(super) fn freeze_smtp(
        &mut self,
        event_id: &str,
        destination: &str,
        message: &super::smtp_message::Prepared,
    ) -> Result<(), StorageError> {
        let (sender, recipient) = message.envelope();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<bool> = tx.query_row("SELECT sender=?3 AND recipient=?4 AND data=?5 FROM smtp_messages WHERE event_id=?1 AND destination=?2", params![event_id,destination,sender,recipient,message.data()], |row| row.get(0)).optional()?;
        if let Some(identical) = existing {
            if !identical {
                return Err(StorageError::Conflict);
            }
            tx.commit()?;
            return Ok(());
        }
        let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox o WHERE o.event_id=?1 AND o.destination=?2 AND o.status='pending' AND NOT EXISTS(SELECT 1 FROM delivery_attempts a WHERE a.event_id=o.event_id AND a.destination=o.destination))", params![event_id,destination], |row| row.get(0))?;
        if !eligible {
            return Err(StorageError::Conflict);
        }
        tx.execute(
            "INSERT INTO smtp_messages VALUES (?1,?2,?3,?4,?5)",
            params![event_id, destination, sender, recipient, message.data()],
        )?;
        tx.commit()?;
        Ok(())
    }

    // Explicit preparation of new snapshots only; never infer legacy provenance.
    pub(super) fn freeze_smtp_revision(
        &mut self,
        event_id: &str,
        destination: &str,
        message: &super::smtp_message::Prepared,
        revision: &super::smtp_revision::Descriptor,
    ) -> Result<(), StorageError> {
        let envelope = message.envelope();
        if revision.destination() != destination
            || revision.envelope() != (envelope.0.as_str(), envelope.1.as_str())
        {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cluster: String = tx
            .query_row(
                "SELECT cluster FROM events WHERE event_id=?1",
                [event_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StorageError::Conflict)?;
        tx.execute(
            "INSERT INTO smtp_render_revisions VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING",
            params![cluster, destination, revision.token(), revision.canonical()],
        )?;
        let identical: bool = tx.query_row("SELECT descriptor=?4 FROM smtp_render_revisions WHERE cluster=?1 AND destination=?2 AND revision=?3", params![cluster,destination,revision.token(),revision.canonical()], |row| row.get(0))?;
        if !identical {
            return Err(StorageError::Conflict);
        }
        let (sender, recipient) = message.envelope();
        let existing: Option<bool> = tx.query_row("SELECT s.sender=?3 AND s.recipient=?4 AND s.data=?5 AND EXISTS(SELECT 1 FROM smtp_revision_bindings b WHERE b.event_id=s.event_id AND b.destination=s.destination AND b.cluster=?6 AND b.revision=?7) FROM smtp_messages s WHERE s.event_id=?1 AND s.destination=?2", params![event_id,destination,sender,recipient,message.data(),cluster,revision.token()], |row| row.get(0)).optional()?;
        if let Some(identical) = existing {
            if !identical {
                return Err(StorageError::Conflict);
            }
        } else {
            let eligible: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox o WHERE o.event_id=?1 AND o.destination=?2 AND o.status='pending' AND NOT EXISTS(SELECT 1 FROM delivery_attempts a WHERE a.event_id=o.event_id AND a.destination=o.destination))", params![event_id,destination], |row| row.get(0))?;
            if !eligible {
                return Err(StorageError::Conflict);
            }
            tx.execute(
                "INSERT INTO smtp_messages VALUES (?1,?2,?3,?4,?5)",
                params![event_id, destination, sender, recipient, message.data()],
            )?;
            tx.execute(
                "INSERT INTO smtp_revision_bindings VALUES (?1,?2,?3,?4)",
                params![event_id, destination, cluster, revision.token()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(super) fn bound_smtp_revision(
        &self,
        event_id: &str,
        destination: &str,
    ) -> Result<Option<super::smtp_revision::Descriptor>, StorageError> {
        type Row = (Option<String>, Option<Vec<u8>>, bool);
        let row: Option<Row> = self.db.query_row("SELECT CASE WHEN length(r.revision)=64 THEN r.revision END, CASE WHEN typeof(r.descriptor)='blob' AND length(r.descriptor) BETWEEN 1 AND 4096 THEN r.descriptor END, b.cluster=e.cluster FROM smtp_revision_bindings b LEFT JOIN smtp_render_revisions r ON r.cluster=b.cluster AND r.destination=b.destination AND r.revision=b.revision LEFT JOIN events e ON e.event_id=b.event_id WHERE b.event_id=?1 AND b.destination=?2", params![event_id,destination], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        let Some((token, bytes, coherent)) = row else {
            return Ok(None);
        };
        if !coherent {
            return Err(StorageError::InvalidInput);
        }
        let descriptor = super::smtp_revision::Descriptor::from_stored(
            &token.ok_or(StorageError::InvalidInput)?,
            zeroize::Zeroizing::new(bytes.ok_or(StorageError::InvalidInput)?),
        )
        .map_err(|_| StorageError::InvalidInput)?;
        let snapshot = self
            .frozen_smtp(event_id, destination)?
            .ok_or(StorageError::InvalidInput)?;
        if descriptor.destination() != destination || descriptor.envelope() != snapshot.envelope() {
            return Err(StorageError::InvalidInput);
        }
        Ok(Some(descriptor))
    }

    pub(super) fn bound_smtp_provider(
        &self,
        event_id: &str,
        destination: &str,
    ) -> Result<Option<super::smtp_provider_revision::Descriptor>, StorageError> {
        type Row = (Option<String>, Option<Vec<u8>>, bool);
        let row: Option<Row> = self.db.query_row("SELECT CASE WHEN length(r.revision)=64 THEN r.revision END, CASE WHEN typeof(r.descriptor)='blob' AND length(r.descriptor) BETWEEN 1 AND 4096 THEN r.descriptor END, COALESCE(b.cluster=e.cluster AND b.cluster=s.cluster,0) FROM smtp_provider_bindings b LEFT JOIN smtp_provider_revisions r ON r.cluster=b.cluster AND r.destination=b.destination AND r.revision=b.revision LEFT JOIN events e ON e.event_id=b.event_id LEFT JOIN smtp_revision_bindings s ON s.event_id=b.event_id AND s.destination=b.destination WHERE b.event_id=?1 AND b.destination=?2", params![event_id,destination], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        let Some((token, bytes, coherent)) = row else {
            return Ok(None);
        };
        if !coherent {
            return Err(StorageError::InvalidInput);
        }
        let provider = super::smtp_provider_revision::Descriptor::from_stored(
            &token.ok_or(StorageError::InvalidInput)?,
            zeroize::Zeroizing::new(bytes.ok_or(StorageError::InvalidInput)?),
        )
        .map_err(|_| StorageError::InvalidInput)?;
        if provider.profile().destination != destination
            || self.bound_smtp_revision(event_id, destination)?.is_none()
        {
            return Err(StorageError::InvalidInput);
        }
        Ok(Some(provider))
    }

    // One bounded transaction; every binding (including pending work) protects
    // its descriptor. This operation does not remove snapshots or ledger history.
    pub(super) fn prune_smtp_revisions(
        &mut self,
        cluster: &str,
        limit: usize,
    ) -> Result<usize, StorageError> {
        if cluster.is_empty() || cluster.len() > 4096 || limit == 0 || limit > 256 {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute("DELETE FROM smtp_render_revisions WHERE rowid IN (SELECT r.rowid FROM smtp_render_revisions r WHERE r.cluster=?1 AND NOT EXISTS(SELECT 1 FROM smtp_revision_bindings b WHERE b.cluster=r.cluster AND b.destination=r.destination AND b.revision=r.revision) ORDER BY r.destination,r.revision LIMIT ?2) AND NOT EXISTS(SELECT 1 FROM smtp_revision_bindings b WHERE b.cluster=smtp_render_revisions.cluster AND b.destination=smtp_render_revisions.destination AND b.revision=smtp_render_revisions.revision)", params![cluster,limit as i64])?;
        tx.commit()?;
        Ok(removed)
    }

    // One bounded transaction; every binding (including pending work) protects
    // its descriptor. This operation does not remove snapshots or ledger history.
    pub(super) fn prune_smtp_provider_revisions(
        &mut self,
        cluster: &str,
        limit: usize,
    ) -> Result<usize, StorageError> {
        if cluster.is_empty() || cluster.len() > 4096 || limit == 0 || limit > 256 {
            return Err(StorageError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute("DELETE FROM smtp_provider_revisions WHERE rowid IN (SELECT r.rowid FROM smtp_provider_revisions r WHERE r.cluster=?1 AND NOT EXISTS(SELECT 1 FROM smtp_provider_bindings b WHERE b.cluster=r.cluster AND b.destination=r.destination AND b.revision=r.revision) ORDER BY r.destination,r.revision LIMIT ?2) AND NOT EXISTS(SELECT 1 FROM smtp_provider_bindings b WHERE b.cluster=smtp_provider_revisions.cluster AND b.destination=smtp_provider_revisions.destination AND b.revision=smtp_provider_revisions.revision)", params![cluster,limit as i64])?;
        tx.commit()?;
        Ok(removed)
    }

    // Durable SMTP evidence cannot be downgraded to an ordinary callback.
    pub(super) fn has_smtp_artifacts(
        &self,
        event_id: &str,
        destination: &str,
    ) -> Result<bool, StorageError> {
        self.db.query_row("SELECT EXISTS(SELECT 1 FROM smtp_messages WHERE event_id=?1 AND destination=?2) OR EXISTS(SELECT 1 FROM smtp_revision_bindings WHERE event_id=?1 AND destination=?2) OR EXISTS(SELECT 1 FROM smtp_provider_bindings WHERE event_id=?1 AND destination=?2)", params![event_id, destination], |row| row.get(0)).map_err(StorageError::from)
    }

    pub(super) fn frozen_smtp(
        &self,
        event_id: &str,
        destination: &str,
    ) -> Result<Option<FrozenMessage>, StorageError> {
        // SQL bounds before BLOB materialization defend against oversized owned
        // state. A malformed existing row must fail, never masquerade as missing.
        type FrozenRow = (Option<String>, Option<String>, Option<Vec<u8>>);
        let value: Option<FrozenRow> = self.db.query_row("SELECT CASE WHEN length(CAST(sender AS BLOB)) BETWEEN 1 AND 254 THEN sender END, CASE WHEN length(CAST(recipient AS BLOB)) BETWEEN 1 AND 254 THEN recipient END, CASE WHEN typeof(data)='blob' AND length(data) BETWEEN 4 AND 16384 THEN data END FROM smtp_messages WHERE event_id=?1 AND destination=?2", params![event_id,destination], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        let Some((sender, recipient, data)) = value else {
            return Ok(None);
        };
        let sender = sender.ok_or(StorageError::InvalidInput)?;
        let recipient = recipient.ok_or(StorageError::InvalidInput)?;
        let data = zeroize::Zeroizing::new(data.ok_or(StorageError::InvalidInput)?);
        super::smtp_message::address(&sender).map_err(|_| StorageError::InvalidInput)?;
        super::smtp_message::address(&recipient).map_err(|_| StorageError::InvalidInput)?;
        super::smtp_message::validate_frozen_data(&data).map_err(|_| StorageError::InvalidInput)?;
        Ok(Some(FrozenMessage {
            sender,
            recipient,
            data,
        }))
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
        self.persist_candidate(
            PollPreparation {
                candidate,
                deliverable,
                destinations,
                smtp_destinations: &[],
                messages: &[],
            },
            cluster,
            through,
            observed_at,
            SmtpPlan {
                providers: std::collections::HashMap::new(),
                revisions: std::collections::HashMap::new(),
                destinations: &[],
                messages: std::collections::HashMap::new(),
            },
        )
    }

    pub(super) fn persist_routed_reconciler<'a>(
        &mut self,
        candidate: &super::lifecycle::Reconciler,
        deliverable: &[super::lifecycle::Event],
        cluster: &str,
        through: &str,
        observed_at: i64,
        select: impl FnMut(&super::lifecycle::Event) -> Vec<&'a str>,
    ) -> Result<(), StorageError> {
        self.persist_selected(
            PollPreparation {
                candidate,
                deliverable,
                destinations: &[],
                smtp_destinations: &[],
                messages: &[],
            },
            cluster,
            through,
            observed_at,
            SmtpPlan {
                providers: std::collections::HashMap::new(),
                revisions: std::collections::HashMap::new(),
                destinations: &[],
                messages: std::collections::HashMap::new(),
            },
            select,
        )
    }

    // Rendering occurs before this call; all eligible new pairs commit together.
    // Historical events retain their routes and never consume new preparations.
    pub(super) fn persist_prepared_reconciler(
        &mut self,
        preparation: PollPreparation<'_>,
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        self.persist_prepared(
            preparation,
            (None, None),
            cluster,
            through,
            observed_at,
            None,
        )
    }

    pub(super) fn persist_prepared_revisions<'a>(
        &mut self,
        preparation: PollPreparation<'a>,
        revisions: &'a [super::smtp_revision::Descriptor],
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        self.persist_prepared(
            preparation,
            (Some(revisions), None),
            cluster,
            through,
            observed_at,
            None,
        )
    }

    pub(super) fn persist_prepared_profiles<'a>(
        &mut self,
        preparation: PollPreparation<'a>,
        revisions: SmtpRevisions<'a>,
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        self.persist_prepared(
            preparation,
            (Some(revisions.rendering), Some(revisions.providers)),
            cluster,
            through,
            observed_at,
            None,
        )
    }

    pub(super) fn persist_selected_profiles<'a>(
        &mut self,
        preparation: PollPreparation<'a>,
        revisions: SmtpRevisions<'a>,
        cluster: &str,
        through: &str,
        observed_at: i64,
        selected: &std::collections::HashMap<super::lifecycle::Event, Vec<&'a str>>,
    ) -> Result<(), StorageError> {
        self.persist_prepared(
            preparation,
            (Some(revisions.rendering), Some(revisions.providers)),
            cluster,
            through,
            observed_at,
            Some(selected),
        )
    }

    fn persist_prepared<'a>(
        &mut self,
        preparation: PollPreparation<'a>,
        bindings: (
            Option<&'a [super::smtp_revision::Descriptor]>,
            Option<&'a [super::smtp_provider_revision::Descriptor]>,
        ),
        cluster: &str,
        through: &str,
        observed_at: i64,
        selected: Option<&std::collections::HashMap<super::lifecycle::Event, Vec<&'a str>>>,
    ) -> Result<(), StorageError> {
        let (revisions, providers) = bindings;
        let PollPreparation {
            candidate,
            deliverable,
            destinations,
            smtp_destinations,
            messages,
        } = preparation;
        let mut routes = std::collections::HashSet::new();
        let mut smtp_routes = std::collections::HashSet::new();
        let mut deliveries = std::collections::HashSet::new();
        if destinations
            .iter()
            .any(|name| name.is_empty() || !routes.insert(*name))
            || smtp_destinations
                .iter()
                .any(|name| !routes.contains(name) || !smtp_routes.insert(*name))
            || deliverable
                .iter()
                .any(|event| !candidate.emitted.contains(event) || !deliveries.insert(event))
        {
            return Err(StorageError::InvalidInput);
        }
        if let Some(selected) = selected {
            if selected.len() != deliverable.len()
                || deliverable
                    .iter()
                    .any(|event| !selected.contains_key(event))
                || selected.values().any(|names| {
                    let mut seen = std::collections::HashSet::new();
                    names.len() > 64
                        || names
                            .iter()
                            .any(|name| !routes.contains(name) || !seen.insert(name))
                })
            {
                return Err(StorageError::InvalidInput);
            }
        }
        let mut smtp = SmtpPlan {
            providers: std::collections::HashMap::new(),
            revisions: std::collections::HashMap::new(),
            destinations: smtp_destinations,
            messages: std::collections::HashMap::new(),
        };
        if let Some(revisions) = revisions {
            if revisions.len() != smtp_routes.len() || revisions.is_empty() {
                return Err(StorageError::InvalidInput);
            }
            for revision in revisions {
                if !revision.revision_identity()
                    || !smtp_routes.contains(revision.destination())
                    || smtp
                        .revisions
                        .insert(revision.destination(), revision)
                        .is_some()
                {
                    return Err(StorageError::InvalidInput);
                }
            }
            for message in messages {
                let revision = smtp
                    .revisions
                    .get(message.destination)
                    .ok_or(StorageError::InvalidInput)?;
                let envelope = message.message.envelope();
                if revision.envelope() != (envelope.0.as_str(), envelope.1.as_str()) {
                    return Err(StorageError::InvalidInput);
                }
            }
        }
        if let Some(providers) = providers {
            if providers.len() != smtp.revisions.len() || providers.is_empty() {
                return Err(StorageError::InvalidInput);
            }
            for provider in providers {
                let profile = provider.profile();
                if !smtp.revisions.contains_key(profile.destination)
                    || smtp
                        .providers
                        .insert(profile.destination, provider)
                        .is_some()
                {
                    return Err(StorageError::InvalidInput);
                }
            }
        }
        for message in messages {
            if !deliveries.contains(message.event)
                || !smtp_routes.contains(message.destination)
                || selected.is_some_and(|selected| {
                    !selected
                        .get(message.event)
                        .is_some_and(|names| names.contains(&message.destination))
                })
                || smtp
                    .messages
                    .insert(
                        (event_id(message.event), message.destination),
                        message.message,
                    )
                    .is_some()
            {
                return Err(StorageError::InvalidInput);
            }
        }
        if let Some(selected) = selected {
            self.persist_selected(preparation, cluster, through, observed_at, smtp, |event| {
                selected.get(event).cloned().unwrap_or_default()
            })
        } else {
            self.persist_candidate(preparation, cluster, through, observed_at, smtp)
        }
    }

    fn persist_candidate(
        &mut self,
        preparation: PollPreparation<'_>,
        cluster: &str,
        through: &str,
        observed_at: i64,
        smtp: SmtpPlan<'_>,
    ) -> Result<(), StorageError> {
        let destinations = preparation.destinations;
        self.persist_selected(preparation, cluster, through, observed_at, smtp, |_| {
            destinations.to_vec()
        })
    }
    fn persist_selected<'a>(
        &mut self,
        preparation: PollPreparation<'_>,
        cluster: &str,
        through: &str,
        observed_at: i64,
        smtp: SmtpPlan<'_>,
        mut select: impl FnMut(&super::lifecycle::Event) -> Vec<&'a str>,
    ) -> Result<(), StorageError> {
        let PollPreparation {
            candidate,
            deliverable,
            ..
        } = preparation;
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
        let selected: Vec<_> = ledger
            .iter()
            .map(|event| {
                if deliverable.contains(event) {
                    select(event)
                } else {
                    Vec::new()
                }
            })
            .collect();
        let events: Vec<_> = ledger
            .iter()
            .zip(&ids)
            .zip(&selected)
            .map(|((event, id), destinations)| Event {
                id,
                job: Job {
                    cluster: &event.identity.cluster,
                    id: &event.identity.job,
                    submitted: &event.identity.submit,
                    started: &event.identity.start,
                    state: event.state.code(),
                },
                kind: event.state.code(),
                destinations,
            })
            .collect();
        self.record_poll_with_smtp(&jobs, &events, cluster, through, observed_at, smtp)
    }

    // A read transaction gives jobs/events/checkpoint one consistent snapshot.
    // LIMIT bounds restored rows; an overflow fails rather than discarding history.
    pub(super) fn restore(
        &mut self,
        cluster: &str,
        capacity: usize,
    ) -> Result<(super::lifecycle::Reconciler, Option<String>), StorageError> {
        self.restore_with_capacities(cluster, (capacity, capacity))
    }
    pub(super) fn restore_with_capacities(
        &mut self,
        cluster: &str,
        capacities: (usize, usize),
    ) -> Result<(super::lifecycle::Reconciler, Option<String>), StorageError> {
        let (capacity, event_capacity) = capacities;
        use super::lifecycle::{Event as LifecycleEvent, Identity, Reconciler, State};
        let limit = i64::try_from(capacity)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(StorageError::InvalidInput)?;
        let tx = self.db.transaction()?;
        let event_limit = i64::try_from(event_capacity)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or(StorageError::InvalidInput)?;
        let mut restored = Reconciler::with_capacities(capacity, event_capacity);
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
        let rows = statement.query_map(params![cluster, event_limit], |row| {
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
        if restored.latest.len() > capacity || restored.emitted.len() > event_capacity {
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
    #[cfg(test)]
    fn record_poll(
        &mut self,
        jobs: &[Job<'_>],
        events: &[Event<'_>],
        cluster: &str,
        through: &str,
        observed_at: i64,
    ) -> Result<(), StorageError> {
        self.record_poll_with_smtp(
            jobs,
            events,
            cluster,
            through,
            observed_at,
            SmtpPlan {
                providers: std::collections::HashMap::new(),
                revisions: std::collections::HashMap::new(),
                destinations: &[],
                messages: std::collections::HashMap::new(),
            },
        )
    }

    fn record_poll_with_smtp<'a>(
        &mut self,
        jobs: &[Job<'_>],
        events: &[Event<'a>],
        cluster: &str,
        through: &str,
        observed_at: i64,
        mut smtp: SmtpPlan<'a>,
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
                if smtp.destinations.contains(destination) {
                    let message = smtp
                        .messages
                        .remove(&(event.id.to_owned(), *destination))
                        .ok_or(StorageError::InvalidInput)?;
                    let (sender, recipient) = message.envelope();
                    tx.execute(
                        "INSERT INTO smtp_messages VALUES (?1,?2,?3,?4,?5)",
                        params![event.id, destination, sender, recipient, message.data()],
                    )?;
                    if let Some(revision) = smtp.revisions.get(destination) {
                        tx.execute("INSERT INTO smtp_render_revisions VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING", params![cluster,destination,revision.token(),revision.canonical()])?;
                        let identical: bool = tx.query_row("SELECT descriptor=?4 FROM smtp_render_revisions WHERE cluster=?1 AND destination=?2 AND revision=?3", params![cluster,destination,revision.token(),revision.canonical()], |row| row.get(0))?;
                        if !identical {
                            return Err(StorageError::Conflict);
                        }
                        tx.execute(
                            "INSERT INTO smtp_revision_bindings VALUES (?1,?2,?3,?4)",
                            params![event.id, destination, cluster, revision.token()],
                        )?;
                    }
                    if let Some(provider) = smtp.providers.get(destination) {
                        tx.execute("INSERT INTO smtp_provider_revisions VALUES (?1,?2,?3,?4) ON CONFLICT DO NOTHING", params![cluster,destination,provider.token(),provider.canonical()])?;
                        let identical: bool = tx.query_row("SELECT descriptor=?4 FROM smtp_provider_revisions WHERE cluster=?1 AND destination=?2 AND revision=?3", params![cluster,destination,provider.token(),provider.canonical()], |row| row.get(0))?;
                        if !identical {
                            return Err(StorageError::Conflict);
                        }
                        tx.execute(
                            "INSERT INTO smtp_provider_bindings VALUES (?1,?2,?3,?4)",
                            params![event.id, destination, cluster, provider.token()],
                        )?;
                    }
                }
            }
        }
        // Extra/historical preparations must not silently overwrite stored bytes.
        if !smtp.messages.is_empty() {
            return Err(StorageError::InvalidInput);
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
    fn independent_health_probes_validate_each_capacity_and_preserve_shared_adapter() {
        let directory = Directory::new();
        let mut store = open(&directory);
        retention_fixture(&mut store);
        let shared = store.health("cluster", 100, 50, 1, 10).unwrap();
        assert_eq!(
            shared,
            store
                .health_with_capacities("cluster", 100, 50, (1, 1), 10)
                .unwrap()
        );
        for capacities in [(0, 1), (1, 0), (usize::MAX, 1), (1, usize::MAX)] {
            assert_eq!(
                store
                    .health_with_capacities("cluster", 100, 50, capacities, 10)
                    .unwrap_err(),
                HealthError::InvalidInput
            );
        }
        for capacities in [(usize::MAX, 1), (1, usize::MAX)] {
            assert!(matches!(
                store.restore_with_capacities("cluster", capacities),
                Err(StorageError::InvalidInput)
            ));
        }
    }
    #[test]
    fn health_reports_queue_protection_and_cleanup_changes_after_reopen() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let before = store.health("cluster", 100, 50, 10, 10).unwrap();
        assert_eq!((before.jobs_sampled, before.events_sampled), (1, 1));
        assert_eq!(
            (before.pending, before.due, before.delivered, before.failed),
            (1, 1, 1, 3)
        );
        assert_eq!(
            (before.cleanup_eligible, before.completion_unproven),
            (2, 2)
        );
        assert!(!before.destinations_truncated);
        assert_eq!(
            store
                .health("cluster", 70, 50, 10, 10)
                .unwrap()
                .cleanup_eligible,
            0
        );
        assert_eq!(store.health("cluster", 0, 50, 10, 10).unwrap().due, 0);
        let plan = store.retention_plan("cluster", 100, 50, 10).unwrap();
        store
            .apply_retention("cluster", 100, 50, 10, &plan)
            .unwrap();
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        let after = store.health("cluster", 100, 50, 10, 10).unwrap();
        assert_eq!(
            (
                after.destinations_sampled,
                after.cleanup_eligible,
                after.completion_unproven
            ),
            (3, 0, 2)
        );
        assert_eq!(
            (after.jobs_sampled, after.events_sampled, after.pending),
            (1, 1, 1)
        );
        assert_eq!(
            store
                .health("other", 100, 50, 10, 10)
                .unwrap()
                .destinations_sampled,
            0
        );
        let display = format!("{after:?}");
        for private in ["cluster", "alice", "internal-v1:", "unsent", "actual"] {
            assert!(!display.contains(private));
        }
    }

    #[test]
    fn health_samples_are_explicitly_truncated_and_capacity_pressure_is_detected() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let sample = store.health("cluster", 100, 50, 1, 1).unwrap();
        assert_eq!(sample.destinations_sampled, 1);
        assert!(sample.destinations_truncated);
        assert!(!sample.jobs_over_capacity && !sample.events_over_capacity);
        store.db.execute_batch("INSERT INTO jobs VALUES ('cluster','2','s','actual','RUNNING',10); INSERT INTO events SELECT 'fixture-extra',cluster,job_id,submitted_at,started_at,'RUNNING',10 FROM jobs WHERE job_id='2';").unwrap();
        let sample = store.health("cluster", 100, 50, 1, 5).unwrap();
        assert!(sample.jobs_over_capacity && sample.events_over_capacity);
        assert_eq!((sample.jobs_sampled, sample.events_sampled), (2, 2));
        assert!(!sample.destinations_truncated);
        assert!(matches!(
            store.restore("cluster", 1),
            Err(StorageError::Conflict)
        ));
    }

    #[test]
    fn health_errors_are_sanitized_and_invalid_limits_reject_before_reading() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        for (cluster, now, age, capacity, limit) in [
            ("", 1, 1, 1, 1),
            ("cluster", -1, 1, 1, 1),
            ("cluster", 1, 0, 1, 1),
            ("cluster", 1, 1, 0, 1),
            ("cluster", 1, 1, 1, 0),
            ("cluster", 1, 1, usize::MAX, 1),
            ("cluster", 1, 1, 1, usize::MAX),
        ] {
            assert_eq!(
                store.health(cluster, now, age, capacity, limit),
                Err(HealthError::InvalidInput)
            );
        }
        store
            .db
            .execute_batch("DROP TABLE delivery_attempts;")
            .unwrap();
        assert_eq!(
            store.health("cluster", 100, 50, 10, 10),
            Err(HealthError::Database)
        );
        assert_eq!(
            format!(
                "{:?}",
                store.health("cluster", 100, 50, 10, 10).unwrap_err()
            ),
            "Database"
        );
    }

    #[test]
    fn retention_cleanup_survives_reopen_without_replaying_events() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let plan = store.retention_plan("cluster", 100, 50, 1).unwrap();
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 0, &plan),
            Err(StorageError::InvalidInput)
        ));
        assert!(matches!(
            store.apply_retention("other", 100, 50, 1, &plan),
            Err(StorageError::Conflict)
        ));
        assert!(matches!(
            store.apply_retention("cluster", 70, 50, 1, &plan),
            Err(StorageError::Conflict)
        ));
        assert_eq!(
            store.apply_retention("cluster", 100, 50, 1, &plan).unwrap(),
            1
        );
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 1, &plan),
            Err(StorageError::Conflict)
        ));
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        assert_eq!(
            store.retention_plan("cluster", 100, 50, 10).unwrap().len(),
            1
        );
        assert_eq!(store.due_deliveries("cluster", 100, 10).unwrap().len(), 1);
        let (mut reconciler, checkpoint) = store.restore("cluster", 10).unwrap();
        let events = reconciler
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        assert!(events.is_empty());
        store
            .persist_reconciler(
                &reconciler,
                &events,
                &["done", "new"],
                "cluster",
                checkpoint.as_deref().unwrap(),
                100,
            )
            .unwrap();
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT count(*) FROM outbox WHERE destination IN ('done','new')",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .db
                .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                .optional()
                .unwrap(),
            None
        );
    }

    #[test]
    fn stale_later_candidate_rolls_back_earlier_deletions() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let plan = store.retention_plan("cluster", 100, 50, 10).unwrap();
        store
            .db
            .execute(
                "UPDATE outbox SET status='pending' WHERE destination='failed'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 10, &plan),
            Err(StorageError::Conflict)
        ));
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT count(*) FROM delivery_attempts WHERE destination='done'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
        store
            .db
            .execute(
                "UPDATE outbox SET status='failed' WHERE destination='failed'",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "UPDATE delivery_attempts SET attempted_at=31 WHERE destination='failed'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 10, &plan),
            Err(StorageError::Conflict)
        ));
        store
            .db
            .execute(
                "UPDATE delivery_attempts SET attempted_at=30 WHERE destination='failed'",
                [],
            )
            .unwrap();
        store
            .db
            .execute(
                "UPDATE delivery_attempts SET attempt_number=2 WHERE destination='failed'",
                [],
            )
            .unwrap();
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 10, &plan),
            Err(StorageError::Conflict)
        ));
    }

    #[test]
    fn delete_failure_restores_attempts_and_all_outbox_rows() {
        let directory = Directory::new();
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        retention_fixture(&mut store);
        let plan = store.retention_plan("cluster", 100, 50, 10).unwrap();
        store.db.execute_batch("CREATE TRIGGER reject_cleanup BEFORE DELETE ON outbox WHEN OLD.destination='failed' BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
        assert!(matches!(
            store.apply_retention("cluster", 100, 50, 10, &plan),
            Err(StorageError::Database(_))
        ));
        drop(store);
        let mut store = Store::open(&directory.path(), Duration::from_millis(50)).unwrap();
        assert_eq!(store.retention_plan("cluster", 100, 50, 10).unwrap(), plan);
        store
            .db
            .execute_batch("DROP TRIGGER reject_cleanup;")
            .unwrap();
        assert_eq!(
            store
                .apply_retention("cluster", 100, 50, 10, &plan)
                .unwrap(),
            2
        );
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
        store.db.pragma_update(None, "user_version", 5).unwrap();
        drop(store);
        assert!(matches!(
            Store::open(&directory.path(), Duration::from_millis(50)),
            Err(StorageError::UnsupportedVersion)
        ));
        let db = Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            5
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

    fn atomic_candidate() -> (
        super::super::lifecycle::Reconciler,
        Vec<super::super::lifecycle::Event>,
    ) {
        let mut candidate = super::super::lifecycle::Reconciler::with_capacity(10);
        let events = candidate
            .observe_snapshot(
                "",
                "cluster|1|alice|s|actual|COMPLETED|0:0\ncluster|2|alice|s|actual|COMPLETED|0:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        (candidate, events)
    }
    fn atomic_plan<'a>(
        candidate: &'a super::super::lifecycle::Reconciler,
        events: &'a [super::super::lifecycle::Event],
        messages: &'a [SmtpPreparation<'a>],
    ) -> PollPreparation<'a> {
        PollPreparation {
            candidate,
            deliverable: events,
            destinations: &["plain", "smtp"],
            smtp_destinations: &["smtp"],
            messages,
        }
    }

    fn provider(name: &str, relay: &str) -> super::super::smtp_provider_revision::Descriptor {
        use super::super::smtp_provider_revision::{Descriptor, Profile, TlsMode};
        Descriptor::new(&Profile {
            destination: name,
            relay,
            port: 587,
            tls: TlsMode::RequiredStartTls,
            server_name: "smtp.fixture.invalid",
            trust_reference: "fixture-trust",
            trust_fingerprint: &"a".repeat(64),
            authentication_identity: "fixture-user",
            credential_reference: "fixture-secret",
        })
        .unwrap()
    }
    fn provider_rendering() -> super::super::smtp_revision::Descriptor {
        let routes = [super::super::smtp_routes::Route {
            name: "smtp",
            visibility: super::super::smtp_render::JobVisibility::Hidden,
            metadata: super::super::smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace: "fixture",
            },
        }];
        let plan = super::super::smtp_routes::Plan::new(&["smtp"], &routes, 10).unwrap();
        super::super::smtp_revision::Descriptor::from_revision_plan(&plan, "smtp").unwrap()
    }
    fn provider_messages(
        events: &[super::super::lifecycle::Event],
        descriptor: &super::super::smtp_revision::Descriptor,
    ) -> Vec<super::super::smtp_message::Prepared> {
        events
            .iter()
            .map(|event| {
                let selected = super::super::smtp_metadata::select_revision(
                    event,
                    "smtp",
                    "fixture",
                    10,
                    descriptor.token(),
                )
                .unwrap();
                super::super::smtp_render::prepare(
                    event,
                    &super::super::smtp_render::Metadata {
                        sender: "s@fixture.invalid",
                        recipient: "r@fixture.invalid",
                        identity: &selected.identity,
                        timestamp: selected.timestamp,
                    },
                    super::super::smtp_render::JobVisibility::Hidden,
                )
                .unwrap()
            })
            .collect()
    }
    #[test]
    fn atomic_provider_profiles_reopen_and_changes_apply_only_to_new_events() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (mut candidate, events) = atomic_candidate();
        let rendering = [provider_rendering()];
        let providers = [provider("smtp", "relay.fixture.invalid")];
        let owned = provider_messages(&events, &rendering[0]);
        let messages: Vec<_> = events
            .iter()
            .zip(&owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &events, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &providers,
                },
                "cluster",
                "window-1",
                10,
            )
            .unwrap();
        assert_eq!(count(&store, "smtp_provider_revisions"), 1);
        assert_eq!(count(&store, "smtp_provider_bindings"), 2);
        assert!(store
            .bound_smtp_provider(&event_id(&events[0]), "plain")
            .unwrap()
            .is_none());
        drop(store);
        let mut store = open(&directory);
        for event in &events {
            let provider = store
                .bound_smtp_provider(&event_id(event), "smtp")
                .unwrap()
                .unwrap();
            assert_eq!(provider.token(), providers[0].token());
            assert!(provider.matches(&providers[0].profile()).unwrap());
        }
        let added = candidate
            .observe_snapshot(
                "",
                "cluster|3|alice|s|actual|FAILED|1:0",
                "cluster",
                "alice",
                true,
            )
            .unwrap();
        assert_eq!(added.len(), 1);
        let changed = [provider("smtp", "changed.fixture.invalid")];
        let new_owned = provider_messages(&added, &rendering[0]);
        let messages: Vec<_> = added
            .iter()
            .zip(&new_owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &added, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &changed,
                },
                "cluster",
                "window-2",
                20,
            )
            .unwrap();
        assert_eq!(count(&store, "smtp_provider_revisions"), 2);
        assert_eq!(count(&store, "smtp_provider_bindings"), 3);
        assert_eq!(
            store
                .bound_smtp_provider(&event_id(&added[0]), "smtp")
                .unwrap()
                .unwrap()
                .token(),
            changed[0].token()
        );
        for (event, message) in events.iter().zip(&owned) {
            assert_eq!(
                store
                    .bound_smtp_provider(&event_id(event), "smtp")
                    .unwrap()
                    .unwrap()
                    .token(),
                providers[0].token()
            );
            assert_eq!(
                store
                    .frozen_smtp(&event_id(event), "smtp")
                    .unwrap()
                    .unwrap()
                    .data(),
                message.data()
            );
        }
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &[], &[]),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &changed,
                },
                "cluster",
                "window-3",
                30,
            )
            .unwrap();
        assert_eq!(count(&store, "smtp_provider_revisions"), 2);
    }
    #[test]
    fn provider_preparation_requires_complete_unique_matching_routes() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let rendering = [provider_rendering()];
        let owned = provider_messages(&events, &rendering[0]);
        let messages: Vec<_> = events
            .iter()
            .zip(&owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        for providers in [
            vec![],
            vec![provider("other", "relay.fixture.invalid")],
            vec![
                provider("smtp", "relay.fixture.invalid"),
                provider("smtp", "relay.fixture.invalid"),
            ],
        ] {
            assert!(matches!(
                store.persist_prepared_profiles(
                    atomic_plan(&candidate, &events, &messages),
                    SmtpRevisions {
                        rendering: &rendering,
                        providers: &providers
                    },
                    "cluster",
                    "window",
                    10
                ),
                Err(StorageError::InvalidInput)
            ));
        }
        assert_eq!(count(&store, "jobs"), 0);
        assert_eq!(count(&store, "smtp_provider_revisions"), 0);
    }
    #[test]
    fn provider_binding_and_checkpoint_failures_roll_back_entire_preparation() {
        for table in ["smtp_provider_bindings", "checkpoints"] {
            let directory = Directory::new();
            let mut store = open(&directory);
            let (candidate, events) = atomic_candidate();
            let rendering = [provider_rendering()];
            let providers = [provider("smtp", "relay.fixture.invalid")];
            let owned = provider_messages(&events, &rendering[0]);
            let messages: Vec<_> = events
                .iter()
                .zip(&owned)
                .map(|(event, message)| SmtpPreparation {
                    event,
                    destination: "smtp",
                    message,
                })
                .collect();
            store.db.execute_batch(&format!("CREATE TRIGGER reject_provider BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'fixture'); END;")).unwrap();
            assert!(matches!(
                store.persist_prepared_profiles(
                    atomic_plan(&candidate, &events, &messages),
                    SmtpRevisions {
                        rendering: &rendering,
                        providers: &providers
                    },
                    "cluster",
                    "window",
                    10
                ),
                Err(StorageError::Database(_))
            ));
            for table in [
                "jobs",
                "events",
                "outbox",
                "smtp_messages",
                "smtp_render_revisions",
                "smtp_revision_bindings",
                "smtp_provider_revisions",
                "smtp_provider_bindings",
                "checkpoints",
            ] {
                assert_eq!(count(&store, table), 0);
            }
            drop(store);
            let mut store = open(&directory);
            assert!(store.restore("cluster", 10).unwrap().1.is_none());
            store
                .db
                .execute_batch("DROP TRIGGER reject_provider;")
                .unwrap();
            store
                .persist_prepared_profiles(
                    atomic_plan(&candidate, &events, &messages),
                    SmtpRevisions {
                        rendering: &rendering,
                        providers: &providers,
                    },
                    "cluster",
                    "window",
                    20,
                )
                .unwrap();
            assert_eq!(count(&store, "smtp_provider_bindings"), 2);
        }
    }
    #[test]
    fn provider_conflicts_and_malformed_or_missing_references_fail_closed() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let rendering = [provider_rendering()];
        let providers = [provider("smtp", "relay.fixture.invalid")];
        let owned = provider_messages(&events, &rendering[0]);
        let messages: Vec<_> = events
            .iter()
            .zip(&owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        store
            .db
            .execute(
                "INSERT INTO smtp_provider_revisions VALUES ('cluster','smtp',?1,?2)",
                params![providers[0].token(), b"conflict".as_slice()],
            )
            .unwrap();
        assert!(matches!(
            store.persist_prepared_profiles(
                atomic_plan(&candidate, &events, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &providers
                },
                "cluster",
                "window",
                10
            ),
            Err(StorageError::Conflict)
        ));
        assert_eq!(count(&store, "events"), 0);
        assert_eq!(count(&store, "smtp_revision_bindings"), 0);
        store
            .db
            .execute("DELETE FROM smtp_provider_revisions", [])
            .unwrap();
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &events, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &providers,
                },
                "cluster",
                "window",
                10,
            )
            .unwrap();
        store
            .db
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        for bytes in [
            vec![0; 4097],
            b"corrupt".to_vec(),
            provider("smtp", "other.fixture.invalid")
                .canonical()
                .to_vec(),
        ] {
            store
                .db
                .execute("UPDATE smtp_provider_revisions SET descriptor=?1", [bytes])
                .unwrap();
            assert!(matches!(
                store.bound_smtp_provider(&event_id(&events[0]), "smtp"),
                Err(StorageError::InvalidInput)
            ));
        }
        store
            .db
            .execute_batch("PRAGMA foreign_keys=OFF; DELETE FROM smtp_provider_revisions;")
            .unwrap();
        assert!(matches!(
            store.bound_smtp_provider(&event_id(&events[0]), "smtp"),
            Err(StorageError::InvalidInput)
        ));
    }
    #[test]
    fn provider_foreign_keys_enforce_cluster_scope_and_completed_cleanup_cascades() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let rendering = [provider_rendering()];
        let providers = [provider("smtp", "relay.fixture.invalid")];
        let owned = provider_messages(&events, &rendering[0]);
        let messages: Vec<_> = events
            .iter()
            .zip(&owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &events, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &providers,
                },
                "cluster",
                "window",
                10,
            )
            .unwrap();
        assert!(store
            .db
            .execute("DELETE FROM smtp_provider_revisions", [])
            .is_err());
        assert!(store
            .db
            .execute("UPDATE smtp_provider_bindings SET cluster='other'", [])
            .is_err());
        assert!(store
            .db
            .execute(
                "UPDATE smtp_provider_bindings SET revision=?1",
                ["a".repeat(64)]
            )
            .is_err());
        for item in store.due_deliveries("cluster", 10, 10).unwrap() {
            store
                .record_delivery(&item, DeliveryOutcome::Success, 10, None)
                .unwrap();
        }
        let cleanup = store.retention_plan("cluster", 30, 5, 10).unwrap();
        store
            .apply_retention("cluster", 30, 5, 10, &cleanup)
            .unwrap();
        assert_eq!(count(&store, "smtp_provider_bindings"), 0);
        assert_eq!(count(&store, "smtp_revision_bindings"), 0);
        assert_eq!(count(&store, "smtp_messages"), 0);
        assert_eq!(count(&store, "smtp_provider_revisions"), 1);
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 1).unwrap(),
            1
        );
        drop(store);
        let mut store = open(&directory);
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 256).unwrap(),
            0
        );
        assert_eq!(count(&store, "smtp_provider_revisions"), 0);
        assert_eq!(count(&store, "events"), 2);
    }
    #[test]
    fn provider_upgrade_preserves_legacy_and_render_only_snapshots_without_backfill() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let rendering = revision("fixture");
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &smtp_message("legacy"), &rendering)
            .unwrap();
        store.db.execute_batch("DROP TABLE smtp_provider_bindings; DROP TABLE smtp_provider_revisions; DROP INDEX smtp_render_binding_scope; PRAGMA user_version=3; CREATE TABLE smtp_provider_bindings(blocker INTEGER);").unwrap();
        drop(store);
        assert!(matches!(
            Store::open(&directory.path(), Duration::from_millis(50)),
            Err(StorageError::Database(_))
        ));
        let db = Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('smtp_provider_revisions','smtp_render_binding_scope')",[],|row|row.get::<_,i64>(0)).unwrap(),0);
        db.execute_batch("DROP TABLE smtp_provider_bindings;")
            .unwrap();
        drop(db);
        let mut store = open(&directory);
        assert_eq!(
            store
                .db
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert!(store
            .bound_smtp_provider(&item.event_id, "smtp")
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .data(),
            smtp_message("legacy").data()
        );
        let (candidate, _) = store.restore("cluster", 10).unwrap();
        let rendering = [provider_rendering()];
        let providers = [provider("smtp", "relay.fixture.invalid")];
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &[], &[]),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: &providers,
                },
                "cluster",
                "zz-window",
                20,
            )
            .unwrap();
        assert!(store
            .bound_smtp_provider(&item.event_id, "smtp")
            .unwrap()
            .is_none());
        assert_eq!(count(&store, "smtp_provider_revisions"), 0);
        store
            .db
            .execute("DELETE FROM smtp_revision_bindings", [])
            .unwrap();
        assert!(store
            .bound_smtp_provider(&item.event_id, "smtp")
            .unwrap()
            .is_none());
        assert_eq!(count(&store, "smtp_messages"), 1);
    }

    #[test]
    fn atomic_smtp_poll_commits_mixed_routes_and_preserves_replay_after_reopen() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let first = smtp_message("first\n.\n.original");
        let second = smtp_message("second");
        let other = super::super::smtp_message::prepare(
            "s@fixture.invalid",
            "other@fixture.invalid",
            "other",
            "alternate",
            "other route",
            0,
        )
        .unwrap();
        let messages = [
            SmtpPreparation {
                event: &events[0],
                destination: "smtp",
                message: &first,
            },
            SmtpPreparation {
                event: &events[1],
                destination: "smtp",
                message: &second,
            },
            SmtpPreparation {
                event: &events[0],
                destination: "smtp-two",
                message: &other,
            },
            SmtpPreparation {
                event: &events[1],
                destination: "smtp-two",
                message: &other,
            },
        ];
        let mut initial = atomic_plan(&candidate, &events, &messages);
        initial.destinations = &["plain", "smtp", "smtp-two"];
        initial.smtp_destinations = &["smtp", "smtp-two"];
        store
            .persist_prepared_reconciler(initial, "cluster", "window-1", 10)
            .unwrap();
        for (table, expected) in [
            ("jobs", 2),
            ("events", 2),
            ("outbox", 6),
            ("smtp_messages", 4),
            ("checkpoints", 1),
        ] {
            assert_eq!(count(&store, table), expected);
        }
        let item = store
            .due_deliveries("cluster", 10, 10)
            .unwrap()
            .into_iter()
            .find(|item| item.destination == "smtp" && item.event == events[0])
            .unwrap();
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 20, Some(25))
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        for (event, expected) in [(&events[0], &first), (&events[1], &second)] {
            let frozen = store
                .frozen_smtp(&event_id(event), "smtp")
                .unwrap()
                .unwrap();
            assert!(frozen.data() == expected.data());
            assert!(frozen.envelope() == ("s@fixture.invalid", "r@fixture.invalid"));
            let alternate = store
                .frozen_smtp(&event_id(event), "smtp-two")
                .unwrap()
                .unwrap();
            assert!(alternate.data() == other.data());
            assert!(alternate.envelope() == ("s@fixture.invalid", "other@fixture.invalid"));
            assert!(store
                .frozen_smtp(&event_id(event), "plain")
                .unwrap()
                .is_none());
        }
        let (restored, checkpoint) = store.restore("cluster", 10).unwrap();
        assert_eq!(restored.emitted, candidate.emitted);
        assert_eq!(checkpoint.as_deref(), Some("window-1"));
        let changed = smtp_message("changed");
        let replacements = [SmtpPreparation {
            event: &events[0],
            destination: "smtp",
            message: &changed,
        }];
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&candidate, &events, &replacements),
                "cluster",
                "window-2",
                30
            ),
            Err(StorageError::InvalidInput)
        ));
        // Replaying a recorded event needs no message and never changes its routes.
        let mut replay = atomic_plan(&candidate, &events, &[]);
        replay.destinations = &["new-smtp"];
        replay.smtp_destinations = &["new-smtp"];
        store
            .persist_prepared_reconciler(replay, "cluster", "window-2", 30)
            .unwrap();
        assert_eq!(count(&store, "outbox"), 6);
        assert_eq!(count(&store, "smtp_messages"), 4);
        let retry = store
            .due_deliveries("cluster", 25, 10)
            .unwrap()
            .into_iter()
            .find(|row| row.event_id == item.event_id && row.destination == "smtp")
            .unwrap();
        assert_eq!(retry.attempt_number, 2);
        assert!(
            store
                .frozen_smtp(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .data()
                == first.data()
        );
        assert_eq!(
            store
                .db
                .query_row(
                    "SELECT next_attempt_at FROM outbox WHERE event_id=?1 AND destination='smtp'",
                    [&item.event_id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            25
        );
    }

    #[test]
    fn atomic_smtp_missing_extra_duplicate_and_invalid_routes_leave_no_state() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let message = smtp_message("original");
        let pair = |event, destination| SmtpPreparation {
            event,
            destination,
            message: &message,
        };
        for case in 0..9 {
            let mut messages = vec![pair(&events[0], "smtp"), pair(&events[1], "smtp")];
            match case {
                0 => messages.clear(),
                1 => {
                    messages.pop();
                }
                2 => messages.push(pair(&events[0], "smtp")),
                3 => messages[0].destination = "plain",
                _ => (),
            }
            let mut plan = atomic_plan(&candidate, &events, &messages);
            match case {
                4 => plan.destinations = &["smtp", "smtp"],
                5 => plan.destinations = &[""],
                6 => plan.smtp_destinations = &["absent"],
                7 => plan.smtp_destinations = &["smtp", "smtp"],
                8 => {
                    plan.destinations = &["smtp", "smtp-two"];
                    plan.smtp_destinations = &["smtp", "smtp-two"];
                }
                _ => (),
            }
            assert!(matches!(
                store.persist_prepared_reconciler(plan, "cluster", "window", 10),
                Err(StorageError::InvalidInput)
            ));
            for table in ["jobs", "events", "outbox", "smtp_messages", "checkpoints"] {
                assert_eq!(count(&store, table), 0);
            }
        }
        let empty = super::super::lifecycle::Reconciler::with_capacity(10);
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&empty, &events, &[]),
                "cluster",
                "window",
                10
            ),
            Err(StorageError::InvalidInput)
        ));
        let duplicate = [events[0].clone(), events[0].clone()];
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&candidate, &duplicate, &[]),
                "cluster",
                "window",
                10
            ),
            Err(StorageError::InvalidInput)
        ));
        let extra = [pair(&events[0], "smtp")];
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&candidate, &[], &extra),
                "cluster",
                "window",
                10
            ),
            Err(StorageError::InvalidInput)
        ));
    }

    #[test]
    fn atomic_smtp_late_failures_preserve_previous_poll_and_allow_restart_retry() {
        for table in ["smtp_messages", "checkpoints"] {
            let directory = Directory::new();
            let mut store = open(&directory);
            let mut baseline = super::super::lifecycle::Reconciler::with_capacity(10);
            let suppressed = baseline
                .observe_snapshot("1|alice|s|estimate|PENDING", "", "cluster", "alice", false)
                .unwrap();
            store
                .persist_reconciler(&baseline, &suppressed, &["plain"], "cluster", "window-0", 9)
                .unwrap();
            let (candidate, events) = atomic_candidate();
            let message = smtp_message("original");
            let messages = events
                .iter()
                .map(|event| SmtpPreparation {
                    event,
                    destination: "smtp",
                    message: &message,
                })
                .collect::<Vec<_>>();
            let condition = if table == "smtp_messages" {
                "WHEN (SELECT COUNT(*) FROM smtp_messages)>0"
            } else {
                ""
            };
            store.db.execute_batch(&format!("CREATE TRIGGER stop_preparation BEFORE INSERT ON {table} {condition} BEGIN SELECT RAISE(ABORT,'private-fixture'); END;")).unwrap();
            assert!(matches!(
                store.persist_prepared_reconciler(
                    atomic_plan(&candidate, &events, &messages),
                    "cluster",
                    "window-1",
                    10
                ),
                Err(StorageError::Database(_))
            ));
            drop(store);
            let mut store = open(&directory);
            assert_eq!(count(&store, "jobs"), 1);
            for table in ["events", "outbox", "smtp_messages", "delivery_attempts"] {
                assert_eq!(count(&store, table), 0);
            }
            let (restored, checkpoint) = store.restore("cluster", 10).unwrap();
            assert_eq!(restored.latest, baseline.latest);
            assert_eq!(checkpoint.as_deref(), Some("window-0"));
            store
                .db
                .execute_batch("DROP TRIGGER stop_preparation")
                .unwrap();
            store
                .persist_prepared_reconciler(
                    atomic_plan(&candidate, &events, &messages),
                    "cluster",
                    "window-1",
                    10,
                )
                .unwrap();
            assert_eq!(count(&store, "smtp_messages"), 2);
            assert_eq!(count(&store, "outbox"), 4);
            assert_eq!(
                store.restore("cluster", 10).unwrap().1.as_deref(),
                Some("window-1")
            );
        }
    }

    #[test]
    fn atomic_smtp_does_not_backfill_legacy_events_or_regress_checkpoint() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        store
            .persist_reconciler(&candidate, &events, &["smtp"], "cluster", "window-1", 10)
            .unwrap();
        let message = smtp_message("original");
        let messages = events
            .iter()
            .map(|event| SmtpPreparation {
                event,
                destination: "smtp",
                message: &message,
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&candidate, &events, &messages),
                "cluster",
                "window-2",
                20
            ),
            Err(StorageError::InvalidInput)
        ));
        assert!(matches!(
            store.persist_prepared_reconciler(
                atomic_plan(&candidate, &events, &[]),
                "cluster",
                "window-0",
                20
            ),
            Err(StorageError::Conflict)
        ));
        store
            .persist_prepared_reconciler(
                atomic_plan(&candidate, &events, &[]),
                "cluster",
                "window-2",
                20,
            )
            .unwrap();
        assert_eq!(count(&store, "smtp_messages"), 0);
        assert_eq!(count(&store, "outbox"), 2);
        assert_eq!(
            store.restore("cluster", 10).unwrap().1.as_deref(),
            Some("window-2")
        );
    }

    fn smtp_seed(store: &mut Store) -> DueDelivery {
        let mut reconciler = super::super::lifecycle::Reconciler::with_capacity(10);
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
                &["smtp"],
                "cluster",
                "2026-10-10T01:00:00",
                10,
            )
            .unwrap();
        store.due_deliveries("cluster", 10, 10).unwrap().remove(0)
    }
    fn smtp_message(body: &str) -> super::super::smtp_message::Prepared {
        super::super::smtp_message::prepare(
            "s@fixture.invalid",
            "r@fixture.invalid",
            "event_1",
            "done",
            body,
            0,
        )
        .unwrap()
    }
    fn revision(namespace: &str) -> super::super::smtp_revision::Descriptor {
        let routes = [super::super::smtp_routes::Route {
            name: "smtp",
            visibility: super::super::smtp_render::JobVisibility::Hidden,
            metadata: super::super::smtp_routes::Metadata {
                sender: "s@fixture.invalid",
                recipient: "r@fixture.invalid",
                namespace,
            },
        }];
        let plan = super::super::smtp_routes::Plan::new(&["smtp"], &routes, 1).unwrap();
        super::super::smtp_revision::Descriptor::from_plan(&plan, "smtp").unwrap()
    }
    #[test]
    fn revisions_reopen_immutable_and_reject_legacy_backfill() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let original = revision("original");
        let message = smtp_message("done");
        assert!(matches!(
            store.freeze_smtp_revision(&item.event_id, "other", &message, &original),
            Err(StorageError::InvalidInput)
        ));
        assert!(matches!(
            store.freeze_smtp_revision("missing", "smtp", &message, &original),
            Err(StorageError::Conflict)
        ));
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &message, &original)
            .unwrap();
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 20, Some(25))
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        assert_eq!(
            store
                .bound_smtp_revision(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .token(),
            original.token()
        );
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &message, &original)
            .unwrap();
        for (message, revision) in [
            (smtp_message("changed"), revision("original")),
            (smtp_message("done"), revision("changed")),
        ] {
            assert!(matches!(
                store.freeze_smtp_revision(&item.event_id, "smtp", &message, &revision),
                Err(StorageError::Conflict)
            ));
        }
        assert_eq!(count(&store, "smtp_render_revisions"), 1);
        assert_eq!(count(&store, "smtp_revision_bindings"), 1);
        store
            .db
            .execute("DELETE FROM smtp_revision_bindings", [])
            .unwrap();
        assert!(store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .is_none());
        assert!(matches!(
            store.freeze_smtp_revision(&item.event_id, "smtp", &message, &original),
            Err(StorageError::Conflict)
        ));
    }
    #[test]
    fn revision_late_failure_rolls_back_and_foreign_keys_protect_pending_work() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let descriptor = revision("fixture");
        store.db.execute_batch("CREATE TRIGGER reject_binding BEFORE INSERT ON smtp_revision_bindings BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(matches!(
            store.freeze_smtp_revision(&item.event_id, "smtp", &smtp_message("done"), &descriptor),
            Err(StorageError::Database(_))
        ));
        for table in [
            "smtp_messages",
            "smtp_render_revisions",
            "smtp_revision_bindings",
        ] {
            assert_eq!(count(&store, table), 0);
        }
        assert_eq!(count(&store, "outbox"), 1);
        store
            .db
            .execute_batch("DROP TRIGGER reject_binding;")
            .unwrap();
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &smtp_message("done"), &descriptor)
            .unwrap();
        assert!(store
            .db
            .execute("DELETE FROM smtp_render_revisions", [])
            .is_err());
        store
            .db
            .execute("DELETE FROM smtp_revision_bindings", [])
            .unwrap();
        assert!(store
            .db
            .execute(
                "INSERT INTO smtp_revision_bindings VALUES (?1,'smtp','wrong',?2)",
                params![item.event_id, descriptor.token()]
            )
            .is_err());
        assert!(store
            .db
            .execute(
                "INSERT INTO smtp_revision_bindings VALUES (?1,'smtp','cluster',?2)",
                params![item.event_id, "0".repeat(64)]
            )
            .is_err());
        store
            .db
            .execute(
                "INSERT INTO smtp_revision_bindings VALUES (?1,'smtp','cluster',?2)",
                params![item.event_id, descriptor.token()],
            )
            .unwrap();
        store.db.execute("DELETE FROM outbox", []).unwrap();
        assert_eq!(count(&store, "smtp_revision_bindings"), 0);
        assert_eq!(count(&store, "smtp_messages"), 0);
        assert_eq!(count(&store, "smtp_render_revisions"), 1);
    }
    #[test]
    fn revision_upgrade_preserves_legacy_and_failed_migration_recovers() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        store
            .freeze_smtp(&item.event_id, "smtp", &smtp_message("legacy"))
            .unwrap();
        store.db.execute_batch("DROP TABLE smtp_provider_bindings; DROP TABLE smtp_provider_revisions; DROP TABLE smtp_revision_bindings; DROP TABLE smtp_render_revisions; PRAGMA user_version=2; CREATE TABLE smtp_revision_bindings(blocker INTEGER);").unwrap();
        drop(store);
        assert!(Store::open(&directory.path(), Duration::from_millis(50)).is_err());
        let db = Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='smtp_render_revisions'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        db.execute_batch("DROP TABLE smtp_revision_bindings;")
            .unwrap();
        drop(db);
        let mut store = open(&directory);
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .data(),
            smtp_message("legacy").data()
        );
        assert!(store
            .bound_smtp_revision(&item.event_id, "smtp")
            .unwrap()
            .is_none());
        assert!(matches!(
            store.freeze_smtp_revision(
                &item.event_id,
                "smtp",
                &smtp_message("legacy"),
                &revision("fixture")
            ),
            Err(StorageError::Conflict)
        ));
        assert_eq!(count(&store, "smtp_render_revisions"), 0);
    }
    #[test]
    fn revision_cleanup_is_bounded_cluster_scoped_and_rolls_back_late_failure() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let bound = revision("bound");
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &smtp_message("done"), &bound)
            .unwrap();
        for namespace in ["orphan-a", "orphan-b"] {
            let descriptor = revision(namespace);
            store
                .db
                .execute(
                    "INSERT INTO smtp_render_revisions VALUES ('cluster','smtp',?1,?2)",
                    params![descriptor.token(), descriptor.canonical()],
                )
                .unwrap();
        }
        store
            .db
            .execute(
                "INSERT INTO smtp_render_revisions VALUES ('other','smtp',?1,?2)",
                params![bound.token(), bound.canonical()],
            )
            .unwrap();
        for (cluster, limit) in [("", 1), ("cluster", 0), ("cluster", 257)] {
            assert!(matches!(
                store.prune_smtp_revisions(cluster, limit),
                Err(StorageError::InvalidInput)
            ));
        }
        store.db.execute_batch("CREATE TABLE cleanup_probe(n INTEGER); CREATE TRIGGER reject_second_revision BEFORE DELETE ON smtp_render_revisions BEGIN INSERT INTO cleanup_probe VALUES(1); SELECT CASE WHEN (SELECT count(*) FROM cleanup_probe)=2 THEN RAISE(ABORT,'fixture') END; END;").unwrap();
        assert!(matches!(
            store.prune_smtp_revisions("cluster", 2),
            Err(StorageError::Database(_))
        ));
        assert_eq!(count(&store, "smtp_render_revisions"), 4);
        assert_eq!(count(&store, "cleanup_probe"), 0);
        store
            .db
            .execute_batch("DROP TRIGGER reject_second_revision;")
            .unwrap();
        assert_eq!(store.prune_smtp_revisions("cluster", 1).unwrap(), 1);
        assert_eq!(count(&store, "smtp_render_revisions"), 3);
        assert_eq!(store.prune_smtp_revisions("cluster", 256).unwrap(), 1);
        assert_eq!(store.prune_smtp_revisions("cluster", 1).unwrap(), 0);
        assert_eq!(
            store
                .bound_smtp_revision(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .token(),
            bound.token()
        );
        assert_eq!(store.prune_smtp_revisions("other", 1).unwrap(), 1);
        drop(store);
        let mut store = open(&directory);
        assert_eq!(count(&store, "smtp_render_revisions"), 1);
        assert_eq!(store.prune_smtp_revisions("cluster", 1).unwrap(), 0);
        assert_eq!(store.due_deliveries("cluster", 10, 10).unwrap().len(), 1);
    }
    #[test]
    fn provider_cleanup_is_bounded_cluster_scoped_and_rolls_back_late_failure() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let (candidate, events) = atomic_candidate();
        let rendering = [provider_rendering()];
        let bound = provider("smtp", "bound.fixture.invalid");
        let owned = provider_messages(&events, &rendering[0]);
        let messages: Vec<_> = events
            .iter()
            .zip(&owned)
            .map(|(event, message)| SmtpPreparation {
                event,
                destination: "smtp",
                message,
            })
            .collect();
        store
            .persist_prepared_profiles(
                atomic_plan(&candidate, &events, &messages),
                SmtpRevisions {
                    rendering: &rendering,
                    providers: std::slice::from_ref(&bound),
                },
                "cluster",
                "window",
                10,
            )
            .unwrap();
        let item = store.due_deliveries("cluster", 10, 10).unwrap().remove(0);
        for namespace in ["orphan-a", "orphan-b"] {
            let descriptor = provider("smtp", namespace);
            store
                .db
                .execute(
                    "INSERT INTO smtp_provider_revisions VALUES ('cluster','smtp',?1,?2)",
                    params![descriptor.token(), descriptor.canonical()],
                )
                .unwrap();
        }
        store
            .db
            .execute(
                "INSERT INTO smtp_provider_revisions VALUES ('other','smtp',?1,?2)",
                params![bound.token(), bound.canonical()],
            )
            .unwrap();
        for (cluster, limit) in [
            ("", 1),
            (&"x".repeat(4097), 1),
            ("cluster", 0),
            ("cluster", 257),
        ] {
            assert!(matches!(
                store.prune_smtp_provider_revisions(cluster, limit),
                Err(StorageError::InvalidInput)
            ));
        }
        store.db.execute_batch("CREATE TABLE cleanup_probe(n INTEGER); CREATE TRIGGER reject_second_provider BEFORE DELETE ON smtp_provider_revisions BEGIN INSERT INTO cleanup_probe VALUES(1); SELECT CASE WHEN (SELECT count(*) FROM cleanup_probe)=2 THEN RAISE(ABORT,'fixture') END; END;").unwrap();
        assert!(matches!(
            store.prune_smtp_provider_revisions("cluster", 2),
            Err(StorageError::Database(_))
        ));
        assert_eq!(count(&store, "smtp_provider_revisions"), 4);
        assert_eq!(count(&store, "cleanup_probe"), 0);
        store
            .db
            .execute_batch("DROP TRIGGER reject_second_provider;")
            .unwrap();
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 1).unwrap(),
            1
        );
        assert_eq!(count(&store, "smtp_provider_revisions"), 3);
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 256).unwrap(),
            1
        );
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 1).unwrap(),
            0
        );
        assert_eq!(
            store
                .bound_smtp_provider(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .token(),
            bound.token()
        );
        assert_eq!(store.prune_smtp_provider_revisions("other", 1).unwrap(), 1);
        drop(store);
        let mut store = open(&directory);
        assert_eq!(count(&store, "smtp_provider_revisions"), 1);
        assert_eq!(
            store.prune_smtp_provider_revisions("cluster", 1).unwrap(),
            0
        );
        assert_eq!(
            store
                .frozen_smtp(&item.event_id, "smtp")
                .unwrap()
                .unwrap()
                .data(),
            owned[0].data()
        );
        assert_eq!(count(&store, "smtp_render_revisions"), 1);
        assert_eq!(count(&store, "events"), 2);
        assert_eq!(store.due_deliveries("cluster", 10, 10).unwrap().len(), 4);
    }
    #[test]
    fn malformed_owned_revision_state_fails_closed() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let descriptor = revision("fixture");
        store
            .freeze_smtp_revision(&item.event_id, "smtp", &smtp_message("done"), &descriptor)
            .unwrap();
        store
            .db
            .execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        for bytes in [
            vec![0; 4097],
            vec![0],
            revision("changed").canonical().to_vec(),
        ] {
            store
                .db
                .execute("UPDATE smtp_render_revisions SET descriptor=?1", [bytes])
                .unwrap();
            assert!(matches!(
                store.bound_smtp_revision(&item.event_id, "smtp"),
                Err(StorageError::InvalidInput)
            ));
            assert!(matches!(
                store.freeze_smtp_revision(
                    &item.event_id,
                    "smtp",
                    &smtp_message("done"),
                    &descriptor
                ),
                Err(StorageError::Conflict)
            ));
        }
        store
            .db
            .execute_batch("PRAGMA foreign_keys=OFF; DELETE FROM smtp_render_revisions;")
            .unwrap();
        assert!(matches!(
            store.bound_smtp_revision(&item.event_id, "smtp"),
            Err(StorageError::InvalidInput)
        ));
    }
    #[test]
    fn frozen_smtp_is_immutable_and_exact_across_retry_and_reopen() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        assert!(store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .is_none());
        let message = smtp_message("done\n.\n.dot");
        store
            .freeze_smtp(&item.event_id, &item.destination, &message)
            .unwrap();
        store
            .freeze_smtp(&item.event_id, &item.destination, &message)
            .unwrap();
        assert!(matches!(
            store.freeze_smtp(&item.event_id, &item.destination, &smtp_message("changed")),
            Err(StorageError::Conflict)
        ));
        assert!(matches!(
            store.freeze_smtp(&item.event_id, "missing", &message),
            Err(StorageError::Conflict)
        ));
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 20, Some(25))
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        let frozen = store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .unwrap();
        assert!(frozen.envelope() == ("s@fixture.invalid", "r@fixture.invalid"));
        assert!(frozen.data() == message.data());
        let loaded =
            super::super::smtp_frozen_frame::load(&store, &item.event_id, &item.destination)
                .unwrap();
        let encoded =
            super::super::smtp_frozen_frame::encode(b"root", 25, "u", "p", &loaded).unwrap();
        let decoded = super::super::smtp_frozen_frame::decode_request(encoded.as_slice()).unwrap();
        assert!(decoded.message.data() == frozen.data());
        assert!(decoded.message.envelope() == message.envelope());
        assert!(super::super::smtp_frozen_frame::load(&store, &item.event_id, "missing").is_err());
        for (root, port, username, password) in [
            (b"".as_slice(), 25, "u", "p"),
            (b"root", 0, "u", "p"),
            (b"root", 25, "bad\0", "p"),
            (b"root", 25, "", "p"),
            (b"root", 25, "u", ""),
        ] {
            assert!(super::super::smtp_frozen_frame::encode(
                root, port, username, password, &loaded
            )
            .is_err());
        }
        assert!(
            super::super::smtp_frozen_frame::encode(&vec![0; 4097], 25, "u", "p", &loaded).is_err()
        );

        let retry = store.due_deliveries("cluster", 25, 10).unwrap().remove(0);
        assert_eq!(retry.event_id, item.event_id);
        assert_eq!(retry.attempt_number, 2);
        store
            .freeze_smtp(&item.event_id, &item.destination, &message)
            .unwrap();
        assert!(matches!(
            store.freeze_smtp(
                &item.event_id,
                &item.destination,
                &smtp_message("changed after restart")
            ),
            Err(StorageError::Conflict)
        ));
        assert_eq!(count(&store, "smtp_messages"), 1);
    }
    #[test]
    fn legacy_upgrade_preserves_rows_and_refuses_fabricating_attempted_messages() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        store
            .record_delivery(&item, DeliveryOutcome::TransientFailure, 20, Some(25))
            .unwrap();
        store
            .db
            .execute_batch("DROP TABLE smtp_provider_bindings; DROP TABLE smtp_provider_revisions; DROP TABLE smtp_revision_bindings; DROP TABLE smtp_render_revisions; DROP TABLE smtp_messages; PRAGMA user_version=1;")
            .unwrap();
        drop(store);
        let mut store = open(&directory);
        assert_eq!(count(&store, "outbox"), 1);
        assert_eq!(count(&store, "delivery_attempts"), 1);
        assert_eq!(
            store
                .db
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert!(store
            .frozen_smtp(&item.event_id, &item.destination)
            .unwrap()
            .is_none());
        assert!(matches!(
            store.freeze_smtp(&item.event_id, &item.destination, &smtp_message("new")),
            Err(StorageError::Conflict)
        ));
        drop(store);
        let store = open(&directory);
        assert_eq!(count(&store, "smtp_messages"), 0);
    }
    #[test]
    fn migration_and_freeze_failures_roll_back_without_changing_outbox() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        store.db.execute_batch("DROP TABLE smtp_provider_bindings; DROP TABLE smtp_provider_revisions; DROP TABLE smtp_revision_bindings; DROP TABLE smtp_render_revisions; DROP TABLE smtp_messages; PRAGMA user_version=1; CREATE TABLE smtp_messages(blocker INTEGER);").unwrap();
        drop(store);
        assert!(matches!(
            Store::open(&directory.path(), Duration::from_millis(50)),
            Err(StorageError::Database(_))
        ));
        let db = Connection::open(directory.path()).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        db.execute_batch("DROP TABLE smtp_messages;").unwrap();
        drop(db);
        let mut store = open(&directory);
        store.db.execute_batch("CREATE TRIGGER reject_smtp BEFORE INSERT ON smtp_messages BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(matches!(
            store.freeze_smtp(&item.event_id, &item.destination, &smtp_message("done")),
            Err(StorageError::Database(_))
        ));
        assert_eq!(count(&store, "smtp_messages"), 0);
        assert_eq!(count(&store, "outbox"), 1);
        assert_eq!(count(&store, "delivery_attempts"), 0);
        store.db.execute_batch("DROP TRIGGER reject_smtp;").unwrap();
        store
            .freeze_smtp(&item.event_id, &item.destination, &smtp_message("done"))
            .unwrap();
        store
            .db
            .execute(
                "DELETE FROM outbox WHERE event_id=?1 AND destination=?2",
                params![item.event_id, item.destination],
            )
            .unwrap();
        assert_eq!(count(&store, "smtp_messages"), 0);
    }
    #[test]
    fn frozen_reads_reject_corrupt_or_oversized_state_before_returning_bytes() {
        let directory = Directory::new();
        let mut store = open(&directory);
        let item = smtp_seed(&mut store);
        let message = smtp_message("done");
        store
            .freeze_smtp(&item.event_id, &item.destination, &message)
            .unwrap();
        for data in [
            b"bad\n.\r\n".as_slice(),
            b"line\r\n.\r\nbody\r\n.\r\n",
            b"line\r\n",
        ] {
            store
                .db
                .execute("UPDATE smtp_messages SET data=?1", params![data])
                .unwrap();
            assert!(matches!(
                store.frozen_smtp(&item.event_id, &item.destination),
                Err(StorageError::InvalidInput)
            ));
        }
        store.db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE smtp_messages SET data=zeroblob(20000);").unwrap();
        assert!(matches!(
            store.frozen_smtp(&item.event_id, &item.destination),
            Err(StorageError::InvalidInput)
        ));
        store
            .db
            .execute(
                "UPDATE smtp_messages SET data=?1,sender=?2",
                params![message.data(), "injected\r\n"],
            )
            .unwrap();
        assert!(matches!(
            store.frozen_smtp(&item.event_id, &item.destination),
            Err(StorageError::InvalidInput)
        ));
    }
}
