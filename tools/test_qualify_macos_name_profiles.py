# Copyright 2026 Ervins Strauhmanis
# SPDX-License-Identifier: MPL-2.0
"""Ownership/cleanup rejection controls; no mounted image or CLI run is mocked into a pass."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('macos_name_profiles', Path(__file__).with_name('qualify_macos_name_profiles.py'))
PROFILE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROFILE)


@unittest.skipUnless(os.name == 'posix', 'Owned hdiutil image controls require POSIX paths, uid and held-file native identity; Windows is not a macOS qualification host')
class OwnedImageTests(unittest.TestCase):
    def entry(self, image='/owned/image.dmg', mount='/owned/mount'):
        return {'image-path': image, 'system-entities': [{'dev-entry': '/dev/disk71'}, {'dev-entry': '/dev/disk72'}, {'dev-entry': '/dev/disk72s1', 'mount-point': mount}]}

    def test_apfs_synthesized_devices_require_one_exact_owned_image_and_mount(self):
        image, mount = Path('/owned/image.dmg'), Path('/owned/mount')
        observed = PROFILE.select_association({'images': [self.entry()]}, image, mount)
        self.assertEqual(observed['volume_device'], '/dev/disk72s1')
        self.assertEqual(len(observed['devices']), 3)
        self.assertIsNone(PROFILE.select_association({'images': []}, image, mount))
        self.assertIsNone(PROFILE.select_association({'images': [self.entry('/other/image.dmg')]}, image, mount))
        for value in [
            {}, {'images': ['unknown']}, {'images': [{}]},
            {'images': [self.entry(), self.entry()]},
            {'images': [self.entry(mount='/other/mount')]},
            {'images': [{'image-path': str(image), 'system-entities': [{'dev-entry': '/dev/other', 'mount-point': str(mount)}]}]},
        ]:
            with self.subTest(value=value), self.assertRaises(RuntimeError):
                PROFILE.select_association(value, image, mount)

    def test_reassociated_device_never_reaches_detach_executor(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            image = root / 'image.dmg'
            image.write_bytes(b'owned image')
            with image.open('rb') as held, patch.object(PROFILE, 'association', return_value={'mount': '/owned/mount', 'volume_device': '/dev/disk99', 'devices': ['/dev/disk99']}), patch.object(PROFILE, 'command') as execute:
                with self.assertRaises(RuntimeError):
                    PROFILE.cleanup_image(image, root / 'mount', root, held, {'mount': '/owned/mount', 'volume_device': '/dev/disk71', 'devices': ['/dev/disk71']})
                execute.assert_not_called()
                self.assertTrue(image.is_file())

    def test_busy_detach_is_terminal_and_diagnostic_failure_cannot_mask_it(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            image = root / 'image.dmg'
            image.write_bytes(b'owned image')
            association = {'mount': str(root / 'mount'), 'volume_device': '/dev/disk71', 'devices': ['/dev/disk71']}
            with image.open('rb') as held, patch.object(PROFILE, 'association', return_value=association), patch.object(PROFILE, 'command', return_value=(16, b'')) as execute, patch.object(PROFILE, 'observe_busy_volume', side_effect=RuntimeError('diagnostic unavailable')) as observe:
                with self.assertRaisesRegex(RuntimeError, 'detach failed with exit 16'):
                    PROFILE.cleanup_image(image, root / 'mount', root, held, association)
                self.assertEqual(execute.call_count, 1)
                self.assertEqual(execute.call_args.args[0], ['/usr/bin/hdiutil', 'detach', str(root / 'mount')])
                observe.assert_called_once_with(image, root / 'mount', root, held, association)
                self.assertEqual(image.read_bytes(), b'owned image')

    def test_busy_observation_retains_only_owned_relative_paths_and_correlated_processes(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            mount = root / 'mount'
            def execute(arguments, private, label, timeout):
                self.assertEqual(timeout, 10)
                self.assertIn('/dev/disk71s1', arguments)
                PROFILE.write_json(private / (label + '.process.json'), {'pid': 99, 'exit_code': 0})
                if label == 'holders':
                    self.assertEqual(arguments[:5], ['/usr/sbin/lsof', '-nP', '-Fpcfn', '+f', '--'])
                    return 0, ('p1234\ncobserved-agent\nf4r\nn' + str(mount) + '/destination/copy.eml\nf5r\nn/unrelated/private-file\n').encode()
                return 0, PROFILE.plistlib.dumps({'MountPoint': str(mount), 'DeviceNode': '/dev/disk71s1', 'FilesystemType': 'hfs', 'UnrelatedSecret': '/private/unrelated'})
            binding = {'mount': str(mount), 'volume_device': '/dev/disk71s1', 'devices': ['/dev/disk71s1']}
            with patch.object(PROFILE, 'same_owned_image'), patch.object(PROFILE, 'association', return_value=binding), patch.object(PROFILE, 'command', side_effect=execute):
                PROFILE.observe_busy_volume(root / 'owned.dmg', mount, root, None, binding)
            text = (root / 'busy-volume-observation.json').read_text()
            self.assertNotIn('/unrelated', text)
            self.assertNotIn('1234', text)
            self.assertNotIn('UnrelatedSecret', text)
            observed = PROFILE.json.loads(text)
            self.assertEqual(observed['commands'][1]['holders'][0]['files'], ['destination/copy.eml', '<unrecognized-name-redacted>'])
            self.assertEqual(observed['commands'][1]['holders'][0]['command_basename'], 'observed-agent')

    def test_missing_or_changed_mount_never_runs_holder_scan(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            binding = {'mount': str(root / 'mount'), 'volume_device': '/dev/disk71s1', 'devices': ['/dev/disk71s1']}
            for actual in [None, dict(binding, volume_device='/dev/disk99s1')]:
                with self.subTest(actual=actual), patch.object(PROFILE, 'same_owned_image'), patch.object(PROFILE, 'association', return_value=actual), patch.object(PROFILE, 'command') as execute:
                    PROFILE.observe_busy_volume(root / 'owned.dmg', root / 'mount', root, None, binding)
                    execute.assert_not_called()
                    report = PROFILE.json.loads((root / 'busy-volume-observation.json').read_text())
                    self.assertIn('unavailable', report)
            # A mountpoint resolving to the parent filesystem must also stop
            # before lsof even when the earlier image inventory still matched.
            def parent_volume(arguments, private, label, timeout):
                self.assertEqual(label, 'volume')
                PROFILE.write_json(private / (label + '.process.json'), {'pid': 99, 'exit_code': 0})
                return 0, PROFILE.plistlib.dumps({'MountPoint': '/', 'DeviceNode': '/dev/disk1s1'})
            with patch.object(PROFILE, 'same_owned_image'), patch.object(PROFILE, 'association', return_value=binding), patch.object(PROFILE, 'command', side_effect=parent_volume) as execute:
                PROFILE.observe_busy_volume(root / 'owned.dmg', root / 'mount', root, None, binding)
                self.assertEqual(execute.call_count, 1)
                report = PROFILE.json.loads((root / 'busy-volume-observation.json').read_text())
                self.assertEqual(len(report['commands']), 1)
                self.assertIsNotNone(report['commands'][0]['failure'])

    def test_replaced_image_never_authorizes_detach_or_deletion(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            image = root / 'image.dmg'
            image.write_bytes(b'owned image')
            with image.open('rb') as held:
                image.rename(root / 'held-image.dmg')
                image.write_bytes(b'unrelated replacement')
                with patch.object(PROFILE, 'command') as execute, self.assertRaises(RuntimeError):
                    PROFILE.cleanup_image(image, root / 'mount', root, held, None)
                execute.assert_not_called()
                self.assertEqual(image.read_bytes(), b'unrelated replacement')

    def test_case_probe_records_actual_native_entries_and_distinct_or_shared_identity(self):
        with tempfile.TemporaryDirectory() as root:
            observed = PROFILE.case_observation(Path(root))
            self.assertIn(observed['case_sensitive'], [True, False])
            self.assertEqual(len(observed['native_entries']), 2 if observed['case_sensitive'] else 1)
            self.assertTrue(all(entry['native_hex'] and entry['inode'] for entry in observed['native_entries']))

    def test_noowners_volume_is_rejected_before_case_or_product_execution(self):
        native = {'DeviceNode': '/dev/disk72s1', 'MountPoint': '/owned/mount', 'FilesystemType': 'apfs', 'GlobalPermissionsEnabled': False}
        with patch.object(PROFILE, 'plist_command', return_value=native), patch.object(PROFILE, 'case_observation') as probe:
            with self.assertRaises(RuntimeError):
                PROFILE.volume_observation(Path('/owned/mount'), {'volume_device': '/dev/disk72s1'}, Path('/evidence'), 'apfs', False)
            probe.assert_not_called()

    def test_missing_native_environment_records_every_required_profile_as_unqualified(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            binary = root / 'binary'
            binary.write_bytes(b'owned placeholder, never executed')
            out = root / 'result'
            with patch.object(PROFILE.sys, 'argv', ['qualify_macos_name_profiles.py', str(binary), str(out)]), patch.object(PROFILE.platform, 'system', return_value='unsupported'), patch.object(PROFILE, 'command') as execute:
                self.assertEqual(PROFILE.main(), 1)
                execute.assert_not_called()
            result = PROFILE.json.loads((out / 'result.json').read_text())
            self.assertFalse(result['passed'])
            self.assertEqual(len(result['profiles']), 4)
            self.assertTrue(all(row['status'] == 'failed_or_unavailable' and row['behavior'] is None for row in result['profiles']))

    def test_profile_set_is_the_fixed_required_four(self):
        self.assertEqual([profile[0] for profile in PROFILE.PROFILES], ['apfs-case-insensitive', 'apfs-case-sensitive', 'hfs-case-insensitive', 'hfs-case-sensitive'])
        self.assertEqual([profile[1] for profile in PROFILE.PROFILES], ['APFS', 'Case-sensitive APFS', 'HFS+', 'Case-sensitive HFS+'])


if __name__ == '__main__':
    unittest.main()
