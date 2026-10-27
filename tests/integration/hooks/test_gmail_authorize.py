"""Synthetic workstation-helper tests; no Google packages, accounts or browser."""
import datetime
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('gmail_authorize', ROOT / 'scripts/gmail-authorize.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class GmailAuthorizeTests(unittest.TestCase):
    def test_grant_requires_verified_minimal_offline_identity(self):
        credentials = SimpleNamespace(refresh_token='fake-refresh', token='fake-access',
            expiry=datetime.datetime(2026, 1, 1), scopes=MODULE.SCOPES, granted_scopes=None,
            client_id='fake-client', client_secret='fake-secret')
        identity = dict(sub='fake-subject', email='s@example.invalid', email_verified=True)
        grant = MODULE.grant_from(credentials, identity)
        self.assertEqual(grant['scopes'], MODULE.SCOPES)
        with self.assertRaises(ValueError):
            MODULE.grant_from(credentials, dict(identity, email_verified=False))
        credentials.scopes = MODULE.SCOPES + ['gmail.readonly']
        with self.assertRaises(ValueError):
            MODULE.grant_from(credentials, identity)
        credentials.scopes = MODULE.SCOPES
        credentials.refresh_token = None
        with self.assertRaises(ValueError):
            MODULE.grant_from(credentials, identity)

    def test_publication_refuses_overwrite_links_and_broad_permissions(self):
        with tempfile.TemporaryDirectory() as raw:
            root = Path(raw).resolve()
            root.chmod(0o700)
            path = root / 'grant.json'
            MODULE.write_new(path, {'fake': True})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                MODULE.write_new(path, {'changed': True})
            link = root / 'link.json'
            link.symlink_to(path)
            with self.assertRaises(FileExistsError):
                MODULE.write_new(link, {'changed': True})
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                MODULE.write_new(root / 'new.json', {'fake': True})
