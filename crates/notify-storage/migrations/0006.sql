-- Transaction body: migrate only when freezing the first detailed Slack delivery.
CREATE TABLE slack_event_frames (
    internal_event_id TEXT NOT NULL,
    destination TEXT NOT NULL,
    payload BLOB NOT NULL CHECK(typeof(payload)='blob' AND length(payload) BETWEEN 1 AND 16384),
    sha256 BLOB NOT NULL CHECK(typeof(sha256)='blob' AND length(sha256)=32),
    PRIMARY KEY(internal_event_id,destination),
    FOREIGN KEY(internal_event_id,destination) REFERENCES public_event_routes(internal_event_id,destination)
);
CREATE TRIGGER slack_frame_sealed BEFORE INSERT ON slack_event_frames
WHEN EXISTS(SELECT 1 FROM public_event_frames WHERE internal_event_id=NEW.internal_event_id)
BEGIN SELECT RAISE(ABORT,'Slack frames are frozen'); END;
CREATE TRIGGER slack_frame_no_update BEFORE UPDATE ON slack_event_frames
BEGIN SELECT RAISE(ABORT,'Slack frame is frozen'); END;
CREATE TRIGGER slack_frame_no_delete BEFORE DELETE ON slack_event_frames
BEGIN SELECT RAISE(ABORT,'Slack frame is frozen'); END;
PRAGMA user_version=6;
