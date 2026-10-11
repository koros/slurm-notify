"""Failure/edge fixtures for private drafts, without resource resolution."""
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch

from draft_validation import InvalidDraft, bounded_integer, decode, validate

ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'docs/proposals/examples'
MANIFEST = json.loads((EXAMPLES / 'config-limits-v1.json').read_text())
MIXED = json.loads((EXAMPLES / 'local-mixed-config.json').read_text())


class DraftValidationTests(unittest.TestCase):
    def assert_invalid(self, value, category=None):
        data = value if isinstance(value, bytes) else json.dumps(value).encode()
        with self.assertRaises(InvalidDraft) as caught:
            validate(data, MANIFEST)
        self.assertIn(str(caught.exception), {'Bytes', 'Syntax', 'Depth', 'Structure',
                      'Duplicate', 'Integer', 'Fields', 'Range', 'Text', 'Version',
                      'Policy', 'Reference', 'Relationship'})
        if category:
            self.assertEqual(str(caught.exception), category)

    def test_examples_are_valid_without_loading_resources(self):
        fixtures = [(EXAMPLES / name).read_bytes() for name in
                    ['local-webhook-config.json', 'local-mixed-config.json']]
        with patch('builtins.open', side_effect=AssertionError('resource opened')), \
                patch('io.open', side_effect=AssertionError('resource opened')), \
                patch('os.open', side_effect=AssertionError('resource opened')):
            for data in fixtures:
                self.assertEqual(validate(data, MANIFEST)['schema_version'], 1)
        empty = copy.deepcopy(MIXED)
        empty['routes'] = []
        empty['delivery']['smtp_permissions'] = []
        self.assertEqual(validate(json.dumps(empty).encode(), MANIFEST)['routes'], [])

    def test_byte_budget_utf8_bom_trailing_and_static_errors(self):
        good = json.dumps(MIXED).encode()
        maximum = MANIFEST['structural_limits']['config_bytes']
        self.assertEqual(validate(good + b' ' * (maximum - len(good)), MANIFEST)['schema_version'], 1)
        for data in [b'', good + b' ' * (maximum + 1 - len(good)), b'\xff',
                     b'\xef\xbb\xbf' + good, good + b' private-value', b'{"private-value":']:
            self.assert_invalid(data)

    def test_duplicate_decoded_keys_and_integer_lexemes(self):
        for data in [b'{"schema_version":1,"schema_versio\\u006e":1}',
                     b'{"a":{"private-value":1,"private-value":2}}']:
            self.assert_invalid(data, 'Duplicate')
        for token in ['1.0', '1e0', '-0', '-1', '18446744073709551616', '9' * 100,
                      'NaN', 'Infinity', '-Infinity']:
            self.assert_invalid(('{"schema_version":' + token + '}').encode(), 'Integer')

    def test_depth_budget_ignores_escaped_string_brackets(self):
        caps = MANIFEST['structural_limits']
        self.assertEqual(decode(b'[' * 8 + b'0' + b']' * 8, caps), [[[[[[[[0]]]]]]]])
        with self.assertRaisesRegex(InvalidDraft, '^Depth$'):
            decode(b'[' * 9 + b'0' + b']' * 9, caps)
        quoted = {'a': '"\\' + '[' * 100 + '}' * 100}
        self.assertEqual(decode(json.dumps(quoted).encode(), caps), quoted)
        for data in [b']', b'{"a":"unfinished', b'[}']:
            with self.assertRaises(InvalidDraft):
                decode(data, caps)

    def test_object_and_array_budgets(self):
        caps = MANIFEST['structural_limits']
        decode(json.dumps(dict.fromkeys((str(n) for n in range(128)), 0)).encode(), caps)
        decode(json.dumps([0] * 128).encode(), caps)
        for value in [dict.fromkeys((str(n) for n in range(129)), 0), [0] * 129]:
            with self.assertRaisesRegex(InvalidDraft, '^Structure$'):
                decode(json.dumps(value).encode(), caps)

    def test_each_numeric_budget_checks_boundaries_types_and_overflow(self):
        groups = ['integer_ranges', 'webhook_integer_ranges', 'smtp_integer_ranges']
        for group in groups:
            for name, bounds in MANIFEST[group].items():
                with self.subTest(field=name):
                    for value in [bounds['minimum'], bounds['maximum']]:
                        bounded_integer(value, bounds)
                    for value in [bounds['minimum'] - 1, bounds['maximum'] + 1,
                                  True, False, 1.0, '1', None, 1 << 64]:
                        with self.assertRaisesRegex(InvalidDraft, '^Range$'):
                            bounded_integer(value, bounds)
        for group, field in [('monitor', 'poll_seconds'), ('delivery', 'max_attempts'),
                             ('maintenance', 'cleanup_batch_limit')]:
            value = copy.deepcopy(MIXED)
            value[group][field] = True
            self.assert_invalid(value, 'Range')

    def test_unknown_missing_fields_and_versions(self):
        for group in [None, 'monitor', 'storage', 'delivery', 'maintenance']:
            value = copy.deepcopy(MIXED)
            target = value if group is None else value[group]
            target['private-value'] = 'sensitive-placeholder'
            self.assert_invalid(value, 'Fields')
            value = copy.deepcopy(MIXED)
            target = value if group is None else value[group]
            del target[next(iter(target))]
            self.assert_invalid(value, 'Fields')
        for version in [0, 2, '1', True]:
            value = copy.deepcopy(MIXED)
            value['schema_version'] = version
            self.assert_invalid(value, 'Version')
        value = copy.deepcopy(MIXED)
        value['providers']['research_email']['authentication']['password'] = 'private-value'
        self.assert_invalid(value, 'Fields')

    def test_all_cross_field_rules(self):
        for group, field, replacement in [
            ('monitor', 'query_timeout_seconds', 60),
            ('monitor', 'accounting_overlap_seconds', 3600),
            ('monitor', 'stale_after_seconds', 59),
            ('delivery', 'max_backoff_seconds', 4),
            ('delivery', 'jitter_seconds', 301),
        ]:
            value = copy.deepcopy(MIXED)
            value[group][field] = replacement
            self.assert_invalid(value, 'Relationship')

    def test_routes_provider_reference_and_count_failures(self):
        maximum = copy.deepcopy(MIXED)
        maximum['routes'] = maximum['routes'] * 128
        validate(json.dumps(maximum).encode(), MANIFEST)
        maximum['routes'].append(maximum['routes'][0])
        self.assert_invalid(maximum, 'Structure')
        maximum = copy.deepcopy(MIXED)
        maximum['providers'] = {str(n): copy.deepcopy(MIXED['providers']['primary']) for n in range(64)}
        maximum['routes'][0]['send_to'] = list(maximum['providers'])
        del maximum['delivery']['prepared_smtp_message_limit']
        del maximum['delivery']['smtp_permissions']
        validate(json.dumps(maximum).encode(), MANIFEST)
        for key, replacement in [('events', []), ('events', ['UNKNOWN']),
                                 ('events', ['JOB_FAILED'] * 2), ('send_to', ['missing']),
                                 ('send_to', ['primary'] * 2), ('send_to', [None])]:
            value = copy.deepcopy(MIXED)
            value['routes'][0][key] = replacement
            self.assert_invalid(value)
        for providers in [{}, {'private-value': None}, {str(n): MIXED['providers']['primary'] for n in range(65)}]:
            value = copy.deepcopy(MIXED)
            value['providers'] = providers
            self.assert_invalid(value)

    def test_smtp_permissions_and_resource_references(self):
        maximum = copy.deepcopy(MIXED)
        maximum['providers']['research_email']['trust_der_files'] = [f'/fixture/{n}.der' for n in range(32)]
        permission = maximum['delivery']['smtp_permissions'][0]
        maximum['delivery']['smtp_permissions'] = [dict(permission, provider_revision=f'{n:064x}') for n in range(64)]
        validate(json.dumps(maximum).encode(), MANIFEST)
        maximum['delivery']['smtp_permissions'].append(dict(permission, provider_revision=f'{64:064x}'))
        self.assert_invalid(maximum, 'Structure')
        for field in ['prepared_smtp_message_limit', 'smtp_permissions']:
            value = copy.deepcopy(MIXED)
            del value['delivery'][field]
            self.assert_invalid(value, 'Fields')
        for field, replacement in [('cluster_id', 'other'), ('destination', 'primary'),
                                   ('provider_revision', 'A' * 64), ('rendering_revision', 'a' * 63)]:
            value = copy.deepcopy(MIXED)
            value['delivery']['smtp_permissions'][0][field] = replacement
            self.assert_invalid(value)
        value = copy.deepcopy(MIXED)
        value['delivery']['smtp_permissions'] *= 2
        self.assert_invalid(value, 'Duplicate')
        for roots in [[], ['relative.der'], ['/same.der'] * 2, ['/fixture.der'] * 33]:
            value = copy.deepcopy(MIXED)
            value['providers']['research_email']['trust_der_files'] = roots
            self.assert_invalid(value)
        for field, replacement in [('tls_mode', 'implicit'), ('visibility', 'unknown'),
                                   ('recipient', '\U0001f600@example.invalid')]:
            value = copy.deepcopy(MIXED)
            value['providers']['research_email'][field] = replacement
            self.assert_invalid(value)

    def test_text_seed_policy_and_surrogate_failures_are_static(self):
        for seed in ['01', '-1', '18446744073709551616', 1, '', None]:
            value = copy.deepcopy(MIXED)
            value['delivery']['jitter_seed'] = seed
            self.assert_invalid(value)
        for field, replacement in [('initial_observation', 'unknown'), ('mode', 'remote'),
                                   ('user_jobs_only', False), ('cluster_id', '\ud800')]:
            value = copy.deepcopy(MIXED)
            value['monitor'][field] = replacement
            self.assert_invalid(value)
        for reference in ['BAD-NAME', '1BAD', 'x' * 129, 'private-value\n']:
            value = copy.deepcopy(MIXED)
            value['providers']['primary']['url_env'] = reference
            self.assert_invalid(value, 'Text')


if __name__ == '__main__':
    unittest.main()
