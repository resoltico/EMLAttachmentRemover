"""Exercise the ownership boundary of the macOS integration scripts."""

from __future__ import annotations

import os
import shlex
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING, Final

from tests.test_support import SUBPROCESS_TIMEOUT_SECONDS, subprocess_environment

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
INSTALLER: Final = PROJECT_ROOT / "integrations/macos-shortcuts/install.sh"
UNINSTALLER: Final = PROJECT_ROOT / "integrations/macos-shortcuts/uninstall.sh"
POSIX_SHELL: Final = Path("/bin/sh")
POSIX_AVAILABLE: Final = os.name == "posix" and POSIX_SHELL.is_file()
POSIX_REASON: Final = "requires the mandatory POSIX integration environment"
MARKER_NAME: Final = ".eml-attachment-remover-installation"


def _run_script(
    script: Path,
    environment: Mapping[str, str],
    *,
    cwd: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run one integration script under a bounded environment.

    Returns:
        The completed shell process.

    """
    return subprocess.run(
        [str(POSIX_SHELL), str(script)],
        cwd=cwd,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def _environment(
    home: Path,
    zipapp: Path,
    installation: Path,
) -> dict[str, str]:
    """Build an isolated integration environment.

    Returns:
        Environment selecting the supplied public test paths.

    """
    environment = subprocess_environment()
    environment.update({
        "EML_REMOVER_HOME": str(installation),
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_ZIPAPP": str(zipapp),
        "HOME": str(home),
    })
    return environment


def _fixture(base: Path, installation: Path) -> tuple[Path, dict[str, str]]:
    """Create public installer inputs.

    Returns:
        The fake zipapp and isolated environment.

    """
    home = base / "home"
    home.mkdir()
    zipapp = base / "public.pyz"
    zipapp.write_bytes(b"PUBLIC ZIPAPP")
    return zipapp, _environment(home, zipapp, installation)


@unittest.skipUnless(POSIX_AVAILABLE, POSIX_REASON)
class IntegrationOwnershipTests(unittest.TestCase):
    """Require marker ownership and exact-file uninstall behaviour."""

    def test_difficult_install_path_is_marked_and_exactly_uninstalled(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation = base / "public žā ' installation"
            _zipapp, environment = _fixture(base, installation)

            installed = _run_script(INSTALLER, environment)
            self.assertEqual(installed.returncode, 0, installed.stderr)
            expected_command = (
                f"EML_REMOVER_HOME={shlex.quote(str(installation.resolve()))} "
                f"/bin/sh "
                f"{shlex.quote(str(installation.resolve() / 'run-from-finder.sh'))} "
                '"$@"'
            )
            self.assertIn("Name the shortcut: Remove EML Attachments", installed.stdout)
            self.assertIn(expected_command, installed.stdout)
            self.assertIn("'Input' to 'Shortcut Input'", installed.stdout)
            self.assertTrue((installation / MARKER_NAME).is_file())
            self.assertEqual(
                (installation / "remove-eml-attachments.pyz").read_bytes(),
                b"PUBLIC ZIPAPP",
            )

            removed = _run_script(UNINSTALLER, environment)
            self.assertEqual(removed.returncode, 0, removed.stderr)
            self.assertFalse(installation.exists())

    def test_installer_rejects_root_home_home_ancestor_and_dot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            safe_target = base / "safe-target"
            zipapp, environment = _fixture(base, safe_target)
            home = Path(environment["HOME"])
            unsafe_targets: Sequence[Path] = (
                Path(os.sep) / "tmp" / "..",
                home,
                home / "..",
                Path(),
            )
            for target in unsafe_targets:
                with self.subTest(target=target):
                    selected = _environment(home, zipapp, target)
                    result = _run_script(INSTALLER, selected, cwd=base)
                    self.assertEqual(result.returncode, 4, result.stderr)
                    self.assertIn("Refusing", result.stderr)

    def test_final_installation_symlink_is_rejected_by_both_scripts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            real_directory = base / "public-real"
            real_directory.mkdir()
            installation = base / "public-link"
            installation.symlink_to(real_directory, target_is_directory=True)
            _zipapp, environment = _fixture(base, installation)

            install_result = _run_script(INSTALLER, environment)
            uninstall_result = _run_script(UNINSTALLER, environment)

            self.assertEqual(install_result.returncode, 4)
            self.assertEqual(uninstall_result.returncode, 4)
            self.assertTrue(installation.is_symlink())
            self.assertEqual(list(real_directory.iterdir()), [])

    def test_installer_never_follows_an_existing_destination_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation = base / "public-installation"
            _zipapp, environment = _fixture(base, installation)
            first = _run_script(INSTALLER, environment)
            self.assertEqual(first.returncode, 0, first.stderr)
            target = installation / "remove-eml-attachments.pyz"
            target.unlink()
            victim = base / "public-victim"
            victim.write_bytes(b"MUST REMAIN")
            target.symlink_to(victim)

            second = _run_script(INSTALLER, environment)

            self.assertEqual(second.returncode, 4)
            self.assertIn("unsafe installed zipapp entry", second.stderr)
            self.assertEqual(victim.read_bytes(), b"MUST REMAIN")
            self.assertTrue(target.is_symlink())

    def test_uninstaller_refuses_a_directory_without_the_exact_marker(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation = base / "public-unowned"
            installation.mkdir()
            unknown = installation / "must-remain.txt"
            unknown.write_bytes(b"MUST REMAIN")
            _zipapp, environment = _fixture(base, installation)

            result = _run_script(UNINSTALLER, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("without the exact installation marker", result.stderr)
            self.assertEqual(unknown.read_bytes(), b"MUST REMAIN")

    def test_uninstaller_preserves_unknown_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            installation = base / "public-installation"
            _zipapp, environment = _fixture(base, installation)
            installed = _run_script(INSTALLER, environment)
            self.assertEqual(installed.returncode, 0, installed.stderr)
            unknown = installation / "must-remain.txt"
            unknown.write_bytes(b"MUST REMAIN")

            result = _run_script(UNINSTALLER, environment)

            self.assertEqual(result.returncode, 4)
            self.assertIn("directory containing unknown entries", result.stderr)
            self.assertEqual(unknown.read_bytes(), b"MUST REMAIN")
            self.assertTrue((installation / MARKER_NAME).is_file())
            self.assertTrue((installation / "remove-eml-attachments.pyz").is_file())
            self.assertTrue((installation / "run-from-finder.sh").is_file())

    def test_scripts_require_exact_cpython_and_publish_marker_last(self) -> None:
        """Keep interpreter and interrupted-install ownership checks explicit."""
        installer = INSTALLER.read_text(encoding="utf-8")
        runner = (
            PROJECT_ROOT / "integrations/macos-shortcuts/run-from-finder.sh"
        ).read_text(encoding="utf-8")
        for script in (installer, runner):
            with self.subTest(script=script[:20]):
                self.assertIn('platform.python_implementation() != "CPython"', script)
                self.assertIn("sys.version_info[:2] != (3, 14)", script)
        remove_marker = installer.index(
            'remove_exact_marker "$INSTALL_DIR/$MARKER_NAME"'
        )
        publish_zipapp = installer.index(
            'replace_file "$ZIPAPP_TEMP" "$INSTALL_DIR/remove-eml-attachments.pyz"'
        )
        publish_runner = installer.index(
            'replace_file "$RUNNER_TEMP" "$INSTALL_DIR/run-from-finder.sh"'
        )
        publish_marker = installer.index(
            'replace_file "$MARKER_TEMP" "$INSTALL_DIR/$MARKER_NAME"',
            publish_runner,
        )
        self.assertLess(remove_marker, publish_zipapp)
        self.assertLess(publish_zipapp, publish_runner)
        self.assertLess(publish_runner, publish_marker)


class IntegrationDocumentationTests(unittest.TestCase):
    """Keep manual Shortcut lifecycle directions explicit."""

    def test_guide_names_and_fully_uninstalls_the_shortcut(self) -> None:
        guide = (INSTALLER.parent / "README.md").read_text(encoding="utf-8")

        self.assertIn("Name it **Remove EML Attachments**", guide)
        self.assertIn("command printed by the installer", guide)
        self.assertIn("**Input** to **Shortcut Input**", guide)
        self.assertIn("confirm that it still", guide)
        self.assertIn("find **Remove EML Attachments**, choose **Delete**", guide)
        self.assertIn("reacquire the same release", guide)
