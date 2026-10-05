"""Portable controls for source-derived bundled runtime archive roundtrips."""

from __future__ import annotations

import plistlib
import shutil
import stat
from contextlib import nullcontext
from typing import TYPE_CHECKING

from tools import build_timestamp, macos_runtime, macos_runtime_archive
from tools import macos_archive as archive

from tests.test_macos_archive import bundle_fixture

if TYPE_CHECKING:
    from pathlib import Path

    import pytest


def test_bundled_archive_preserves_verified_runtime_resources_and_internal_links(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Portable archive controls supplement actual signed app/runtime tests on Mac."""
    app = bundle_fixture(tmp_path)
    info = app / "Contents/Info.plist"
    metadata = plistlib.loads(info.read_bytes())
    metadata["EMLRuntimeMode"] = "bundled"
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
    monkeypatch.setattr(
        macos_runtime_archive, "reference", lambda _cpu: nullcontext(expected)
    )
    monkeypatch.setattr(macos_runtime, "require_native", lambda _root, _cpu: [])
    packaged = tmp_path / "bundled.zip"
    archive.package(app, packaged)
    extracted = tmp_path / "extracted"
    archive.__dict__["_extract"](packaged, extracted)
    delivered = extracted / archive.RUNTIME_PREFIX
    assert (delivered / "bin/python3").readlink().as_posix() == "python3.14"
    assert (delivered / "bin/python3").read_bytes() == program.read_bytes()
    assert stat.S_IMODE((delivered / "bin/python3.14").stat().st_mode) == 0o755
    assert stat.S_IMODE((delivered / "bin").stat().st_mode) == 0o755
    assert (delivered / "bin/python3").lstat().st_mtime == build_timestamp.EPOCH
