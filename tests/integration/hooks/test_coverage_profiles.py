"""Coverage collection must retain all counter data and fail closed on corruption."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('coverage_collection', ROOT / 'scripts/collect-coverage.py')
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class CoverageCollectionTests(unittest.TestCase):
    def test_only_empty_placeholders_are_removed_and_fallback_data_moves(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / 'target'
            fallback = root / 'fallback'
            target.mkdir()
            fallback.mkdir()
            (target / 'killed.profraw').touch()
            (fallback / 'default_empty.profraw').touch()
            invalid = target / 'truncated.profraw'
            invalid.write_bytes(b'corrupt nonempty profile')
            child = fallback / 'default_child.profraw'
            child.write_bytes(b'child counter data')
            unrelated = fallback / 'keep.txt'
            unrelated.write_bytes(b'')
            self.assertEqual(M.collect(target, fallback), 2)
            self.assertEqual(invalid.read_bytes(), b'corrupt nonempty profile')
            self.assertEqual((target / child.name).read_bytes(), b'child counter data')
            self.assertFalse(child.exists())
            self.assertTrue(unrelated.exists())
            self.assertEqual(M.collect(target, fallback), 0)

    def test_collision_never_overwrites_profile_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / 'target'
            fallback = root / 'fallback'
            target.mkdir()
            fallback.mkdir()
            for parent, data in [(target, b'first'), (fallback, b'second')]:
                (parent / 'default_same.profraw').write_bytes(data)
            with self.assertRaises(FileExistsError):
                M.collect(target, fallback)
            self.assertEqual((target / 'default_same.profraw').read_bytes(), b'first')
            self.assertEqual((fallback / 'default_same.profraw').read_bytes(), b'second')
