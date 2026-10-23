//! Shared bounded queue/checkpoint snapshot in one read transaction.
use rusqlite::{params, Connection, OptionalExtension};
// Counts describe a bounded sample; truncation explicitly prevents exact totals.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct StorageHealth {
    pub(super) jobs_sampled: usize,
    pub(super) events_sampled: usize,
    pub(super) jobs_over_capacity: bool,
    pub(super) events_over_capacity: bool,
    pub(super) destinations_sampled: usize,
    pub(super) destinations_truncated: bool,
    pub(super) pending: usize,
    pub(super) due: usize,
    pub(super) retrying: usize,
    pub(super) delivered: usize,
    pub(super) failed: usize,
    pub(super) cleanup_eligible: usize,
    pub(super) completion_unproven: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HealthError {
    InvalidInput,
    Database,
}

pub(super) fn snapshot(
    db: &mut Connection,
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
        let tx = db.transaction()?;
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
            retrying: 0,
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
                    if outcome.as_deref() == Some("transient_failure") {
                        health.retrying += 1;
                    }
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
