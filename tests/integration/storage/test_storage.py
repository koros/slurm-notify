"""SQL schema experiments, using only Python's standard-library SQLite driver.

These tests do not establish a production storage API or event wire contract.
"""
import sqlite3
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
MIGRATION = (ROOT / "crates/notify-storage/migrations/0001.sql").read_text()


def connect(path):
    db = sqlite3.connect(path)
    db.execute("PRAGMA foreign_keys = ON")
    assert db.execute("PRAGMA foreign_keys").fetchone() == (1,)
    return db


def migrate(db):
    version = db.execute("PRAGMA user_version").fetchone()[0]
    if version == 1:
        return
    if version != 0:
        raise ValueError("unsupported database version")
    try:
        db.executescript(MIGRATION)
    except sqlite3.Error:
        db.rollback()
        raise


def record(db, destinations=("primary",), event_id="event-1"):
    # The job snapshot, ledger, routing fan-out and checkpoint share one commit.
    db.execute("INSERT INTO jobs VALUES (?, ?, ?, ?, ?, ?)",
               ("cluster", "1", "submit", "start", "COMPLETED", 1))
    db.execute("INSERT INTO events VALUES (?, ?, ?, ?, ?, ?, ?)",
               (event_id, "cluster", "1", "submit", "start", "JOB_COMPLETED", 1))
    for destination in destinations:
        db.execute("INSERT INTO outbox VALUES (?, ?, ?, ?)",
                   (event_id, destination, "pending", 1))
    db.execute("INSERT INTO checkpoints VALUES (?, ?)", ("cluster", "through"))


class StorageSchemaTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.path = Path(self.directory.name) / "state.sqlite"
        self.db = connect(self.path)
        migrate(self.db)

    def tearDown(self):
        self.db.close()
        self.directory.cleanup()

    def test_atomic_commit_survives_reopen(self):
        with self.db:
            record(self.db, ("primary", "email"))
        self.db.close()
        self.db = connect(self.path)
        migrate(self.db)
        self.assertEqual(self.db.execute("SELECT count(*) FROM events").fetchone(), (1,))
        self.assertEqual(self.db.execute("SELECT count(*) FROM outbox").fetchone(), (2,))
        self.assertEqual(self.db.execute("SELECT history_through FROM checkpoints").fetchone(), ("through",))

    def test_failed_fanout_rolls_back_snapshot_ledger_and_checkpoint(self):
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                record(self.db, ("primary", ""))
        for table in ("jobs", "events", "outbox", "checkpoints"):
            self.assertEqual(self.db.execute(f"SELECT count(*) FROM {table}").fetchone(), (0,))

    def test_uncommitted_poll_is_rolled_back_on_close(self):
        record(self.db)
        self.db.close()
        self.db = connect(self.path)
        self.assertEqual(self.db.execute("SELECT count(*) FROM jobs").fetchone(), (0,))
        self.assertEqual(self.db.execute("SELECT count(*) FROM outbox").fetchone(), (0,))

    def test_logical_event_and_destination_uniqueness(self):
        with self.db:
            record(self.db)
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("INSERT INTO events VALUES (?, ?, ?, ?, ?, ?, ?)",
                                ("different-id", "cluster", "1", "submit", "start", "JOB_COMPLETED", 2))
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("INSERT INTO outbox VALUES (?, ?, ?, ?)", ("event-1", "primary", "pending", 1))

    def test_foreign_keys_and_retention_protection(self):
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("INSERT INTO outbox VALUES (?, ?, ?, ?)", ("missing", "primary", "pending", 1))
        with self.db:
            record(self.db)
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("DELETE FROM events")

    def test_delivery_attempt_and_outcome_commit_together(self):
        with self.db:
            record(self.db, ("primary", "email"))
        with self.assertRaises(sqlite3.IntegrityError):
            with self.db:
                self.db.execute("UPDATE outbox SET status='delivered' WHERE destination='primary'")
                self.db.execute("INSERT INTO delivery_attempts VALUES (?, ?, ?, ?, ?)",
                                ("event-1", "primary", 0, 2, "success"))
        self.assertEqual(self.db.execute("SELECT status FROM outbox WHERE destination='primary'").fetchone(), ("pending",))
        with self.db:
            self.db.execute("INSERT INTO delivery_attempts VALUES (?, ?, ?, ?, ?)",
                            ("event-1", "primary", 1, 2, "success"))
            self.db.execute("UPDATE outbox SET status='delivered' WHERE destination='primary'")
        self.db.close()
        self.db = connect(self.path)
        self.assertEqual(self.db.execute("SELECT destination, status FROM outbox ORDER BY destination").fetchall(),
                         [("email", "pending"), ("primary", "delivered")])
        self.assertEqual(self.db.execute("SELECT count(*) FROM delivery_attempts").fetchone(), (1,))

    def test_migration_is_repeat_safe_and_rejects_future_versions(self):
        migrate(self.db)
        self.assertEqual(self.db.execute("PRAGMA user_version").fetchone(), (1,))
        self.db.execute("PRAGMA user_version=2")
        with self.assertRaises(ValueError):
            migrate(self.db)
        self.assertEqual(self.db.execute("PRAGMA user_version").fetchone(), (2,))

    def test_failed_initial_migration_can_be_rolled_back(self):
        db = connect(":memory:")
        try:
            with self.assertRaises(sqlite3.OperationalError):
                db.executescript(MIGRATION.replace("PRAGMA user_version = 1;", "INVALID SQL;"))
            db.rollback()
            self.assertEqual(db.execute("PRAGMA user_version").fetchone(), (0,))
            self.assertEqual(db.execute("SELECT count(*) FROM sqlite_master WHERE type='table'").fetchone(), (0,))
            migrate(db)
            self.assertEqual(db.execute("PRAGMA user_version").fetchone(), (1,))
        finally:
            db.close()


if __name__ == "__main__":
    unittest.main()
