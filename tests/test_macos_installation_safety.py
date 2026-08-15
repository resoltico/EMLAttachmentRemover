"""Adversarial tests for macOS integration installation transactions."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tests.macos_integration_support import (
    INSTALLER,
    INSTALLER_SUPPORT,
    INTEGRATION_ROOT,
    MARKER_BYTES,
    MARKER_NAME,
    POSIX_AVAILABLE,
    POSIX_REASON,
    POSIX_SHELL,
)
from tests.macos_integration_support import (
    command_override as _command_override,
)
from tests.macos_integration_support import (
    fixture as _fixture,
)
from tests.macos_integration_support import (
    install as _install,
)
from tests.macos_integration_support import (
    integration_environment as _environment,
)
from tests.macos_integration_support import (
    run_script as _run,
)
from tests.test_support import SUBPROCESS_TIMEOUT_SECONDS


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
class InstallerHardeningTests(unittest.TestCase):
    """Require exact owned entries and failure-safe installer publication."""

    def test_installer_rejects_nonregular_managed_destinations(self) -> None:
        for entry_name, entry_kind in (
            ("remove-eml-attachments.pyz", "directory"),
            ("run-from-finder.sh", "fifo"),
        ):
            with self.subTest(entry=entry_name, kind=entry_kind):
                with tempfile.TemporaryDirectory() as directory:
                    installation, _zipapp, environment = _install(Path(directory))
                    entry = installation / entry_name
                    _replace_entry(Path(directory), entry, entry_kind)

                    result = _run(INSTALLER, environment)

                    self.assertEqual(result.returncode, 4)
                    self.assertTrue((installation / MARKER_NAME).is_file())
                    self.assertEqual(entry.is_dir(), entry_kind == "directory")

    def test_installer_requires_the_marker_bytes_to_be_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            installation, _zipapp, environment = _install(Path(directory))
            marker = installation / MARKER_NAME
            marker.write_bytes(MARKER_BYTES + b"unexpected\n")
            original_zipapp = (installation / "remove-eml-attachments.pyz").read_bytes()

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("exact installation marker", result.stderr)
            self.assertEqual(marker.read_bytes(), MARKER_BYTES + b"unexpected\n")
            self.assertEqual(
                (installation / "remove-eml-attachments.pyz").read_bytes(),
                original_zipapp,
            )

    def test_installer_rejects_unknown_hidden_entries_before_staging(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            installation, _zipapp, environment = _install(Path(directory))
            unknown = installation / ".public-unknown"
            unknown.write_bytes(b"MUST REMAIN\n")

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("unknown entries", result.stderr)
            self.assertEqual(unknown.read_bytes(), b"MUST REMAIN\n")
            self.assertEqual(
                sorted(path.name for path in installation.iterdir()),
                [
                    ".eml-attachment-remover-installation",
                    ".public-unknown",
                    "remove-eml-attachments.pyz",
                    "run-from-finder.sh",
                ],
            )

    def test_new_install_cleanup_removes_all_staging_after_copy_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _fixture(base)
            proxy = base / "public-copy-failure-proxy"
            proxy.write_text(
                "#!/bin/sh\n"
                'case "${2:-}" in *shutil.copyfileobj*) exit 73 ;; esac\n'
                f'exec {sys.executable!s} "$@"\n',
                encoding="utf-8",
            )
            proxy.chmod(0o755)
            environment["EML_REMOVER_PYTHON"] = str(proxy)

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 73)
            self.assertFalse(installation.exists())

    def test_new_install_cleanup_removes_directory_after_staging_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _fixture(base)
            command_directory = _command_override(base, "mktemp", "exit 77")
            environment["PATH"] = (
                f"{command_directory}{os.pathsep}{environment['PATH']}"
            )

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 7)
            self.assertIn("Could not stage", result.stderr)
            self.assertFalse(installation.exists())

    def test_installer_rejects_a_selected_zipapp_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, zipapp, environment = _fixture(base)
            selected = base / "public-selected-link.pyz"
            selected.symlink_to(zipapp)
            environment["EML_REMOVER_ZIPAPP"] = str(selected)

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 3)
            self.assertIn("safe regular file", result.stderr)
            self.assertFalse(installation.exists())

    def test_installer_rejects_a_symbolic_link_support_library(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            isolated_root = base / "public-project"
            integration = isolated_root / "integrations/macos-shortcuts"
            integration.mkdir(parents=True)
            isolated_installer = integration / "install.sh"
            shutil.copyfile(INSTALLER, isolated_installer)
            (integration / "installer-filesystem.sh").symlink_to(INSTALLER_SUPPORT)
            (integration / "run-from-finder.sh").write_text(
                "#!/bin/sh\nexit 0\n",
                encoding="utf-8",
            )
            installation, _zipapp, environment = _fixture(base)

            result = _run(isolated_installer, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("support library", result.stderr)
            self.assertFalse(installation.exists())

    def test_default_zipapp_is_rebuilt_even_when_a_stale_build_exists(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            isolated_root = base / "public-project"
            integration = isolated_root / "integrations/macos-shortcuts"
            integration.mkdir(parents=True)
            shutil.copyfile(INSTALLER, integration / "install.sh")
            shutil.copyfile(
                INSTALLER_SUPPORT,
                integration / "installer-filesystem.sh",
            )
            shutil.copyfile(
                INTEGRATION_ROOT / "run-from-finder.sh",
                integration / "run-from-finder.sh",
            )
            tools = isolated_root / "tools"
            tools.mkdir()
            (tools / "build_zipapp.py").write_text(
                "from pathlib import Path\n"
                "import sys\n"
                "target = Path(sys.argv[sys.argv.index('--target') + 1])\n"
                "target.parent.mkdir(parents=True, exist_ok=True)\n"
                "target.write_bytes(b'FRESH ZIPAPP\\n')\n",
                encoding="utf-8",
            )
            stale = isolated_root / "build/remove-eml-attachments.pyz"
            stale.parent.mkdir()
            stale.write_bytes(b"STALE ZIPAPP\n")
            installation, _zipapp, environment = _fixture(base)
            environment.pop("EML_REMOVER_ZIPAPP")

            result = _run(integration / "install.sh", environment)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(stale.read_bytes(), b"FRESH ZIPAPP\n")
            self.assertEqual(
                (installation / "remove-eml-attachments.pyz").read_bytes(),
                b"FRESH ZIPAPP\n",
            )

    def test_failed_reinstall_restores_exact_ownership_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _install(base)
            proxy = base / "public-python-proxy"
            proxy.write_text(
                "#!/bin/sh\n"
                'if [ ! -e "$PUBLIC_FAILURE_SENTINEL" ]; then\n'
                '    case "${4:-}" in\n'
                "        */run-from-finder.sh)\n"
                '            case "${2:-}" in\n'
                "                *os.replace*)\n"
                '                    : > "$PUBLIC_FAILURE_SENTINEL"\n'
                "                    exit 74\n"
                "                    ;;\n"
                "            esac\n"
                "            ;;\n"
                "    esac\n"
                "fi\n"
                f'exec {sys.executable!s} "$@"\n',
                encoding="utf-8",
            )
            proxy.chmod(0o755)
            environment["EML_REMOVER_PYTHON"] = str(proxy)
            environment["PUBLIC_FAILURE_SENTINEL"] = str(
                base / "public-failure-sentinel"
            )
            original_zipapp = (installation / "remove-eml-attachments.pyz").read_bytes()
            original_runner = (installation / "run-from-finder.sh").read_bytes()
            (base / "public-archive.pyz").write_bytes(b"NEW ZIPAPP\n")

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 74, result.stderr)
            self.assertEqual((installation / MARKER_NAME).read_bytes(), MARKER_BYTES)
            self.assertEqual(
                (installation / "remove-eml-attachments.pyz").read_bytes(),
                original_zipapp,
            )
            self.assertEqual(
                (installation / "run-from-finder.sh").read_bytes(),
                original_runner,
            )
            self.assertFalse(
                [
                    path
                    for path in installation.iterdir()
                    if path.name.startswith(".") and path.name != MARKER_NAME
                ],
            )

    def test_installation_path_is_displayed_without_terminal_controls(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            home = base / "public-home"
            home.mkdir()
            installation = base / "public-ž\x1b[31minstallation\n"
            zipapp = base / "public.pyz"
            zipapp.write_bytes(b"PUBLIC ZIPAPP\n")

            result = _run(INSTALLER, _environment(home, installation, zipapp))

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("\x1b", result.stdout)
            self.assertIn("public-ž\\x1b[31minstallation\\x0a", result.stdout)
            command = next(
                line
                for line in result.stdout.splitlines()
                if line.startswith("EML_REMOVER_SHORTCUT_HOME=")
            )
            self.assertIn("\\0033", command)
            self.assertIn("\\0012", command)
            launched = subprocess.run(
                [str(POSIX_SHELL), "-c", command],
                env=_environment(home, installation, zipapp),
                text=True,
                capture_output=True,
                check=False,
                timeout=SUBPROCESS_TIMEOUT_SECONDS,
            )
            self.assertEqual(launched.returncode, 2, launched.stderr)
            self.assertIn("no Finder files were supplied", launched.stderr)

    def test_installation_directory_is_private_and_unsafe_existing_mode_refused(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation, _zipapp, environment = _install(base)
            self.assertEqual(installation.stat().st_mode & 0o777, 0o700)
            installation.chmod(0o777)

            result = _run(INSTALLER, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("not safely owned", result.stderr)
            self.assertEqual(installation.stat().st_mode & 0o777, 0o777)
            self.assertEqual((installation / MARKER_NAME).read_bytes(), MARKER_BYTES)

    def test_installer_rejects_a_nonsticky_world_writable_ancestor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            home = base / "public-home"
            home.mkdir()
            unsafe_parent = base / "public-unsafe-parent"
            unsafe_parent.mkdir(mode=0o777)
            unsafe_parent.chmod(0o777)
            installation = unsafe_parent / "public-installation"
            zipapp = base / "public.pyz"
            zipapp.write_bytes(b"PUBLIC ZIPAPP\n")

            result = _run(
                INSTALLER,
                _environment(home, installation, zipapp),
            )

            self.assertEqual(result.returncode, 4)
            self.assertIn("unsafe ancestor", result.stderr)
            self.assertFalse(installation.exists())
