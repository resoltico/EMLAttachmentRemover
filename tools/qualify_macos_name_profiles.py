#!/usr/bin/env python3
# Copyright 2026 Ervins Strauhmanis
# SPDX-License-Identifier: MPL-2.0
"""Qualify frozen CLI naming on four exclusively owned native disk images."""
import argparse
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import stat
import subprocess
import sys
import tempfile

PROFILES = [
    ('apfs-case-insensitive', 'APFS', 'apfs', False),
    ('apfs-case-sensitive', 'Case-sensitive APFS', 'apfs', True),
    ('hfs-case-insensitive', 'HFS+', 'hfs', False),
    ('hfs-case-sensitive', 'Case-sensitive HFS+', 'hfs', True),
]
DEVICE = re.compile(r'/dev/disk[0-9]+(?:s[0-9]+)*\Z')


class UnsettledCommand(RuntimeError):
    """A timed-out operation cannot authorize image detach or removal."""


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def stream_digest(stream):
    digest = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b''):
        digest.update(chunk)
    return digest.hexdigest()


def binary_identity(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    with os.fdopen(descriptor, 'rb') as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or not before.st_size:
            raise RuntimeError('Frozen CLI must be a nonempty regular file')
        digest = stream_digest(stream)
        after = os.fstat(stream.fileno())
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
        raise RuntimeError('Frozen CLI changed while establishing byte identity')
    return {'sha256': digest, 'byte_length': after.st_size, 'device': after.st_dev, 'inode': after.st_ino}


def command(arguments, directory, label, timeout):
    process = subprocess.Popen(arguments, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        # Retain uncertainty before waiting for actual direct-child reap. The
        # enclosing xtask owner retains the whole process tree if that stalls.
        write_json(directory / (label + '.process.json'), {'pid': process.pid, 'timed_out': True, 'reaped': False, 'exit_code': None})
        process.kill()
        stdout, stderr = process.communicate()
    (directory / (label + '.stdout')).write_bytes(stdout)
    (directory / (label + '.stderr')).write_bytes(stderr)
    write_json(directory / (label + '.process.json'), {'pid': process.pid, 'arguments': [str(value) for value in arguments], 'timed_out': timed_out, 'reaped': True, 'exit_code': process.returncode})
    if timed_out:
        raise UnsettledCommand('Timed-out ' + label + '; image lifetime remains unqualified')
    return process.returncode, stdout


def plist_command(arguments, directory, label):
    code, output = command(arguments, directory, label, 60)
    if code:
        raise RuntimeError(label + ' failed with exit ' + str(code))
    if len(output) > 4 * 1024 * 1024:
        raise RuntimeError(label + ' plist exceeds its bound')
    value = plistlib.loads(output)
    if not isinstance(value, dict):
        raise RuntimeError(label + ' did not return a dictionary')
    return value


def select_association(info, image, mount):
    images = info.get('images')
    if not isinstance(images, list):
        raise RuntimeError('Missing actual disk-image association inventory')
    if any(not isinstance(entry, dict) or not isinstance(entry.get('image-path'), str) for entry in images):
        raise RuntimeError('Unrecognized native image inventory entry')
    matching = [entry for entry in images if entry['image-path'] == str(image)]
    if not matching:
        return None
    if len(matching) != 1:
        raise RuntimeError('Ambiguous owned image association')
    entities = matching[0].get('system-entities')
    if not isinstance(entities, list):
        raise RuntimeError('Missing owned image device entities')
    mounted = [entry for entry in entities if isinstance(entry, dict) and entry.get('mount-point') == str(mount)]
    if len(mounted) != 1 or not isinstance(mounted[0].get('dev-entry'), str) or not DEVICE.fullmatch(mounted[0]['dev-entry']):
        raise RuntimeError('Owned image lacks one exact native mount/device association')
    # APFS can expose both backing and synthesized whole devices. Detach by the
    # exact documented mountpoint after rechecking this image association.
    return {'mount': str(mount), 'volume_device': mounted[0]['dev-entry'], 'devices': [entry['dev-entry'] for entry in entities if isinstance(entry, dict) and isinstance(entry.get('dev-entry'), str) and DEVICE.fullmatch(entry['dev-entry'])]}


def association(image, mount, directory, label):
    # Global image inventory can contain unrelated user paths. Only the owned
    # association and actual process facts belong in uploaded evidence.
    with tempfile.TemporaryDirectory(prefix='eml-private-image-inventory-') as private:
        private = Path(private)
        info = plist_command(['/usr/bin/hdiutil', 'info', '-plist'], private, label)
        process = json.loads((private / (label + '.process.json')).read_text())
        selected = select_association(info, image, mount)
        write_json(directory / (label + '.owned.json'), {'process': process, 'owned_image': str(image), 'association': selected})
        return selected


def same_owned_image(image, held):
    current = image.lstat()
    identity = os.fstat(held.fileno())
    if not stat.S_ISREG(current.st_mode) or current.st_uid != os.geteuid() or (current.st_dev, current.st_ino) != (identity.st_dev, identity.st_ino):
        raise RuntimeError('Owned image native identity changed')
    return {'device': identity.st_dev, 'inode': identity.st_ino, 'owner_uid': identity.st_uid, 'byte_length': identity.st_size}


def cleanup_image(image, mount, directory, held, expected):
    identity = same_owned_image(image, held)
    actual = association(image, mount, directory, 'cleanup-association')
    if actual is not None:
        if expected is not None and actual != expected:
            raise RuntimeError('Owned image device/mount association changed before cleanup')
        code, _ = command(['/usr/bin/hdiutil', 'detach', str(mount)], directory, 'detach', 60)
        if code:
            raise RuntimeError('Owned image detach failed with exit ' + str(code))
    if association(image, mount, directory, 'detached-association') is not None:
        raise RuntimeError('Owned image remains associated after detach')
    same_owned_image(image, held)
    image.unlink()
    return {'detached': True, 'owned_image_removed': True, 'forced_detach': False, 'image_identity': identity}


def case_observation(mount):
    directory = mount / 'case-observation'
    directory.mkdir(mode=0o700)
    upper, lower = directory / 'CaseProbe', directory / 'caseprobe'
    with upper.open('xb') as stream:
        stream.write(b'owned upper case probe\n')
    first = upper.stat()
    try:
        with lower.open('xb') as stream:
            stream.write(b'owned lower case probe\n')
    except FileExistsError as error:
        if error.errno != errno.EEXIST or lower.read_bytes() != upper.read_bytes() or (lower.stat().st_dev, lower.stat().st_ino) != (first.st_dev, first.st_ino):
            raise RuntimeError('Case-insensitive refusal did not preserve the held native object') from error
        sensitive = False
    else:
        second = lower.stat()
        if (second.st_dev, second.st_ino) == (first.st_dev, first.st_ino) or upper.read_bytes() != b'owned upper case probe\n' or lower.read_bytes() != b'owned lower case probe\n':
            raise RuntimeError('Case-sensitive entries did not retain distinct native objects')
        sensitive = True
    return {'case_sensitive': sensitive, 'native_entries': [{'name': entry.name, 'native_hex': os.fsencode(entry.name).hex(), 'device': entry.stat().st_dev, 'inode': entry.stat().st_ino} for entry in sorted(directory.iterdir())]}


def volume_observation(mount, attached, directory, expected_filesystem, expected_sensitive):
    info = plist_command(['/usr/sbin/diskutil', 'info', '-plist', str(mount)], directory, 'native-volume')
    if info.get('DeviceNode') != attached['volume_device'] or info.get('MountPoint') != str(mount) or info.get('FilesystemType') != expected_filesystem or info.get('GlobalPermissionsEnabled') is not True:
        raise RuntimeError('Actual mounted local image filesystem/device differs from requested profile')
    observed = case_observation(mount)
    if observed['case_sensitive'] != expected_sensitive:
        raise RuntimeError('Observed native case mode differs from requested profile')
    return {'filesystem_type': info['FilesystemType'], 'filesystem_name': info.get('FilesystemName'), 'device': info['DeviceNode'], 'mount': info['MountPoint'], 'case': observed, 'ownership_enabled': info['GlobalPermissionsEnabled'], 'locality': 'actual owned hdiutil device association and matching diskutil native volume; no server marketing-name inference'}


def capture_entries(directory):
    result = []
    if not directory.is_dir():
        return result
    for parent, directories, files in os.walk(directory, followlinks=False):
        for name in sorted(directories + files):
            path = Path(parent) / name
            observed = path.lstat()
            if len(result) >= 1024:
                raise RuntimeError('Owned fixture entry inventory exceeds its finite bound')
            entry = {'relative': str(path.relative_to(directory)), 'native_hex': os.fsencode(str(path.relative_to(directory))).hex(), 'device': observed.st_dev, 'inode': observed.st_ino, 'bytes': observed.st_size, 'mode': observed.st_mode}
            if stat.S_ISREG(observed.st_mode):
                if observed.st_size > 16 * 1024 * 1024:
                    raise RuntimeError('Owned fixture file unexpectedly exceeds its bounded size')
                with path.open('rb') as stream:
                    entry['sha256'] = stream_digest(stream)
            result.append(entry)
    return result


def qualify_profile(binary, identity, profile, out):
    profile_id, filesystem, native_filesystem, sensitive = profile
    directory = out / profile_id
    directory.mkdir(mode=0o700)
    image, mount = directory / 'owned-image.dmg', directory / 'mount'
    mount.mkdir(mode=0o700)
    result = {'profile': profile_id, 'requested_filesystem': filesystem, 'status': 'failed_or_unavailable', 'exit_code': None, 'behavior': None, 'volume': None, 'cleanup': None, 'failure': None}
    held, attached, uncertain = None, None, False
    try:
        code, _ = command(['/usr/bin/hdiutil', 'create', '-size', '256m', '-fs', filesystem, '-volname', 'EMLNames', str(image)], directory, 'create', 60)
        if image.exists():
            descriptor = os.open(image, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            held = os.fdopen(descriptor, 'rb')
            result['image_identity'] = same_owned_image(image, held)
        if code or held is None:
            raise RuntimeError('Requested native profile could not be created; exit ' + str(code))
        plist_command(['/usr/bin/hdiutil', 'attach', '-nobrowse', '-noautoopen', '-owners', 'on', '-mountpoint', str(mount), '-plist', str(image)], directory, 'attach')
        attached = association(image, mount, directory, 'attached-association')
        if attached is None:
            raise RuntimeError('Image attach did not establish owned mount/device association')
        result['volume'] = volume_observation(mount, attached, directory, native_filesystem, sensitive)
        destination = mount / 'destination'
        destination.mkdir(mode=0o700)
        code, _ = command([sys.executable, str(Path(__file__).with_name('check_name_profile.py')), '--binary', str(binary), '--source-root', str(directory / 'sources'), '--destination', str(destination), '--out', str(directory / 'behavior')], directory, 'behavior', 180)
        result['exit_code'] = code
        behavior = directory / 'behavior/result.json'
        if behavior.is_file():
            result['behavior'] = json.loads(behavior.read_text())
        if code or result['behavior'] is None or result['behavior'].get('passed') is not True or result['behavior'].get('binary_sha256') != identity['sha256']:
            raise RuntimeError('Actual frozen CLI naming behavior failed or lacks complete evidence')
        if binary_identity(binary) != identity:
            raise RuntimeError('Frozen CLI identity changed during profile execution')
        result['status'] = 'passed'
    except UnsettledCommand as error:
        uncertain = True
        result['failure'] = str(error)
    except Exception as error:
        result['failure'] = str(error)
    finally:
        if not uncertain:
            try:
                result['source_entries'] = capture_entries(directory / 'sources')
                result['destination_entries'] = capture_entries(mount / 'destination')
            except Exception as error:
                result['capture_failure'] = str(error)
                result['status'] = 'failed_or_unavailable'
            if held is not None:
                try:
                    result['cleanup'] = cleanup_image(image, mount, directory, held, attached)
                except Exception as error:
                    result['cleanup_failure'] = str(error)
                    result['status'] = 'failed_or_unavailable'
        else:
            result['cleanup_failure'] = 'Timed-out operation; retain owned image without guessing detach/removal authority'
        if held is not None:
            held.close()
        write_json(directory / 'result.json', result)
    return result, uncertain or result.get('cleanup_failure') is not None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('out', type=Path)
    args = parser.parse_args()
    args.out.mkdir(mode=0o700)
    out, binary = args.out.resolve(), args.binary.absolute()
    result = {'schema': 1, 'binary_sha256': None, 'profiles': [], 'passed': False, 'host': {'os': platform.system(), 'os_version': platform.mac_ver()[0], 'architecture': platform.machine(), 'kernel_release': platform.release()}, 'failure': None}
    try:
        identity = binary_identity(binary)
        result['binary_sha256'] = identity['sha256']
        result['binary_identity'] = identity
        if platform.system() != 'Darwin' or os.geteuid() == 0:
            raise RuntimeError('Native owned macOS name profiles require ordinary-user Darwin execution')
        uncertain = False
        for profile in PROFILES:
            if uncertain:
                result['profiles'].append({'profile': profile[0], 'status': 'failed_or_unavailable', 'exit_code': None, 'behavior': None, 'failure': 'Earlier image/command lifetime remains uncertain; no further image operation admitted'})
            else:
                observed, uncertain = qualify_profile(binary, identity, profile, out)
                result['profiles'].append(observed)
            write_json(out / 'result.json', result)
        if binary_identity(binary) != identity:
            raise RuntimeError('Frozen CLI changed after the profile campaign')
        result['passed'] = len(result['profiles']) == len(PROFILES) and all(row['status'] == 'passed' and row['exit_code'] == 0 and row['cleanup']['detached'] and row['cleanup']['owned_image_removed'] for row in result['profiles'])
    except Exception as error:
        result['failure'] = str(error)
        observed_ids = {row['profile'] for row in result['profiles']}
        result['profiles'].extend({'profile': profile[0], 'status': 'failed_or_unavailable', 'exit_code': None, 'behavior': None, 'failure': 'Required profile unavailable: ' + str(error)} for profile in PROFILES if profile[0] not in observed_ids)
    finally:
        write_json(out / 'result.json', result)
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
