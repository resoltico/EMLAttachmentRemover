"""Runtime copying normalizes executable, resource and directory permissions."""

from __future__ import annotations

import os
import stat
import sys
from pathlib import Path

import pytest
from tools import macos_runtime, macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture

@pytest.mark.skipif(os.name == "nt", reason="POSIX runtime permission normalization")
def test_runtime_copy_normalizes_noncanonical_source_permissions(
    tmp_path: Path,
) -> None:
    archive, pin = runtime_archive_fixture(tmp_path)
    source = macos_runtime_source.extract(archive, tmp_path / "source", pin)
    (source / "install/bin/python3.14").chmod(0o711)
    (source / "licenses/LICENSE.cpython.txt").chmod(0o600)
    (source / "install/bin").chmod(0o700)
    target = tmp_path / "Runtime"
    macos_runtime.copy_install(source, target, Path(sys.executable))
    assert stat.S_IMODE((target / "bin/python3.14").stat().st_mode) == 0o755
    assert (
        stat.S_IMODE((target / "licenses/LICENSE.cpython.txt").stat().st_mode) == 0o644
    )
    assert stat.S_IMODE((target / "bin").stat().st_mode) == 0o755
