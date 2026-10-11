"""Private review-fixture validator; not an installed parser or approved schema."""
import json
import re


class InvalidDraft(ValueError):
    """Static categories only; rejected inputs must not enter diagnostics."""


def fail(category):
    raise InvalidDraft(category) from None


def decode(data, limits):
    if not isinstance(data, bytes) or not 1 <= len(data) <= limits['config_bytes']:
        fail('Bytes')
    try:
        source = data.decode('utf-8')
    except UnicodeError:
        fail('Syntax')
    if source.startswith('\ufeff'):
        fail('Syntax')
    depth = 0
    quoted = escaped = False
    for char in source:
        if quoted:
            if escaped:
                escaped = False
            elif char == '\\':
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in '[{':
            depth += 1
            if depth > limits['container_depth']:
                fail('Depth')
        elif char in ']}':
            depth -= 1
            if depth < 0:
                fail('Syntax')

    def pairs(items):
        if len(items) > limits['object_members']:
            fail('Structure')
        value = {}
        for key, item in items:
            if key in value:
                fail('Duplicate')
            value[key] = item
        return value

    def integer(token):
        if not re.fullmatch(r'0|[1-9][0-9]{0,19}', token):
            fail('Integer')
        value = int(token)
        if value > (1 << 64) - 1:
            fail('Integer')
        return value

    def noninteger(_):
        fail('Integer')

    try:
        value = json.loads(source, object_pairs_hook=pairs, parse_int=integer,
                           parse_float=noninteger, parse_constant=noninteger)
    except InvalidDraft:
        raise
    except (ValueError, RecursionError):
        fail('Syntax')

    def arrays(item):
        if isinstance(item, list):
            if len(item) > limits['array_elements']:
                fail('Structure')
            for child in item:
                arrays(child)
        elif isinstance(item, dict):
            for child in item.values():
                arrays(child)
    arrays(value)
    return value


def object_fields(value, required, optional=()):
    if not isinstance(value, dict) or not set(required) <= value.keys():
        fail('Fields')
    if not value.keys() <= set(required) | set(optional):
        fail('Fields')


def bounded_integer(value, bounds):
    if type(value) is not int or not bounds['minimum'] <= value <= bounds['maximum']:
        fail('Range')


def text(value, pattern, maximum):
    if not isinstance(value, str):
        fail('Text')
    try:
        size = len(value.encode('utf-8'))
    except UnicodeError:
        fail('Text')
    if not 1 <= size <= maximum:
        fail('Text')
    if not re.fullmatch(pattern, value, flags=re.ASCII):
        fail('Text')


def alias(value):
    text(value, r'[A-Za-z0-9_-]+', 64)


def env(value):
    text(value, r'[A-Za-z_][A-Za-z0-9_]*', 128)


def path(value):
    text(value, r'/[^\x00-\x1f\x7f]*', 4096)


def members(value, maximum, allow_empty=False):
    if not isinstance(value, list) or not (0 if allow_empty else 1) <= len(value) <= maximum:
        fail('Structure')
    return value


def distinct(values):
    if len(set(values)) != len(values):
        fail('Duplicate')


