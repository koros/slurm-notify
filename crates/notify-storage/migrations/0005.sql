-- Transaction body: execute inside the event/checkpoint write transaction only.
CREATE TABLE public_event_frames (
    internal_event_id TEXT PRIMARY KEY NOT NULL REFERENCES events(event_id) ON DELETE RESTRICT,
    public_id TEXT UNIQUE NOT NULL CHECK(length(public_id)=36 AND substr(public_id,1,4)='evt_' AND substr(public_id,5) NOT GLOB '*[^0-9a-f]*' AND substr(public_id,17,1)='4' AND substr(public_id,21,1) IN ('8','9','a','b')),
    payload BLOB NOT NULL CHECK(typeof(payload)='blob' AND length(payload) BETWEEN 1 AND 16384),
    sha256 BLOB NOT NULL CHECK(typeof(sha256)='blob' AND length(sha256)=32)
);
CREATE TABLE public_event_routes (
    internal_event_id TEXT NOT NULL REFERENCES public_event_frames(internal_event_id) DEFERRABLE INITIALLY DEFERRED,
    destination TEXT NOT NULL CHECK(length(destination) BETWEEN 1 AND 64),
    revision TEXT NOT NULL CHECK(length(revision)=64 AND revision NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY(internal_event_id,destination)
);
-- Route rows are inserted before the parent frame, then the frame seals the set.
CREATE TRIGGER public_route_sealed BEFORE INSERT ON public_event_routes
WHEN EXISTS(SELECT 1 FROM public_event_frames WHERE internal_event_id=NEW.internal_event_id)
BEGIN SELECT RAISE(ABORT,'public routes are frozen'); END;
CREATE TRIGGER public_route_no_update BEFORE UPDATE ON public_event_routes
BEGIN SELECT RAISE(ABORT,'public routes are frozen'); END;
CREATE TRIGGER public_route_no_delete BEFORE DELETE ON public_event_routes
BEGIN SELECT RAISE(ABORT,'public routes are frozen'); END;
CREATE TRIGGER public_frame_no_update BEFORE UPDATE ON public_event_frames
BEGIN SELECT RAISE(ABORT,'public event is frozen'); END;
CREATE TRIGGER public_frame_no_delete BEFORE DELETE ON public_event_frames
BEGIN SELECT RAISE(ABORT,'public event is frozen'); END;
PRAGMA user_version=5;
