#!/usr/bin/env python3
"""Workstation-only Gmail consent helper. Never run a browser callback on an HPC."""
import argparse
import datetime
import json
import os
from pathlib import Path
import stat
import signal
import sys

SEND = 'https://www.googleapis.com/auth/gmail.send'
SCOPES = [SEND, 'openid', 'email']


def decode(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=pairs)


def protected_directory(path):
    """Reject symlinks and broad/untrusted ancestors, including the final directory."""
    if not path.is_absolute() or any(part in ('.', '..') for part in path.parts):
        raise ValueError('path')
    current = Path('/')
    for part in path.parts[1:]:
        current = current / part
        info = current.lstat()
        if not stat.S_ISDIR(info.st_mode):
            raise ValueError('directory')
        if current == path:
            if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
                raise ValueError('permissions')
        elif info.st_uid not in (0, os.geteuid()) or (
                info.st_mode & 0o022 and not (info.st_uid == 0 and info.st_mode & stat.S_ISVTX)):
            raise ValueError('ancestor')


def protected_client(path):
    protected_directory(path.parent)
    fd = os.open(str(path), os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600:
            raise ValueError('client')
        data = stream.read(16385)
    if len(data) > 16384:
        raise ValueError('client')
    value = decode(data)
    client = value.get('installed', {})
    if set(value) != {'installed'} or client.get('auth_uri') not in ('https://accounts.google.com/o/oauth2/auth', 'https://accounts.google.com/o/oauth2/v2/auth') or client.get('token_uri') != 'https://oauth2.googleapis.com/token':
        raise ValueError('desktop-client')
    return value


def grant_from(credentials, identity):
    if not identity.get('sub') or not identity.get('email') or identity.get('email_verified') is not True:
        raise ValueError('identity')
    if not credentials.refresh_token or not credentials.token or credentials.expiry is None:
        raise ValueError('offline-grant')
    expiry = credentials.expiry
    if expiry.tzinfo is None:
        expiry = expiry.replace(tzinfo=datetime.timezone.utc)
    scopes = set(credentials.granted_scopes or credentials.scopes or [])
    if not {SEND, 'openid'} <= scopes or not ({'email', 'https://www.googleapis.com/auth/userinfo.email'} & scopes) or len(scopes) != 3:
        raise ValueError('scopes')
    return dict(schema_version=1, client_id=credentials.client_id,
                client_secret=credentials.client_secret, subject=identity['sub'],
                email=identity['email'], refresh_token=credentials.refresh_token,
                access_token=credentials.token, expires_at=int(expiry.timestamp()), scopes=SCOPES)


def write_new(path, value):
    protected_directory(path.parent)
    data = (json.dumps(value, indent=2) + '\n').encode()
    if len(data) > 16384:
        raise ValueError('size')
    fd = os.open(str(path), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    directory = os.open(str(path.parent), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--client', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True,
                        help='New protected grant file; existing files are never overwritten')
    args = parser.parse_args()
    try:
        protected_directory(args.output.parent)
        if args.output.exists() or args.output.is_symlink():
            raise ValueError('existing-output')
        # Bound the whole workstation flow, including library HTTP exchanges.
        def expired(_signal, _frame):
            raise TimeoutError()
        signal.signal(signal.SIGALRM, expired)
        signal.alarm(240)
        client = protected_client(args.client)
        from google_auth_oauthlib.flow import InstalledAppFlow
        flow = InstalledAppFlow.from_client_config(client, SCOPES, autogenerate_code_verifier=True)
        flow.oauth2session.trust_env = False
        # The supported library owns state verification, PKCE, one loopback
        # callback and code exchange. Suppress OAuth URLs/codes in terminal output.
        flow.run_local_server(host='127.0.0.1', port=0, timeout_seconds=180,
                              authorization_prompt_message='', open_browser=True,
                              access_type='offline', prompt='consent')
        session = flow.authorized_session()
        session.trust_env = False
        response = session.get('https://openidconnect.googleapis.com/v1/userinfo',
                               timeout=15, allow_redirects=False, stream=True,
                               headers={'Accept-Encoding': 'identity'})
        try:
            if response.status_code != 200:
                raise ValueError('identity')
            body = response.raw.read(8193, decode_content=False)
            if len(body) > 8192 or response.headers.get('Content-Encoding', 'identity') != 'identity':
                raise ValueError('identity')
            grant = grant_from(flow.credentials, decode(body))
        finally:
            response.close()
            session.close()
        write_new(args.output, grant)
        signal.alarm(0)
        print('Protected offline grant created. No email was sent. Keep the file private.')
        return 0
    except Exception:
        print('Authorization or protected publication failed. Inspect the output locally before retrying; no email was sent.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
