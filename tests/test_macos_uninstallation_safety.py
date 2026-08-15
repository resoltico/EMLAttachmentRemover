"""Adversarial tests for macOS integration uninstallation transactions."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

from tests.macos_integration_support import (
    MARKER_BYTES,
    MARKER_NAME,
    POSIX_AVAILABLE,
    POSIX_REASON,
    UNINSTALLER,
)
from tests.macos_integration_support import command_override as _command_override
from tests.macos_integration_support import install as _install
from tests.macos_integration_support import run_script as _run


def _replace_entry(base: Path, entry: Path, entry_kind: str) -> None:
    """Replace a regular fixture file with a selected unsafe entry type."""
    entry.unlink()
    if entry_kind == "directory":
        entry.mkdir()
    elif entry_kind == "fifo":
        if not hasattr(os, "mkfifo"):
            message = "FIFO fixtures require os.mkfifo"
            raise RuntimeError(message)
        os.mkfifo(entry)
    else:
        victim = base / "public-victim"
        victim.write_bytes(b"MUST REMAIN\n")
        entry.symlink_to(victim)


@unittest.skipUnless(POSIX_AVAILABLE, POSIX_REASON)
class UninstallerHardeningTests(unittest.TestCase):
    """Require complete preflight and marker-preserving uninstall failures."""

    def test_uninstaller_preflights_every_managed_entry_type(self) -> None:
        for entry_name, entry_kind in (
            ("remove-eml-attachments.pyz", "directory"),
            ("run-from-finder.sh", "fifo"),
            ("run-from-finder.sh", "symlink"),
        ):
            with self.subTest(entry=entry_name, kind=entry_kind):
                with tempfile.TemporaryDirectory() as directory:
                    base = Path(directory)
                    installation, _zipapp, environment = _install(base)
                    entry = installation / entry_name
                    _replace_entry(base, entry, entry_kind)
                    result = _run(UNINSTALLER, environment)
                    self.assertEqual(result.returncode, 4)
                    self.assertEqual(
                        (installation / MARKER_NAME).read_bytes(), MARKER_BYTES
                    )
                    other_name = (
                        "run-from-finder.sh"
                        if entry_name == "remove-eml-attachments.pyz"
                        else "remove-eml-attachments.pyz"
                    )
                    self.assertTrue((installation / other_name).is_file())

    def test_uninstaller_accepts_an_absent_managed_payload(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            installation, _zipapp, environment = _install(Path(directory))
            (installation / "run-from-finder.sh").unlink()
            result = _run(UNINSTALLER, environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(installation.exists())

    def test_payload_removal_failure_preserves_the_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _install(base)
            command_directory = _command_override(
                base,
                "rm",
                'case "$*" in *run-from-finder.sh*) exit 75 ;; esac\nexec /bin/rm "$@"',
            )
            environment["PATH"] = (
                f"{command_directory}{os.pathsep}{environment['PATH']}"
            )
            result = _run(UNINSTALLER, environment)
            self.assertEqual(result.returncode, 4)
            self.assertEqual((installation / MARKER_NAME).read_bytes(), MARKER_BYTES)
            self.assertFalse((installation / "remove-eml-attachments.pyz").exists())
            self.assertTrue((installation / "run-from-finder.sh").is_file())

    def test_directory_removal_failure_restores_the_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _install(base)
            command_directory = _command_override(base, "rmdir", "exit 76")
            environment["PATH"] = (
                f"{command_directory}{os.pathsep}{environment['PATH']}"
            )
            result = _run(UNINSTALLER, environment)
            self.assertEqual(result.returncode, 4)
            self.assertIn("ownership marker was restored", result.stderr)
            self.assertEqual((installation / MARKER_NAME).read_bytes(), MARKER_BYTES)
            self.assertEqual(
                sorted(path.name for path in installation.iterdir()),
                [MARKER_NAME],
            )

    def test_directory_move_during_uninstall_restores_marker_in_original(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _install(base)
            moved = base / "public-moved-installation"
            command_directory = _command_override(
                base,
                "rm",
                'if [ ! -e "$EML_REMOVER_HOME.moved-once" ]; then\n'
                '    : > "$EML_REMOVER_HOME.moved-once"\n'
                '    mv "$EML_REMOVER_HOME" "$PUBLIC_MOVED_INSTALLATION"\n'
                '    mkdir "$EML_REMOVER_HOME"\n'
                "fi\n"
                'exec /bin/rm "$@"',
            )
            environment.update({
                "PATH": f"{command_directory}{os.pathsep}{environment['PATH']}",
                "PUBLIC_MOVED_INSTALLATION": str(moved),
            })
            result = _run(UNINSTALLER, environment)
            self.assertEqual(result.returncode, 4)
            self.assertIn("ownership marker was restored", result.stderr)
            self.assertEqual((moved / MARKER_NAME).read_bytes(), MARKER_BYTES)
            self.assertTrue(installation.is_dir())


if __name__ == "__main__":
    unittest.main(verbosity=2)