def validate(data, manifest):
    """Validate fixture shape/budgets only; do not resolve env, paths or trust."""
    caps = manifest['structural_limits']
    config = decode(data, caps)
    object_fields(config, ['schema_version', 'monitor', 'storage', 'providers', 'routes',
                           'delivery', 'maintenance'])
    if type(config['schema_version']) is not int or config['schema_version'] != 1:
        fail('Version')
    monitor = config['monitor']
    monitor_numbers = [key.split('.')[1] for key in manifest['integer_ranges']
                       if key.startswith('monitor.')]
    object_fields(monitor, monitor_numbers + ['mode', 'cluster_id', 'user_jobs_only',
                                            'initial_observation'])
    if monitor['mode'] != 'local' or monitor['user_jobs_only'] is not True:
        fail('Policy')
    if monitor['initial_observation'] not in ['suppress', 'emit']:
        fail('Policy')
    alias(monitor['cluster_id'])
    object_fields(config['storage'], ['path'])
    path(config['storage']['path'])
    providers = config['providers']
    if not isinstance(providers, dict) or not 1 <= len(providers) <= caps['providers']:
        fail('Structure')
    smtp_names = []
    for name, provider in providers.items():
        alias(name)
        if not isinstance(provider, dict):
            fail('Fields')
        kind = provider.get('type')
        if kind == 'webhook':
            object_fields(provider, ['type', 'url_env', 'request_timeout_seconds',
                                     'payload_limit_bytes'], ['authorization_env'])
            env(provider['url_env'])
            if 'authorization_env' in provider:
                env(provider['authorization_env'])
        elif kind == 'smtp':
            smtp_names.append(name)
            object_fields(provider, ['type', 'relay', 'port', 'tls_mode', 'server_name',
                                     'trust_der_files', 'authentication', 'sender', 'recipient',
                                     'visibility', 'identity_namespace',
                                     'request_timeout_milliseconds'])
            if provider['tls_mode'] != 'required_starttls' or provider['visibility'] not in ['hidden', 'included']:
                fail('Policy')
            for field in ['relay', 'server_name']:
                text(provider[field], r'[A-Za-z0-9.:-]+', 253)
            for field in ['sender', 'recipient']:
                text(provider[field], r'[^\x00-\x20\x7f-\U0010ffff]+@[^\x00-\x20\x7f-\U0010ffff]+', 254)
            alias(provider['identity_namespace'])
            roots = members(provider['trust_der_files'], 32)
            for root in roots:
                path(root)
            distinct(roots)
            auth = provider['authentication']
            object_fields(auth, ['identity', 'username_env', 'password_env'])
            text(auth['identity'], r'[\x20-\x7e]+', 64)
            env(auth['username_env'])
            env(auth['password_env'])
        else:
            fail('Policy')
        for key, bounds in manifest[kind + '_integer_ranges'].items():
            bounded_integer(provider[key], bounds)
    events = {'JOB_STARTED', 'JOB_COMPLETED', 'JOB_FAILED', 'JOB_TIMEOUT', 'JOB_CANCELLED',
              'JOB_OUT_OF_MEMORY', 'JOB_NODE_FAILURE', 'JOB_BOOT_FAILURE', 'JOB_DEADLINE'}
    for route in members(config['routes'], caps['routes'], allow_empty=True):
        object_fields(route, ['events', 'send_to'])
        selected = members(route['events'], caps['route_events'])
        targets = members(route['send_to'], caps['route_destinations'])
        for event in selected:
            if not isinstance(event, str) or event not in events:
                fail('Reference')
        for target in targets:
            alias(target)
            if target not in providers:
                fail('Reference')
        distinct(selected)
        distinct(targets)
    delivery = config['delivery']
    required = [key.split('.')[1] for key in manifest['integer_ranges']
                if key.startswith('delivery.') and key != 'delivery.prepared_smtp_message_limit']
    required += ['jitter_seed']
    if smtp_names:
        required += ['prepared_smtp_message_limit', 'smtp_permissions']
    object_fields(delivery, required)
    seed = delivery['jitter_seed']
    text(seed, r'0|[1-9][0-9]{0,19}', 20)
    if int(seed) > (1 << 64) - 1:
        fail('Integer')
    permissions = members(delivery.get('smtp_permissions', []), caps['smtp_permissions'], allow_empty=True)
    seen = set()
    for permission in permissions:
        object_fields(permission, ['cluster_id', 'destination', 'rendering_revision', 'provider_revision'])
        alias(permission['cluster_id'])
        alias(permission['destination'])
        if permission['cluster_id'] != monitor['cluster_id'] or permission['destination'] not in smtp_names:
            fail('Reference')
        for field in ['rendering_revision', 'provider_revision']:
            text(permission[field], r'[0-9a-f]{64}', 64)
        key = tuple(permission[field] for field in ['cluster_id', 'destination', 'rendering_revision', 'provider_revision'])
        if key in seen:
            fail('Duplicate')
        seen.add(key)
    maintenance = config['maintenance']
    object_fields(maintenance, [key.split('.')[1] for key in manifest['integer_ranges']
                                if key.startswith('maintenance.')])
    for field, bounds in manifest['integer_ranges'].items():
        if field == 'delivery.prepared_smtp_message_limit' and not smtp_names:
            continue
        group, name = field.split('.')
        bounded_integer(config[group][name], bounds)
    if (monitor['query_timeout_seconds'] >= monitor['poll_seconds']
            or monitor['accounting_overlap_seconds'] >= monitor['accounting_lookback_seconds']
            or monitor['stale_after_seconds'] < monitor['poll_seconds']
            or delivery['max_backoff_seconds'] < delivery['initial_backoff_seconds']
            or delivery['jitter_seconds'] > delivery['max_backoff_seconds']):
        fail('Relationship')
    return config
