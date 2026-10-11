BEGIN IMMEDIATE;
CREATE TABLE smtp_render_revisions (
    cluster TEXT NOT NULL,
    destination TEXT NOT NULL,
    revision TEXT NOT NULL CHECK(length(revision)=64 AND revision NOT GLOB '*[^0-9a-f]*'),
    descriptor BLOB NOT NULL CHECK(typeof(descriptor)='blob' AND length(descriptor) BETWEEN 1 AND 4096),
    PRIMARY KEY(cluster,destination,revision)
);
CREATE TABLE smtp_revision_bindings (
    event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    cluster TEXT NOT NULL,
    revision TEXT NOT NULL,
    PRIMARY KEY(event_id,destination),
    FOREIGN KEY(event_id,destination) REFERENCES outbox(event_id,destination) ON DELETE CASCADE,
    FOREIGN KEY(event_id,destination) REFERENCES smtp_messages(event_id,destination) ON DELETE CASCADE,
    FOREIGN KEY(cluster,destination,revision) REFERENCES smtp_render_revisions(cluster,destination,revision)
);
CREATE TRIGGER smtp_binding_cluster BEFORE INSERT ON smtp_revision_bindings
WHEN NOT EXISTS(SELECT 1 FROM events WHERE event_id=NEW.event_id AND cluster=NEW.cluster)
BEGIN SELECT RAISE(ABORT,'revision cluster mismatch'); END;
PRAGMA user_version=3;
COMMIT;
