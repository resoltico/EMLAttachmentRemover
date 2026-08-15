"""Shared public fixtures for macOS integration subprocess tests."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final

from tests.test_support import SUBPROCESS_TIMEOUT_SECONDS, subprocess_environment

if TYPE_CHECKING:
    from collections.abc import Mapping

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
INTEGRATION_ROOT: Final = PROJECT_ROOT / "integrations/macos-shortcuts"
INSTALLER: Final = INTEGRATION_ROOT / "install.sh"
INSTALLER_SUPPORT: Final = INTEGRATION_ROOT / "installer-filesystem.sh"
UNINSTALLER: Final = INTEGRATION_ROOT / "uninstall.sh"
POSIX_SHELL: Final = Path("/bin/sh")
POSIX_AVAILABLE: Final = os.name == "posix" and POSIX_SHELL.is_file()
POSIX_REASON: Final = "requires the mandatory POSIX integration environment"
MARKER_NAME: Final = ".eml-attachment-remover-installation"
MARKER_BYTES: Final = b"EML Attachment Remover managed installation\n"


def run_script(
    script: Path,
    environment: Mapping[str, str],
    *arguments: str,
) -> subprocess.CompletedProcess[str]:
    """Run one shell integration under a bounded public test environment.

    Returns:
        The completed shell process.

    """
    return subprocess.run(
        [str(POSIX_SHELL), str(script), *arguments],
        env=environment,
        text=True,
        capture_output=True,
        check=False,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
    )


def integration_environment(
    home: Path,
    installation: Path,
    zipapp: Path,
    **updates: str,
) -> dict[str, str]:
    """Return an isolated environment selecting public fixture paths.

    Returns:
        A subprocess environment for the integration scripts.

    """
    environment = subprocess_environment()
    environment.update({
        "EML_REMOVER_HOME": str(installation),
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_REVEAL": "0",
        "EML_REMOVER_ZIPAPP": str(zipapp),
        "HOME": str(home),
    })
    environment.update(updates)
    return environment


def fixture(base: Path) -> tuple[Path, Path, dict[str, str]]:
    """Create one isolated installer fixture.

    Returns:
        The installation directory, zipapp, and subprocess environment.

    """
    home = base / "public-home"
    home.mkdir()
    installation = base / "public-installation"
    zipapp = base / "public-archive.pyz"
    zipapp.write_bytes(b"PUBLIC ZIPAPP\n")
    return installation, zipapp, integration_environment(home, installation, zipapp)


def install(base: Path) -> tuple[Path, Path, dict[str, str]]:
    """Create and verify one managed installation.

    Returns:
        The installation directory, zipapp, and subprocess environment.

    """
    installation, zipapp, environment = fixture(base)
    result = run_script(INSTALLER, environment)
    if result.returncode != 0:
        message = f"fixture installation failed: {result.stdout}\n{result.stderr}"
        raise RuntimeError(message)
    return installation, zipapp, environment


def command_override(base: Path, name: str, body: str) -> Path:
    """Create one executable command override.

    Returns:
        The directory to prepend to ``PATH``.

    """
    command_directory = base / f"public-{name}-commands"
    command_directory.mkdir()
    command = command_directory / name
    command.write_text(f"#!/bin/sh\n{body}\n", encoding="utf-8")
    command.chmod(0o755)
    return command_directory
