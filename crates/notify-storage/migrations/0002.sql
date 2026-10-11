-- Private frozen single-recipient SMTP experiment; no released storage contract.
BEGIN IMMEDIATE;
CREATE TABLE smtp_messages (
    event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    sender TEXT NOT NULL CHECK(length(CAST(sender AS BLOB)) BETWEEN 1 AND 254),
    recipient TEXT NOT NULL CHECK(length(CAST(recipient AS BLOB)) BETWEEN 1 AND 254),
    data BLOB NOT NULL CHECK(typeof(data)='blob' AND length(data) BETWEEN 4 AND 16384),
    PRIMARY KEY(event_id, destination),
    FOREIGN KEY(event_id, destination) REFERENCES outbox(event_id, destination) ON DELETE CASCADE
);
PRAGMA user_version = 2;
COMMIT;
