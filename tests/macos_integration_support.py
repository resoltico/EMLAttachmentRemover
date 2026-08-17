"""Shared public fixtures for macOS integration subprocess tests."""

from __future__ import annotations

import copy
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

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


def valid_finder_result(
    destination: str = "public-output.text-only.eml",
) -> dict[str, object]:
    """Return one complete schema-v2 success record.

    Returns:
        A result containing every text-only audit category.

    """
    return {
        "destination": destination,
        "discarded_body_representations": [
            {"content_type": "multipart/related", "mime_path": "2"}
        ],
        "discarded_body_resources": [
            {
                "content_type": "image/jpeg",
                "disposition": "inline",
                "filename": "public-image.jpg",
                "mime_path": "2.2",
                "referenced_by": ["2.1"],
            }
        ],
        "dry_run": False,
        "output_size": 45,
        "removed_attachments": [
            {
                "content_type": "application/pdf",
                "disposition": "attachment",
                "filename": "public.pdf",
                "mime_path": "3",
            }
        ],
        "selected_plain_text_bodies": [
            {"content_type": "text/plain", "mime_path": "root"}
        ],
        "source": "public-source.eml",
        "source_size": 123,
        "status": "ok",
        "warnings": [],
    }


def valid_finder_report(
    *,
    results: list[dict[str, object]] | None = None,
    skipped: list[dict[str, object]] | None = None,
    errors: list[dict[str, object]] | None = None,
) -> dict[str, object]:
    """Return one complete schema-v2 batch report.

    Returns:
        A report whose status is derived from its errors.

    """
    error_records = [] if errors is None else errors
    return {
        "errors": error_records,
        "ok": not error_records,
        "program": "remove-eml-attachments",
        "results": [] if results is None else results,
        "schema_version": 2,
        "scope": "text-only",
        "skipped": [] if skipped is None else skipped,
        "version": "2.0.0",
    }


def write_report_processor(
    path: Path,
    payload: object,
    *,
    status: int = 0,
    diagnostics: str | None = None,
) -> None:
    """Write a synthetic processor emitting the supplied report."""
    source = "import sys\n"
    if diagnostics is not None:
        source += f"print({diagnostics!r}, file=sys.stderr)\n"
    source += f"print({json.dumps(payload)!r})\n"
    source += f"raise SystemExit({status})\n"
    path.write_text(source, encoding="utf-8")


def replace_result_field(
    report: dict[str, object],
    name: str,
    value: object,
) -> dict[str, object]:
    """Return a report with one first-result field replaced.

    Returns:
        An independent malformed report.

    """
    altered = copy.deepcopy(report)
    result = cast("list[dict[str, object]]", altered["results"])[0]
    result[name] = value
    return altered


def replace_record_field(
    report: dict[str, object],
    category: str,
    name: str,
    value: object,
) -> dict[str, object]:
    """Return a report with one first audit-record field replaced.

    Returns:
        An independent malformed report.

    """
    altered = copy.deepcopy(report)
    result = cast("list[dict[str, object]]", altered["results"])[0]
    record = cast("list[dict[str, object]]", result[category])[0]
    record[name] = value
    return altered


def temporary_mode_processor_source() -> str:
    """Return a processor that records launcher temporary-file modes.

    Returns:
        Synthetic Python source for the public test processor.

    """
    serialized = json.dumps(valid_finder_report())
    return (
        "import json, os, pathlib, stat\n"
        "entries = sorted(pathlib.Path(os.environ['TMPDIR']).iterdir())\n"
        "modes = [stat.S_IMODE(entry.stat().st_mode) for entry in entries]\n"
        "pathlib.Path(os.environ['PUBLIC_MODE_LOG']).write_text(json.dumps(modes))\n"
        f"print({serialized!r})\n"
    )


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
