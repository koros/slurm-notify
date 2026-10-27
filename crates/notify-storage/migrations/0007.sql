-- Transaction body: first selected API-email event only; caller already owns state.
CREATE TABLE email_event_frames (
    internal_event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    profile_reference TEXT NOT NULL CHECK(length(profile_reference)=64),
    content_type TEXT NOT NULL CHECK(length(CAST(content_type AS BLOB)) BETWEEN 1 AND 200),
    payload BLOB NOT NULL CHECK(typeof(payload)='blob' AND length(payload) BETWEEN 1 AND 16384),
    sha256 BLOB NOT NULL CHECK(typeof(sha256)='blob' AND length(sha256)=32),
    PRIMARY KEY(internal_event_id,destination),
    FOREIGN KEY(internal_event_id,destination) REFERENCES public_event_routes(internal_event_id,destination)
);
CREATE TRIGGER email_frame_sealed BEFORE INSERT ON email_event_frames
WHEN EXISTS(SELECT 1 FROM public_event_frames WHERE internal_event_id=NEW.internal_event_id)
BEGIN SELECT RAISE(ABORT,'Email frames are frozen'); END;
CREATE TRIGGER email_frame_no_update BEFORE UPDATE ON email_event_frames
BEGIN SELECT RAISE(ABORT,'Email frame is frozen'); END;
CREATE TRIGGER email_frame_no_delete BEFORE DELETE ON email_event_frames
BEGIN SELECT RAISE(ABORT,'Email frame is frozen'); END;
CREATE TABLE email_provider_pauses (
    cluster TEXT NOT NULL CHECK(length(cluster) BETWEEN 1 AND 64),
    destination TEXT NOT NULL CHECK(length(destination) BETWEEN 1 AND 64),
    revision TEXT NOT NULL CHECK(length(revision)=64),
    reason TEXT NOT NULL CHECK(reason IN ('authentication','policy','credentials')),
    paused_at INTEGER NOT NULL CHECK(paused_at BETWEEN 0 AND 4102444799),
    PRIMARY KEY(cluster,destination)
);
PRAGMA user_version=7;
