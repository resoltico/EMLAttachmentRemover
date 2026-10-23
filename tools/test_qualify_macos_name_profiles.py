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
