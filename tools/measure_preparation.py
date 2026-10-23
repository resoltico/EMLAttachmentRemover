#!/usr/bin/env python3
"""Finite Linux qualification against the exact frozen CLI; no source payload leaves the fixture."""
from __future__ import annotations
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import struct
import subprocess
import sys

MIB = 1024 * 1024
LIMIT = 256 * MIB

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(MIB), b''):
            value.update(block)
    return value.hexdigest()

def fixture(path, wrappers, body_bytes=None, total=None):
    boundaries = [f'level_{index}_end'.encode('ascii') for index in range(wrappers)]
    prefix = b''.join(b'Content-Type: multipart/mixed; boundary=' + b + b'\r\n\r\n--' + b + b'\r\n' for b in reversed(boundaries)) + b'Content-Type: text/plain\r\n\r\n'
    suffix = b''.join(b'\r\n--' + b + b'--\r\n' for b in boundaries)
    size = total - len(prefix) - len(suffix) if total is not None else body_bytes
    if size is None or size < 0 or size + len(prefix) + len(suffix) > LIMIT:
        raise ValueError('Invalid finite resource fixture size')
    block = (b'x' * 126 + b'\r\n') * 512
    with path.open('xb') as stream:
        stream.write(prefix)
        remaining = size
        while remaining:
            part = block[:min(remaining, len(block))]
            stream.write(part)
            remaining -= len(part)
        stream.write(suffix)
    # Independent seed implementation from the written grammar. Preseeding an
    # exact matching copy supplies an independent full-byte candidate oracle via
    # the CLI's existing comparison without measuring publication/sync latency.
    human = path.stem
    logical = (human + '.no-attachments.eml').encode('utf-8')
    seed = b'eml-attachment-remover:destination-route:v1\0' + struct.pack('<I', 1) + bytes([2, 0]) + struct.pack('<I', len(logical)) + logical
    token = base64.b32encode(hashlib.sha256(seed).digest()[:16]).decode('ascii').lower().rstrip('=')
    expected = path.with_name(f'{human}.{token}.no-attachments.eml')
    with path.open('rb') as source, expected.open('xb') as output:
        shutil.copyfileobj(source, output, MIB)
    identity = digest(path)
    if digest(expected) != identity:
        raise RuntimeError('Independent matching fixture copy differs')
    return {'path': str(path), 'expected': str(expected), 'raw_bytes': path.stat().st_size, 'body_bytes': size, 'sha256': identity, 'wrappers': wrappers}

def measure(binary, item, name, directory):
    timing = directory / (name + '.time')
    stdout = directory / (name + '.jsonl')
    stderr = directory / (name + '.stderr')
    with stdout.open('xb') as output, stderr.open('xb') as error:
        # The enclosing xtask controller owns this inherited process group and
        # deadline. GNU time waits/reaps the real CLI; no detached session starts.
        process = subprocess.run(['/usr/bin/time', '-f', '%e %U %S %M', '-o', str(timing), str(binary), '--dry-run', '--format', 'json', '--', item['path']], stdout=output, stderr=error, check=False, env={**os.environ, 'LC_ALL': 'C'})
    result = {'name': name, 'exit_code': process.returncode, 'timing_file': str(timing), 'stdout': str(stdout), 'stderr': str(stderr)}
    if process.returncode:
        raise RuntimeError(f'Resource preparation failed: {result}')
    elapsed, user, system, rss = timing.read_text().strip().split()
    result.update(elapsed_seconds=float(elapsed), user_seconds=float(user), system_seconds=float(system), max_rss_kib=int(rss))
    records = [json.loads(line) for line in stdout.read_text().splitlines()]
    items = [row for row in records if row.get('type') == 'item']
    summaries = [row for row in records if row.get('type') == 'summary']
    if len(records) != 3 or len(items) != 1 or len(summaries) != 1 or records[0].get('type') != 'start' or records[0].get('protocol') != 1 or records[0].get('dry_run') is not True or records[0].get('total') != 1 or records[0].get('collection_complete') is not True:
        raise RuntimeError('Resource stream lacks its actual item and terminal summary')
    row, summary = items[0], summaries[0]
    existing = row.get('existing_observation')
    if row.get('index') != 0 or row.get('status') != 'planned' or row.get('plan') != 'verify_existing' or row.get('source_bytes') != item['raw_bytes'] or row.get('copy_bytes') != item['raw_bytes'] or row.get('removed') != [] or row.get('stripped_fields') != [] or not existing or existing.get('bytes_equal') is not True or existing.get('stable') is not True or summary.get('exit_code') != 0 or summary.get('counts', {}).get('planned') != 1:
        raise RuntimeError('Independent exact existing-copy preparation oracle did not qualify')
    if digest(Path(item['path'])) != item['sha256'] or digest(Path(item['expected'])) != item['sha256']:
        raise RuntimeError('Resource source or independent expected bytes changed')
    result['exact_existing_bytes_matched'] = True
    return result

