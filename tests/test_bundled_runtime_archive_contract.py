"""Portable controls for source-derived bundled runtime archive roundtrips."""

from __future__ import annotations

import os
import plistlib
import shutil
import stat
from contextlib import nullcontext
from typing import TYPE_CHECKING

import pytest
from tools import build_timestamp, macos_runtime, macos_runtime_archive
from tools import macos_archive as archive

from tests.test_macos_archive import bundle_fixture

if TYPE_CHECKING:
    import zipfile
    from pathlib import Path


def _restore_only_ordinary_files(monkeypatch: pytest.MonkeyPatch) -> None:
    """Windows cannot stamp link inode times; POSIX checks retain that assertion."""
    restore = archive.__dict__["_restore_times"]

    def restore_files(
        destination: Path, entries: list[zipfile.ZipInfo], timestamp: int
    ) -> None:
        ordinary = [
            item for item in entries if not stat.S_ISLNK(item.external_attr >> 16)
        ]
        restore(destination, ordinary, timestamp)

    monkeypatch.setattr(archive, "_restore_times", restore_files)


@pytest.mark.parametrize("architecture", ["arm64", "x86_64"])
def test_bundled_archive_preserves_verified_runtime_resources_and_internal_links(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, architecture: str
) -> None:
    """Portable archive controls supplement actual signed app/runtime tests on Mac."""
    if os.name == "nt":
        _restore_only_ordinary_files(monkeypatch)
    app = bundle_fixture(tmp_path)
    info = app / "Contents/Info.plist"
    metadata = plistlib.loads(info.read_bytes())
    metadata["EMLRuntimeMode"] = "bundled"
    metadata["EMLArchitecture"] = architecture
    info.write_bytes(plistlib.dumps(metadata))
    expected = tmp_path / "reference"
    expected.mkdir()
    (expected / "bin").mkdir()
    program = expected / "bin/python3.14"
    program.write_bytes(b"independent public runtime resource")
    program.chmod(0o755)
    (expected / "bin/python3").symlink_to("python3.14")
    runtime = app / "Contents/Resources/Runtime"
    shutil.copytree(expected, runtime, symlinks=True)

    def reference(cpu: str) -> nullcontext[Path]:
        assert cpu == architecture
        return nullcontext(expected)

    def native(root: Path, cpu: str) -> list[Path]:
        assert cpu == architecture
        assert root.is_dir()
        return []

    monkeypatch.setattr(macos_runtime_archive, "reference", reference)
    monkeypatch.setattr(macos_runtime, "require_native", native)
    monkeypatch.setattr(archive, "MAX_TOTAL", 1)
    packaged = tmp_path / "bundled.zip"
    archive.package(app, packaged)
    extracted = tmp_path / "extracted"
    archive.__dict__["_extract"](packaged, extracted)
    delivered = extracted / archive.RUNTIME_PREFIX
    assert (delivered / "bin/python3").readlink().as_posix() == "python3.14"
    assert (delivered / "bin/python3").read_bytes() == program.read_bytes()
    if os.name != "nt":
        assert stat.S_IMODE((delivered / "bin/python3.14").stat().st_mode) == 0o755
        assert stat.S_IMODE((delivered / "bin").stat().st_mode) == 0o755
        assert (delivered / "bin/python3").lstat().st_mtime == build_timestamp.EPOCH
