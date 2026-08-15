"""Distribution and macOS launcher regression tests."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile
from email import policy
from email.message import EmailMessage
from pathlib import Path
from typing import TYPE_CHECKING, ClassVar

from eml_attachment_remover import __version__
from tests.test_support import (
    SUBPROCESS_TIMEOUT_SECONDS,
    parse,
    subprocess_environment,
)

if TYPE_CHECKING:
    from collections.abc import Sequence

PROJECT_ROOT = Path(__file__).resolve().parents[1]
BUILD_TOOL = PROJECT_ROOT / "tools" / "build_zipapp.py"
RELEASE_TAG_CHECK = PROJECT_ROOT / "tools" / "check_release_tag.py"
WRAPPER = PROJECT_ROOT / "integrations" / "macos-shortcuts" / "run-from-finder.sh"
INSTALLER = PROJECT_ROOT / "integrations" / "macos-shortcuts" / "install.sh"
INSTALLER_SUPPORT = (
    PROJECT_ROOT / "integrations" / "macos-shortcuts" / "installer-filesystem.sh"
)
UNINSTALLER = PROJECT_ROOT / "integrations" / "macos-shortcuts" / "uninstall.sh"
POSIX_SHELL = Path("/bin/sh")
POSIX_SHELL_AVAILABLE = os.name == "posix" and POSIX_SHELL.is_file()
POSIX_ONLY = "requires the mandatory POSIX integration environment"


def _run_external(
    command: Sequence[str],
    *,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run an external test program under strict, bounded conditions.

    Returns:
        The completed text-mode process result.

    """
    child_environment = subprocess_environment() if environment is None else environment
    return subprocess.run(
        command,
        env=child_environment,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def _run_python(
    program: Path,
    *arguments: str,
) -> subprocess.CompletedProcess[str]:
    """Run one Python script or archive with development checks enabled.

    Returns:
        The completed text-mode process result.

    """
    return _run_external(
        [
            sys.executable,
            "-X",
            "dev",
            "-W",
            "error",
            str(program),
            *arguments,
        ],
    )


def _integration_environment(**updates: str) -> dict[str, str]:
    """Return a strict environment for POSIX launcher tests.

    Returns:
        The child environment with the supplied integration settings.

    """
    environment = subprocess_environment()
    environment.update(updates)
    return environment


def message_bytes() -> bytes:
    """Create a small EML containing one body and one attachment.

    Returns:
        Serialized synthetic EML bytes.

    """
    message = EmailMessage()
    message["Subject"] = "Distribution test"
    message.set_content("Body")
    message.add_attachment(
        b"attachment",
        maintype="application",
        subtype="octet-stream",
        filename="file.bin",
    )
    return message.as_bytes(policy=policy.SMTP)


class DistributionTests(unittest.TestCase):
    """Exercise packaged entry points and POSIX integrations as external programs."""

    temporary_directory: ClassVar[tempfile.TemporaryDirectory[str]]
    base: ClassVar[Path]
    checksum: ClassVar[Path]
    zipapp: ClassVar[Path]

    @classmethod
    def setUpClass(cls) -> None:
        """Build one temporary zipapp for this test class."""
        cls.temporary_directory = tempfile.TemporaryDirectory()
        cls.base = Path(cls.temporary_directory.name)
        cls.zipapp = cls.base / "remove-eml-attachments.pyz"
        cls.checksum = cls.base / "SHA256SUMS"
        result = _run_python(
            BUILD_TOOL,
            "--target",
            str(cls.zipapp),
            "--checksum-file",
            str(cls.checksum),
        )
        if result.returncode != 0:
            msg = f"zipapp build failed: {result.stdout}\n{result.stderr}"
            raise RuntimeError(msg)

    @classmethod
    def tearDownClass(cls) -> None:
        """Remove the class-level temporary directory."""
        cls.temporary_directory.cleanup()

    def _assert_semantic_output(
        self,
        source: Path,
        destination: Path,
        original: bytes,
    ) -> None:
        """Assert that processing preserved the source and retained body."""
        self.assertEqual(source.read_bytes(), original)
        self.assertGreater(destination.stat().st_size, 0)
        output = parse(destination)
        self.assertEqual(output["Subject"], "Distribution test")
        body = output.get_body(preferencelist=("plain",))
        self.assertIsNotNone(body)
        assert body is not None
        self.assertEqual(body.get_content().strip(), "Body")
        self.assertFalse(
            any(
                part.get_content_disposition() == "attachment" for part in output.walk()
            ),
        )
        self.assertFalse(
            [defect for part in output.walk() for defect in part.defects],
        )

    def test_zipapp_reports_version(self) -> None:
        result = _run_python(self.zipapp, "--version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), f"remove-eml-attachments {__version__}")

    def test_installed_console_script_reports_version(self) -> None:
        command = shutil.which("remove-eml-attachments")
        self.assertIsNotNone(command)
        assert command is not None
        result = _run_external([command, "--version"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), f"remove-eml-attachments {__version__}")

    def test_installed_console_script_processes_a_file_semantically(self) -> None:
        command = shutil.which("remove-eml-attachments")
        self.assertIsNotNone(command)
        assert command is not None
        source = self.base / "installed command source.eml"
        destination = self.base / "installed command output.eml"
        original = message_bytes()
        source.write_bytes(original)
        result = _run_external(
            [command, "--output", str(destination), "--", str(source)],
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self._assert_semantic_output(source, destination, original)

    def test_zipapp_has_no_cached_bytecode(self) -> None:
        with zipfile.ZipFile(self.zipapp) as archive:
            names = archive.namelist()
        self.assertIn("LICENSE", names)
        self.assertFalse(any("__pycache__/" in name for name in names))
        self.assertFalse(any(name.endswith(".pyc") for name in names))

    def test_zipapp_build_is_byte_reproducible(self) -> None:
        second = self.base / "second.pyz"
        result = _run_python(BUILD_TOOL, "--target", str(second))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(second.read_bytes(), self.zipapp.read_bytes())

    def test_zipapp_checksum_manifest_matches_the_archive(self) -> None:
        digest = hashlib.sha256(self.zipapp.read_bytes()).hexdigest()
        expected = f"{digest}  {self.zipapp.name}\n"
        self.assertEqual(self.checksum.read_text(encoding="utf-8"), expected)

    def test_zipapp_processes_a_file(self) -> None:
        source = self.base / "zipapp source.eml"
        original = message_bytes()
        source.write_bytes(original)
        result = _run_python(self.zipapp, str(source))
        self.assertEqual(result.returncode, 0, result.stderr)
        destination = self.base / "zipapp source.attachments-removed.eml"
        self._assert_semantic_output(source, destination, original)

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_posix_shell_scripts_pass_syntax_check(self) -> None:
        for script in (
            WRAPPER,
            INSTALLER,
            INSTALLER_SUPPORT,
            UNINSTALLER,
        ):
            with self.subTest(script=script.name):
                result = _run_external([str(POSIX_SHELL), "-n", str(script)])
                self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_finder_wrapper_passes_multiple_difficult_paths(self) -> None:
        first = self.base / "one žą 🚚.eml"
        second = self.base / "two 'quoted'.eml"
        first.write_bytes(message_bytes())
        second.write_bytes(message_bytes())
        environment = _integration_environment(
            EML_REMOVER_PYTHON=sys.executable,
            EML_REMOVER_REVEAL="0",
            EML_REMOVER_ZIPAPP=str(self.zipapp),
        )
        result = _run_external(
            [str(POSIX_SHELL), str(WRAPPER), str(first), str(second)],
            environment=environment,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            "Created or retained 2 attachment-removed EML file(s)", result.stdout
        )
        self.assertTrue((self.base / "one žą 🚚.attachments-removed.eml").is_file())
        self.assertTrue((self.base / "two 'quoted'.attachments-removed.eml").is_file())

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_installer_uses_user_application_support_without_admin(self) -> None:
        with tempfile.TemporaryDirectory() as home_directory:
            environment = _integration_environment()
            environment["HOME"] = home_directory
            environment["EML_REMOVER_PYTHON"] = sys.executable
            result = _run_external(
                [str(POSIX_SHELL), str(INSTALLER)],
                environment=environment,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            install_directory = (
                Path(home_directory)
                / "Library"
                / "Application Support"
                / "EML Attachment Remover"
            )
            self.assertTrue(
                (install_directory / "remove-eml-attachments.pyz").is_file()
            )
            self.assertTrue((install_directory / "run-from-finder.sh").is_file())
            self.assertIn("Pass Input", result.stdout)

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_finder_wrapper_rejects_an_empty_invocation(self) -> None:
        result = _run_external(
            [str(POSIX_SHELL), str(WRAPPER)],
            environment=_integration_environment(),
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("no Finder files were supplied", result.stderr)

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_finder_wrapper_rejects_a_missing_python(self) -> None:
        source = self.base / "missing-python source.eml"
        source.write_bytes(message_bytes())
        result = _run_external(
            [str(POSIX_SHELL), str(WRAPPER), str(source)],
            environment=_integration_environment(
                EML_REMOVER_PYTHON=str(self.base / "missing-python"),
                EML_REMOVER_ZIPAPP=str(self.zipapp),
            ),
        )
        self.assertEqual(result.returncode, 9)
        self.assertIn("Python 3.14 was not found", result.stderr)

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_finder_wrapper_cleans_temporary_reports_after_a_usage_error(self) -> None:
        source = self.base / "invalid-policy source.eml"
        source.write_bytes(message_bytes())
        report_directory = self.base / "wrapper-temporary-files"
        report_directory.mkdir()
        result = _run_external(
            [str(POSIX_SHELL), str(WRAPPER), str(source)],
            environment=_integration_environment(
                EML_REMOVER_EXISTING="invalid",
                EML_REMOVER_PYTHON=sys.executable,
                EML_REMOVER_ZIPAPP=str(self.zipapp),
                TMPDIR=str(report_directory),
            ),
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("must be error, skip, or force", result.stderr)
        self.assertEqual(list(report_directory.iterdir()), [])

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_uninstaller_removes_only_the_configured_directory(self) -> None:
        installation = self.base / "temporary-installation"
        sibling = self.base / "must-remain.txt"
        sibling.write_bytes(b"public")
        environment = _integration_environment(
            EML_REMOVER_HOME=str(installation),
            EML_REMOVER_PYTHON=sys.executable,
            EML_REMOVER_ZIPAPP=str(self.zipapp),
        )
        install_result = _run_external(
            [str(POSIX_SHELL), str(INSTALLER)],
            environment=environment,
        )
        self.assertEqual(install_result.returncode, 0, install_result.stderr)
        result = _run_external(
            [str(POSIX_SHELL), str(UNINSTALLER)],
            environment=environment,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(installation.exists())
        self.assertEqual(sibling.read_bytes(), b"public")

    @unittest.skipUnless(POSIX_SHELL_AVAILABLE, POSIX_ONLY)
    def test_uninstaller_refuses_the_filesystem_root(self) -> None:
        result = _run_external(
            [str(POSIX_SHELL), str(UNINSTALLER)],
            environment=_integration_environment(EML_REMOVER_HOME="/"),
        )
        self.assertEqual(result.returncode, 4)
        self.assertIn("Refusing an unsafe uninstall path", result.stderr)

    def test_json_zipapp_report_is_valid(self) -> None:
        source = self.base / "json source.eml"
        original = message_bytes()
        source.write_bytes(original)
        result = _run_python(
            self.zipapp,
            "--output-format",
            "json",
            str(source),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertTrue(report["ok"])
        self.assertEqual(len(report["results"]), 1)
        self.assertEqual(
            report["results"][0]["removed"][0]["filename"],
            "file.bin",
        )
        destination = self.base / "json source.attachments-removed.eml"
        self._assert_semantic_output(source, destination, original)

    def test_release_tag_must_match_the_project_version(self) -> None:
        valid = _run_python(RELEASE_TAG_CHECK, f"v{__version__}")
        self.assertEqual(valid.returncode, 0, valid.stderr)
        invalid = _run_python(RELEASE_TAG_CHECK, "v0.0.0")
        self.assertEqual(invalid.returncode, 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