def main():
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise RuntimeError('The required controlled resource campaign is Linux x86_64 only')
    binary, directory, source = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    directory.mkdir()
    record = {'source': source, 'binary_sha256': digest(binary), 'binary_version': subprocess.check_output([str(binary), '--version'], text=True).strip(), 'host': platform.uname()._asdict(), 'python': sys.version, 'time_version': subprocess.check_output(['/usr/bin/time', '--version'], text=True).splitlines()[0], 'filesystem': subprocess.check_output(['stat', '-f', '-c', '%T', str(directory)], text=True).strip(), 'case_mode': None, 'fixtures': {}, 'runs': [], 'failure': None, 'passed': False, 'scope': 'Five interleaved release CLI shallow/deep preparations plus one exact-limit preparation; independently preseeded matching bytes prove candidate identity, no publication or sync is timed.'}
    probe = directory / 'CaseProbe'
    probe.write_bytes(b'')
    try:
        with (directory / 'caseprobe').open('xb'):
            pass
        record['case_mode'] = 'case-sensitive'
    except FileExistsError:
        record['case_mode'] = 'case-insensitive'
    probe.unlink()
    (directory / 'caseprobe').unlink(missing_ok=True)
    try:
        shallow = fixture(directory / 'shallow.eml', 1, body_bytes=32*MIB)
        deep = fixture(directory / 'deep.eml', 63, body_bytes=32*MIB)
        record['fixtures'].update(shallow=shallow, deep=deep)
        for iteration in range(5):
            for key, item in [('shallow', shallow), ('deep', deep)]:
                row = measure(binary, item, f'{key}-{iteration+1}', directory)
                row['fixture'] = key
                record['runs'].append(row)
                (directory / 'measurements.json').write_text(json.dumps(record, indent=2) + '\n')
        shallow_median = statistics.median(row['elapsed_seconds'] for row in record['runs'] if row['fixture'] == 'shallow')
        deep_median = statistics.median(row['elapsed_seconds'] for row in record['runs'] if row['fixture'] == 'deep')
        record['medians'] = {'shallow_seconds': shallow_median, 'deep_seconds': deep_median, 'maximum_deep_seconds': 3*shallow_median+0.2}
        if deep_median > 3*shallow_median+0.2:
            raise RuntimeError('Deep median exceeds the required relative time budget')
        maximum = fixture(directory / 'limit.eml', 63, total=LIMIT)
        record['fixtures']['limit'] = maximum
        measured = measure(binary, maximum, 'limit', directory)
        measured['fixture'] = 'limit'
        record['runs'].append(measured)
        record['rss_limit_bytes'] = 2*maximum['raw_bytes'] + 96*MIB
        if measured['max_rss_kib']*1024 > record['rss_limit_bytes']:
            raise RuntimeError('Exact admitted message exceeds the required RSS envelope')
        if digest(binary) != record['binary_sha256']:
            raise RuntimeError('Measured binary changed')
        record['passed'] = True
    except Exception as error:
        record['failure'] = str(error)
        raise
    finally:
        (directory / 'measurements.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps({'passed': record['passed'], 'source': source, 'binary_sha256': record['binary_sha256'], 'runs': len(record['runs']), 'medians': record['medians'], 'rss_limit_bytes': record['rss_limit_bytes']}, sort_keys=True))

if __name__ == '__main__':
    main()
