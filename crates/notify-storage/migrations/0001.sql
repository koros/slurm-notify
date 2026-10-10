-- Private schema experiment; not a released persistent-state contract.
-- Caller must enable/verify foreign_keys before beginning this transaction.
BEGIN IMMEDIATE;
CREATE TABLE jobs (
    cluster TEXT NOT NULL CHECK(length(cluster) > 0),
    job_id TEXT NOT NULL CHECK(length(job_id) > 0),
    submitted_at TEXT NOT NULL CHECK(length(submitted_at) > 0),
    started_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(length(state) > 0),
    observed_at INTEGER NOT NULL CHECK(observed_at >= 0),
    PRIMARY KEY(cluster, job_id, submitted_at, started_at)
);
CREATE TABLE events (
    event_id TEXT PRIMARY KEY NOT NULL CHECK(length(event_id) > 0),
    cluster TEXT NOT NULL,
    job_id TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    started_at TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK(length(event_type) > 0),
    observed_at INTEGER NOT NULL CHECK(observed_at >= 0),
    UNIQUE(cluster, job_id, submitted_at, started_at, event_type),
    FOREIGN KEY(cluster, job_id, submitted_at, started_at)
        REFERENCES jobs(cluster, job_id, submitted_at, started_at) ON DELETE RESTRICT
);
CREATE TABLE outbox (
    event_id TEXT NOT NULL REFERENCES events(event_id) ON DELETE RESTRICT,
    destination TEXT NOT NULL CHECK(length(destination) > 0),
    status TEXT NOT NULL CHECK(status IN ('pending', 'delivered', 'failed')),
    next_attempt_at INTEGER NOT NULL CHECK(next_attempt_at >= 0),
    PRIMARY KEY(event_id, destination)
);
CREATE INDEX outbox_due ON outbox(status, next_attempt_at);
CREATE TABLE delivery_attempts (
    event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    attempt_number INTEGER NOT NULL CHECK(attempt_number > 0),
    attempted_at INTEGER NOT NULL CHECK(attempted_at >= 0),
    outcome TEXT NOT NULL CHECK(outcome IN ('success', 'transient_failure', 'terminal_failure')),
    PRIMARY KEY(event_id, destination, attempt_number),
    FOREIGN KEY(event_id, destination) REFERENCES outbox(event_id, destination) ON DELETE RESTRICT
);
CREATE TABLE checkpoints (
    cluster TEXT PRIMARY KEY NOT NULL CHECK(length(cluster) > 0),
    history_through TEXT NOT NULL CHECK(length(history_through) > 0)
);
PRAGMA user_version = 1;
COMMIT;
