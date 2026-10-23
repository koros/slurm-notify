//! Shared existing-only SQLite inspection; never creates or migrates state.
use rusqlite::Connection;
use std::{path::Path, time::Duration};
#[derive(Debug)]
pub(super) enum Error {
    State(crate::state_path::Error),
    Version,
    Database(rusqlite::Error),
}
// Field drop order closes SQLite before releasing the ownership guard.
pub(super) struct ReadStatus {
    pub(super) db: Connection,
    pub(super) _owner: crate::state_path::Guard,
}
pub(super) fn open(directory: &Path, timeout: Duration) -> Result<ReadStatus, Error> {
    let owner = crate::state_path::Guard::inspect(directory).map_err(Error::State)?;
    let db = Connection::open_with_flags(
        &owner.database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(Error::Database)?;
    db.busy_timeout(timeout).map_err(Error::Database)?;
    db.pragma_update(None, "query_only", true)
        .map_err(Error::Database)?;
    db.pragma_update(None, "temp_store", "MEMORY")
        .map_err(Error::Database)?;
    let version: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(Error::Database)?;
    if version != 4 {
        return Err(Error::Version);
    }
    Ok(ReadStatus { db, _owner: owner })
}

// Cooperatively interrupt expensive SQL using the configured query timeout.
// SQLite busy waiting is configured separately; filesystem calls have no hard deadline.
pub(super) fn query_budget(db: &Connection, timeout: Duration) -> rusqlite::Result<()> {
    let start = std::time::Instant::now();
    db.progress_handler(1000, Some(move || start.elapsed() >= timeout))
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)] // Custom process harness omits unit-test bodies.
    use super::*;
    #[test]
    fn query_budget_interrupts_expensive_read_work() {
        let db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "query_only", true).unwrap();
        query_budget(&db, Duration::ZERO).unwrap();
        let result = db.query_row(
            "WITH RECURSIVE work(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM work WHERE n<1000000) SELECT sum(n) FROM work",
            [], |row| row.get::<_, i64>(0));
        assert_eq!(
            result.unwrap_err().sqlite_error_code(),
            Some(rusqlite::ErrorCode::OperationInterrupted)
        );
        db.progress_handler(0, None::<fn() -> bool>).unwrap();
        assert_eq!(
            db.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
