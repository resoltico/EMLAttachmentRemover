#!/usr/bin/env python3
"""Independent existing-copy boundary fixture for one owned native Linux profile."""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess

CONTENT = b'Content-Type: text/plain\r\n\r\nExisting-only independent fixture.\r\n'
LEAF = 'message.7flrb7527orzd2eklbz7mwcunm.no-attachments.eml'


def inventory(root):
    result = []
    for path in [root] + sorted(root.rglob('*')):
        info = path.lstat()
        result.append({'name': str(path.relative_to(root)), 'device': info.st_dev,
                       'inode': info.st_ino, 'mode': stat.S_IMODE(info.st_mode),
                       'kind': stat.S_IFMT(info.st_mode), 'uid': info.st_uid, 'gid': info.st_gid,
                       'sha256': hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None})
    return result


def unattempted(observed):
    return observed['file_sync'] == {'method': 'not_requested', 'state': 'not_attempted', 'native_code': None} and observed['namespace_sync'] == {'method': 'not_requested', 'state': 'not_attempted', 'native_code': None, 'required_edges': 0, 'completed_edges': 0, 'failed_edge_index': None}


def qualify(name, item, exit_code, probe, require_sync_success):
    observation = item.get('existing_observation')
    if item['publication']['state'] != 'not_published' or item['publication']['file_sync']['state'] != 'not_attempted' or item['publication']['namespace_sync']['required_edges'] != 0 or item['publication']['cleanup']['state'] != 'not_needed':
        raise RuntimeError('Existing-only fixture attempted publication, cleanup or publication sync')
    if name == 'matching':
        if not observation or observation['bytes_equal'] is not True or observation['stable'] is not True:
            raise RuntimeError('Matching existing bytes were not independently compared')
        file_sync, namespace = observation['file_sync'], observation['namespace_sync']
        if item['status'] == 'up_to_date':
            if exit_code != 0 or item.get('error') is not None or file_sync['native_code'] is not None or namespace['native_code'] is not None or file_sync['method'] != ('fsync_fullfsync' if platform.system() == 'Darwin' else 'fsync') or file_sync['state'] != 'succeeded' or namespace['method'] != 'fsync' or namespace['state'] != 'succeeded' or namespace['required_edges'] != 1 or namespace['completed_edges'] != 1 or namespace['failed_edge_index'] is not None or observation['address'] != 'verified':
                raise RuntimeError('Existing success lacks actual file/endpoint/address receipts')
            return 'qualified'
        if require_sync_success:
            raise RuntimeError('Ordinary existing profile requires actual synchronization success')
        receipts = [file_sync, namespace]
        failed = [receipt for receipt in receipts if receipt['state'] in ['unsupported', 'failed']]
        if exit_code != 1 or item['status'] != 'failed' or item['error']['code'] != 'EXISTING_SYNC_FAILED' or not failed or any(receipt['method'] != 'fsync' or not isinstance(receipt['native_code'], int) for receipt in failed):
            raise RuntimeError('Existing synchronization refusal lacks an actual failed native receipt')
        if file_sync['state'] in ['unsupported', 'failed']:
            expected_namespace = {'method': 'not_requested', 'state': 'not_attempted', 'native_code': None, 'required_edges': 1, 'completed_edges': 0, 'failed_edge_index': None}
            if namespace != expected_namespace:
                raise RuntimeError('Endpoint sync was attempted after failed file sync')
        elif file_sync != {'method': 'fsync', 'state': 'succeeded', 'native_code': None} or namespace['state'] not in ['unsupported', 'failed'] or namespace['required_edges'] != 1 or namespace['completed_edges'] != 0 or namespace['failed_edge_index'] != 0:
            raise RuntimeError('Endpoint failure lacks the preceding successful file flush')
        expected_errors = [probe['file_errno'], probe['endpoint_errno']]
        unsupported = {errno.EINVAL, errno.ENOSYS, errno.ENOTSUP, errno.EOPNOTSUPP}
        if any(value is not None and value not in unsupported for value in expected_errors):
            raise RuntimeError('Independent flush failed with a cause other than unsupported operation')
        for receipt, expected_errno in zip(receipts, expected_errors):
            if receipt['state'] in ['unsupported', 'failed'] and receipt['native_code'] != expected_errno:
                raise RuntimeError('Actual failed receipt differs from independent path-bound fsync probe')
        if not any(expected_errors):
            raise RuntimeError('Unexpected product failure despite successful independent flushes')
        return 'native_sync_refused'
    if item['status'] != 'failed' or exit_code != 1:
        raise RuntimeError('Existing boundary did not fail honestly')
    if name == 'unequal':
        if item['error']['code'] != 'DESTINATION_EXISTS' or not observation or observation['bytes_equal'] is not False or not unattempted(observation):
            raise RuntimeError('Unequal existing bytes requested synchronization or lost comparison')
    elif item['error']['phase'] != 'admission' or item['error']['code'] != {'missing-route': 'DESTINATION_UNSUPPORTED', 'replaced-route': 'DESTINATION_EXISTS'}[name]:
        raise RuntimeError('Missing/replaced route did not retain its concrete admission cause')
    elif observation and not unattempted(observation):
        raise RuntimeError('Missing/replaced route requested existing synchronization')
    if item['publication']['state'] != 'not_published':
        raise RuntimeError('Existing-only fixture attempted publication')
    return 'refused_without_sync'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for field in ['binary', 'source-root', 'destination', 'out']:
        parser.add_argument('--' + field, type=Path, required=True)
    parser.add_argument('--require-sync-success', action='store_true')
    args = parser.parse_args()
    binary, source, destination, out = [p.resolve() for p in [args.binary, args.source_root, args.destination, args.out]]
    source.mkdir(mode=0o700)
    out.mkdir()
    if not destination.is_dir() or any(destination.iterdir()):
        raise RuntimeError('Existing fixture requires an empty owned destination')
    selected = source / 'Inbox'
    selected.mkdir()
    original = selected / 'message.eml'
    original.write_bytes(CONTENT)
    original_before = inventory(source)
    mirror = destination / 'Inbox.eml-dir'
    mirror.mkdir()
    leaf = mirror / LEAF
    leaf.write_bytes(CONTENT)
    # Deliberately refuse native namespace creation policy on every destination
    # directory. No unrelated owner mount/permission preference is changed.
    destination.chmod(0o777)
    mirror.chmod(0o777)
    def flush_probe(path):
        descriptor = os.open(path, os.O_RDONLY)
        try:
            try:
                os.fsync(descriptor)
                return None
            except OSError as error:
                return error.errno
        finally:
            os.close(descriptor)
    probe = {'file_errno': flush_probe(leaf), 'endpoint_errno': flush_probe(mirror),
             'file_identity': inventory(destination)[-1], 'endpoint_identity': inventory(destination)[1]}
    case_probe = destination / 'CaseProbe'
    case_probe.write_bytes(b'')
    case_mode = 'case-insensitive' if (destination / 'caseprobe').exists() else 'case-sensitive'
    case_probe.unlink()
    result = {'schema': 1, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'host': platform.uname()._asdict(), 'case_mode': case_mode, 'independent_sync_probe': probe,
              'require_sync_success': args.require_sync_success,
              'independent_leaf': LEAF, 'original_sha256': hashlib.sha256(CONTENT).hexdigest(),
              'creation_policy_refused': True, 'runs': [], 'failure': None, 'passed': False}
    try:
        for name in ['matching', 'unequal', 'missing-route', 'replaced-route']:
            if name == 'unequal':
                leaf.write_bytes(b'Independent unequal existing bytes.\n')
            elif name == 'missing-route':
                leaf.unlink()
                mirror.rmdir()
            elif name == 'replaced-route':
                mirror.write_bytes(b'Independent non-directory route replacement.\n')
            before = inventory(destination)
            process = subprocess.run([str(binary), '--format', 'json', '--output-dir', str(destination), '--', str(selected)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            (out / (name + '.jsonl')).write_bytes(process.stdout)
            (out / (name + '.stderr')).write_bytes(process.stderr)
            rows = [json.loads(line) for line in process.stdout.splitlines()]
            items = [row for row in rows if row.get('type') == 'item']
            if len(rows) != 3 or len(items) != 1 or rows[0].get('type') != 'start' or rows[0].get('total') != 1 or rows[0].get('collection_complete') is not True or rows[-1].get('type') != 'summary' or rows[-1].get('exit_code') != process.returncode:
                raise RuntimeError('Existing fixture lacks one complete actual CLI observation')
            after = inventory(destination)
            if before != after or inventory(source) != original_before:
                raise RuntimeError('Existing-only operation changed namespace, permissions, identity or bytes')
            outcome = qualify(name, items[0], process.returncode, probe, args.require_sync_success)
            result['runs'].append({'case': name, 'exit_code': process.returncode, 'outcome': outcome,
                                   'inventory_unchanged': True, 'before': before, 'after': after,
                                   'item': items[0], 'transcript': name + '.jsonl'})
        if hashlib.sha256(binary.read_bytes()).hexdigest() != result['binary_sha256']:
            raise RuntimeError('Frozen binary changed during existing-only fixture')
        result['passed'] = True
    except Exception as error:
        result['failure'] = str(error)
        raise
    finally:
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
