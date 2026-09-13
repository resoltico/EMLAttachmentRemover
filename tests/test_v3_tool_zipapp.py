"""V3 zipapp packaging contracts independent of the source checkout importer."""

from __future__ import annotations

import os
import subprocess
import sys
import zipfile
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
BUILD_TOOL = PROJECT_ROOT / "tools" / "build_zipapp.py"
SCHEMA_FILE = PROJECT_ROOT / "schema" / "report.schema.json"


def _environment() -> dict[str, str]:
    return {
        name: value
        for name, value in os.environ.items()
        if not name.startswith("COVERAGE_")
    }


def _build(target: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-B", str(BUILD_TOOL), "--target", str(target)],
        check=False,
        cwd=PROJECT_ROOT,
        env=_environment(),
        text=True,
        capture_output=True,
    )


def test_built_zipapp_has_exact_source_members_and_normalized_metadata(
    tmp_path: Path,
) -> None:
    target = tmp_path / "remove-eml-attachments.pyz"
    result = _build(target)
    assert result.returncode == 0, result.stderr
    assert target.is_file()
    with zipfile.ZipFile(target) as archive:
        assert archive.testzip() is None
        names = set(archive.namelist())
        metadata_name = next(
            name for name in names if name.endswith(".dist-info/METADATA")
        )
        assert "LICENSE" in names
        assert "schema/report.schema.json" in names
        assert "eml_attachment_remover/batch.py" in names
        assert "eml_attachment_remover/html_text.py" not in names
        assert "eml_attachment_remover/mime_text_only.py" not in names
        assert archive.read("schema/report.schema.json") == SCHEMA_FILE.read_bytes()
        assert b"Version: 3.0.0\n" in archive.read(metadata_name)
        launcher = archive.read("__main__.py")
    assert b"eml_attachment_remover.app" in launcher
    assert b"3.14" in launcher
    version = subprocess.run(
        [sys.executable, "-B", str(target), "--version"],
        check=False,
        env=_environment(),
        text=True,
        capture_output=True,
    )
    assert version.returncode == 0, version.stderr
    assert version.stdout.strip() == "remove-eml-attachments 3.0.0"
