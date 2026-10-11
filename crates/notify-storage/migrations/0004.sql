BEGIN IMMEDIATE;
CREATE TABLE smtp_provider_revisions (
    cluster TEXT NOT NULL,
    destination TEXT NOT NULL,
    revision TEXT NOT NULL CHECK(length(revision)=64 AND revision NOT GLOB '*[^0-9a-f]*'),
    descriptor BLOB NOT NULL CHECK(typeof(descriptor)='blob' AND length(descriptor) BETWEEN 1 AND 4096),
    PRIMARY KEY(cluster,destination,revision)
);
CREATE UNIQUE INDEX smtp_render_binding_scope ON smtp_revision_bindings(event_id,destination,cluster);
CREATE TABLE smtp_provider_bindings (
    event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    cluster TEXT NOT NULL,
    revision TEXT NOT NULL,
    PRIMARY KEY(event_id,destination),
    FOREIGN KEY(event_id,destination) REFERENCES outbox(event_id,destination) ON DELETE CASCADE,
    FOREIGN KEY(event_id,destination,cluster) REFERENCES smtp_revision_bindings(event_id,destination,cluster) ON DELETE CASCADE,
    FOREIGN KEY(cluster,destination,revision) REFERENCES smtp_provider_revisions(cluster,destination,revision)
);
PRAGMA user_version=4;
COMMIT;
