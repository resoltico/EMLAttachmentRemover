#!/usr/bin/env python3
"""Finite native naming oracle for an exclusively owned destination profile."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import struct
import subprocess

CONTENT = b'Content-Type: text/plain\r\n\r\nOwned naming profile fixture.\r\n'
SUFFIX = '.no-attachments.eml'


def token(route, name):
    stem = name[:-4] if name[-4:].lower() == '.eml' else name
    parts = [(1, part) for part in route] + [(2, (stem or 'message') + SUFFIX)]
    seed = b'eml-attachment-remover:destination-route:v1\0' + struct.pack('<I', len(parts))
    for kind, part in parts:
        raw = part.encode('utf-8')
        seed += bytes([kind, 0]) + struct.pack('<I', len(raw)) + raw
    return base64.b32encode(hashlib.sha256(seed).digest()[:16]).decode('ascii').lower().rstrip('=')


def truncate(text, units):
    encoding = 'utf-16-le' if os.name == 'nt' else 'utf-8'
    while len(text.encode(encoding)) > units * (2 if os.name == 'nt' else 1):
        text = text[:-1]
    if not text:
        raise ValueError('Fixture component has no human character')
    return text


def expected(destination, route, name, limit):
    parent = destination
    for part in route:
        parent /= truncate(part, limit - 8) + '.eml-dir'
    stem = name[:-4] if name[-4:].lower() == '.eml' else name
    return parent / (truncate(stem or 'message', limit - 46) + '.' + token(route, name) + SUFFIX)


def inventory(root):
    entries = []
    for parent, directories, files in os.walk(root):
        for name in sorted(directories + files):
            path = Path(parent) / name
            facts = path.lstat()
            if not (stat.S_ISREG(facts.st_mode) or stat.S_ISDIR(facts.st_mode)):
                raise RuntimeError('Native profile contains a link or nonordinary entry')
            entries.append({'relative': str(path.relative_to(root)), 'native_hex': os.fsencode(str(path.relative_to(root))).hex() if os.name != 'nt' else str(path.relative_to(root)).encode('utf-16-le').hex(), 'directory': stat.S_ISDIR(facts.st_mode), 'device': facts.st_dev, 'inode': facts.st_ino, 'bytes': facts.st_size, 'mode': stat.S_IMODE(facts.st_mode), 'mtime_ns': facts.st_mtime_ns, 'ctime_ns': facts.st_ctime_ns, 'uid': facts.st_uid, 'gid': facts.st_gid, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(facts.st_mode) else None})
    return entries


def native(path):
    return {'encoding': 'windows-utf16le' if os.name == 'nt' else 'unix-bytes',
            'base64': base64.b64encode(str(path).encode('utf-16-le') if os.name == 'nt' else os.fsencode(path)).decode('ascii')}


def namespace(root, cases, limit):
    entries = inventory(root)
    expected_files = set()
    expected_directories = set()
    for path, route, name in cases:
        target = expected(root, route, name, limit)
        facts = target.lstat()
        if not stat.S_ISREG(facts.st_mode) or target.read_bytes() != CONTENT:
            raise RuntimeError('Independent expected leaf is not an ordinary exact-byte copy')
        expected_files.add((facts.st_dev, facts.st_ino))
        parent = target.parent
        while parent != root:
            facts = parent.lstat()
            if not stat.S_ISDIR(facts.st_mode):
                raise RuntimeError('Independent expected mirror is not an ordinary directory')
            expected_directories.add((facts.st_dev, facts.st_ino))
            parent = parent.parent
    actual_files = {(entry['device'], entry['inode']) for entry in entries if not entry['directory']}
    actual_directories = {(entry['device'], entry['inode']) for entry in entries if entry['directory']}
    if len(expected_files) != len(cases) or actual_files != expected_files or actual_directories != expected_directories or len([entry for entry in entries if not entry['directory']]) != len(cases):
        raise RuntimeError('Actual namespace contains merged leaves, extra mirrors or residual stage entries')
    return {'expected_file_identities': sorted(expected_files), 'actual_file_identities': sorted(actual_files),
            'expected_directory_identities': sorted(expected_directories), 'actual_directory_identities': sorted(actual_directories),
            'entries': entries}


def root_facts(root):
    facts = root.lstat()
    return (facts.st_dev, facts.st_ino, stat.S_IMODE(facts.st_mode), facts.st_uid, facts.st_gid, facts.st_mtime_ns, facts.st_ctime_ns)


def execute(binary, paths, destination, mode, index, out, cases, limit):
    count = len(cases)
    args = [str(binary), '--format', 'json', '--output-dir', str(destination)]
    if mode == 'check':
        args.append('--dry-run')
    args += ['--'] + [str(path) for path in paths]
    run = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    (out / (str(index) + '.jsonl')).write_bytes(run.stdout)
    (out / (str(index) + '.stderr')).write_bytes(run.stderr)
    rows = [json.loads(line) for line in run.stdout.splitlines()]
    items = [row for row in rows if row.get('type') == 'item']
    summaries = [row for row in rows if row.get('type') == 'summary']
    if run.returncode or len(rows) != count + 2 or len(items) != count or len(summaries) != 1 or rows[0].get('type') != 'start' or rows[0].get('total') != count or rows[0].get('collection_complete') is not True or summaries[0].get('exit_code') != 0:
        raise RuntimeError('Native profile CLI failed; retained transcript ' + str(index))
    status = {'check': 'planned', 'create': 'created', 'repeat': 'up_to_date'}[mode]
    if [row.get('index') for row in items] != list(range(count)) or any(row.get('status') != status for row in items):
        raise RuntimeError('Native profile did not establish every required ' + status + ' result')
    expected_by_source = {json.dumps(native(path), sort_keys=True): native(expected(destination, route, name, limit)) for path, route, name in cases}
    seen = set()
    destinations = []
    for item in items:
        source = item['source']['native']
        key = json.dumps(source, sort_keys=True)
        actual = item['destination']['native']
        if key not in expected_by_source or key in seen or actual != expected_by_source[key]:
            raise RuntimeError('Emitted native source/destination differs from independent naming oracle')
        if item['source_bytes'] != len(CONTENT) or item['copy_bytes'] != len(CONTENT):
            raise RuntimeError('Emitted source/copy byte metadata differs from independent fixture')
        seen.add(key)
        destinations.append({'source_native': source, 'independent_expected': expected_by_source[key], 'actual_destination': actual})
    return {'destinations': destinations, 'mode': mode, 'exit_code': run.returncode, 'items': count, 'statuses': [row['status'] for row in items], 'transcript': str(index) + '.jsonl'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['binary', 'source-root', 'destination', 'out']:
        parser.add_argument('--' + name, required=True, type=Path)
    args = parser.parse_args()
    binary, sources, destination, out = [path.resolve() for path in [args.binary, args.source_root, args.destination, args.out]]
    sources.mkdir(mode=0o700)
    out.mkdir()
    if not destination.is_dir() or any(destination.iterdir()):
        raise RuntimeError('Profile destination must be an existing empty exclusively owned directory')
    if token([], 'message.eml') != 'eudtfo5quf3d2hlohgknt7qpaa' or token([], 'A.eml') != 'gsfuhldmedi75npfikxt6rlujm' or token(['A'], 'message.eml') != 'updzf5iesh4csbqanb4nwnbwvq':
        raise RuntimeError('Independent naming oracle disagrees with fixed vectors')
    limit = 255 if os.name == 'nt' else os.pathconf(destination, 'PC_NAME_MAX')
    cases = []
    paths = []
    for index, name in enumerate(['A.eml', 'a.eml', 'é.eml', 'e\u0301.eml', 'Σ.eml', 'σ.eml', 'ς.eml', 'x' * 250 + '.eml']):
        parent = sources / ('file-' + str(index))
        parent.mkdir()
        path = parent / name
        path.write_bytes(CONTENT)
        cases.append((path, [], name))
        paths.append(path)
    for index, name in enumerate(['A', 'a', 'é', 'e\u0301', 'Σ', 'σ', 'x' * 247 + 'AAA', 'x' * 247 + 'BBB']):
        parent = sources / ('tree-' + str(index))
        parent.mkdir()
        selected = parent / name
        selected.mkdir()
        path = selected / 'message.eml'
        path.write_bytes(CONTENT)
        cases.append((path, [name], 'message.eml'))
        paths.append(selected)
    original_identities = {str(path): (path.stat().st_dev, path.stat().st_ino, path.stat().st_size, path.stat().st_mtime_ns, stat.S_IMODE(path.stat().st_mode), path.stat().st_uid, path.stat().st_gid) for path, _, _ in cases}
    probe = destination / 'CaseProbe'
    probe.write_bytes(b'')
    try:
        with (destination / 'caseprobe').open('xb'):
            pass
        case_mode = 'case-sensitive'
    except FileExistsError:
        case_mode = 'case-insensitive'
    probe.unlink()
    (destination / 'caseprobe').unlink(missing_ok=True)
    identity = hashlib.sha256(binary.read_bytes()).hexdigest()
    result = {'schema': 1, 'binary_sha256': identity, 'host': platform.uname()._asdict(), 'case_mode': case_mode, 'destination_device': destination.stat().st_dev, 'name_limit': limit, 'source_bytes_sha256': hashlib.sha256(CONTENT).hexdigest(), 'source_identities': original_identities, 'runs': [], 'expected': [], 'entries': [], 'failure': None, 'passed': False}
    try:
        for index, (mode, reverse) in enumerate([('check', False), ('create', False), ('repeat', True)]):
            before = inventory(destination)
            before_root = root_facts(destination)
            result['runs'].append(execute(binary, list(reversed(paths)) if reverse else paths, destination, mode, index, out, cases, limit))
            if mode in ['check', 'repeat'] and (inventory(destination) != before or root_facts(destination) != before_root):
                raise RuntimeError('Check/repeat changed native namespace, permissions, ownership or bytes')
            if mode != 'check':
                for path, route, name in cases:
                    target = expected(destination, route, name, limit)
                    if path.read_bytes() != CONTENT or target.read_bytes() != CONTENT:
                        raise RuntimeError('Original or independent expected leaf bytes changed')
                result['runs'][-1]['namespace'] = namespace(destination, cases, limit)
        # A second fresh layout, with reversed request order, must use the same
        # complete native leaf names and bytes; this is not a retry of failure.
        fresh = destination / 'fresh-layout'
        fresh.mkdir(mode=0o700)
        result['runs'].append(execute(binary, list(reversed(paths)), fresh, 'create', 3, out, cases, limit))
        for path, route, name in cases:
            target = expected(fresh, route, name, limit)
            if target.read_bytes() != CONTENT or path.read_bytes() != CONTENT:
                raise RuntimeError('Fresh reversed layout differs from the independent oracle')
        result['runs'][-1]['namespace'] = namespace(fresh, cases, limit)
        result['expected'] = [{'source': str(path), 'route': route, 'target': str(expected(destination, route, name, limit)), 'token': token(route, name), 'source_native': native(path), 'destination_native': native(expected(destination, route, name, limit)), 'fresh_destination_native': native(expected(fresh, route, name, limit)), 'sha256': hashlib.sha256(CONTENT).hexdigest()} for path, route, name in cases]
        result['entries'] = inventory(destination)
        for path, _, _ in cases:
            facts = path.stat()
            if (facts.st_dev, facts.st_ino, facts.st_size, facts.st_mtime_ns, stat.S_IMODE(facts.st_mode), facts.st_uid, facts.st_gid) != original_identities[str(path)]:
                raise RuntimeError('Native original identity, size or modification time changed')
        if hashlib.sha256(binary.read_bytes()).hexdigest() != identity:
            raise RuntimeError('Profile executable changed')
        result['passed'] = True
    except Exception as error:
        result['failure'] = str(error)
        raise
    finally:
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
