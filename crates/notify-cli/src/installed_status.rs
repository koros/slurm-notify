//! Installed historical status; configuration metadata only, no resource resolution.
use crate::config_decode::Value;
use crate::config_validate::{get, object, text};
use std::path::Path;

pub(super) enum Error {
    Config,
    Missing,
    Busy,
    State,
    Version,
    Database,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn inspect(path: &Path) -> Result<String, Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .ok_or(Error::Database)?;
    inspect_at(path, now)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn inspect_at(path: &Path, now: i64) -> Result<String, Error> {
    let snapshot = crate::config_snapshot::read(path).map_err(|_| Error::Config)?;
    let draft = crate::config_validate::validate(snapshot.bytes()).map_err(|_| Error::Config)?;
    let root = object(&draft.value).map_err(|_| Error::Config)?;
    let monitor =
        object(get(root, "monitor").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let storage =
        object(get(root, "storage").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let maintenance =
        object(get(root, "maintenance").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let cluster =
        text(get(monitor, "cluster_id").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let raw = text(get(storage, "path").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let number = |fields, name| -> Result<u64, Error> {
        match get(fields, name).map_err(|_| Error::Config)? {
            Value::Integer(value) => Ok(*value),
            _ => Err(Error::Config),
        }
    };
    let parent = crate::state_path::database_parent(raw).map_err(|_| Error::Config)?;
    let mut status = crate::status_store::open(
        parent,
        std::time::Duration::from_secs(number(monitor, "query_timeout_seconds")?),
    )
    .map_err(|error| match error {
        crate::status_store::Error::State(crate::state_path::Error::AlreadyOwned) => Error::Busy,
        crate::status_store::Error::State(crate::state_path::Error::Missing) => Error::Missing,
        crate::status_store::Error::State(_) => Error::State,
        crate::status_store::Error::Version => Error::Version,
        crate::status_store::Error::Database(error) => {
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
            ) {
                Error::Busy
            } else {
                Error::Database
            }
        }
    })?;
    crate::status_store::query_budget(
        &status.db,
        std::time::Duration::from_secs(number(monitor, "query_timeout_seconds")?),
    )
    .map_err(|_| Error::Database)?;
    let (health, checkpoint) = crate::storage_health::snapshot(
        &mut status.db,
        cluster,
        now,
        number(maintenance, "completed_delivery_retention_seconds")? as i64,
        (
            number(monitor, "retained_identity_limit")? as usize,
            number(monitor, "retained_event_limit")? as usize,
        ),
        number(maintenance, "health_sample_limit")? as usize,
        true,
    )
    .map_err(|_| Error::Database)?;
    let age = checkpoint
        .as_deref()
        .map(crate::state_timestamp::parse)
        .transpose()
        .map_err(|_| Error::Database)?
        .map(|checkpoint| {
            now.checked_sub(checkpoint)
                .filter(|age| *age >= 0)
                .ok_or(Error::Database)
        })
        .transpose()?;
    let stale_after = number(monitor, "stale_after_seconds")? as i64;
    let stale = age.map(|age| age >= stale_after).unwrap_or(true);
    let age = age
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into());
    let mut output = format!("Historical state: cluster={cluster}\nProcess health: unknown\nCheckpoint: age_seconds={age} stale={stale}\nQueue sample: queued={} due={} retrying={} failed={} delivered={} sampled={} truncated={}\nCapacity sample: jobs={} events={} jobs_over_limit={} events_over_limit={}\nRetention sample: cleanup_eligible={} completion_unproven={}\n", health.pending, health.due, health.retrying, health.failed, health.delivered, health.destinations_sampled, health.destinations_truncated, health.jobs_sampled, health.events_sampled, health.jobs_over_capacity, health.events_over_capacity, health.cleanup_eligible, health.completion_unproven);
    let providers =
        object(get(root, "providers").map_err(|_| Error::Config)?).map_err(|_| Error::Config)?;
    let api = providers.iter().any(|(_, p)| {
        object(p)
            .ok()
            .and_then(|p| get(p, "type").ok())
            .and_then(|v| text(v).ok())
            .is_some_and(|kind| matches!(kind, "sendgrid" | "mailgun" | "gmail_api"))
    });
    if api {
        let version: i64 = status
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(|_| Error::Database)?;
        let limit = number(maintenance, "health_sample_limit")? as i64;
        let count: i64 = if version >= 7 {
            status.db.query_row("SELECT count(*) FROM (SELECT 1 FROM email_provider_pauses WHERE cluster=?1 LIMIT ?2)",rusqlite::params![cluster,limit+1],|r|r.get(0)).map_err(|_| Error::Database)?
        } else {
            0
        };
        output.push_str(&format!(
            "API email pauses: sampled={} truncated={}\n",
            count.min(limit),
            count > limit
        ));
        let cooldowns: i64 = if version >= 8 {
            status.db.query_row("SELECT count(*) FROM (SELECT 1 FROM email_provider_cooldowns WHERE cluster=?1 AND retry_at>?2 LIMIT ?3)",rusqlite::params![cluster,now,limit+1],|r|r.get(0)).map_err(|_|Error::Database)?
        } else {
            0
        };
        output.push_str(&format!(
            "API email cooldowns: sampled={} truncated={}\n",
            cooldowns.min(limit),
            cooldowns > limit
        ));
    }
    Ok(output)
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) fn inspect(_: &Path) -> Result<String, Error> {
    Err(Error::State)
}
