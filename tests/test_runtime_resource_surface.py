"""Runtime resource inventories preserve complete file kinds and portable modes."""

from __future__ import annotations

import os
import stat
from typing import TYPE_CHECKING

import pytest
from tools import macos_runtime_archive

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX execute-bit inventory semantics")
def test_source_inventory_normalizes_each_kind_and_any_execute_bit(
    tmp_path: Path,
) -> None:
    directory = tmp_path / "bin"
    directory.mkdir()
    program = directory / "program"
    program.write_bytes(b"retained executable resource")
    program.chmod(0o401)
    data = directory / "data"
    data.write_bytes(b"retained ordinary resource")
    data.chmod(0o400)
    (directory / "alias").symlink_to("program")
    assert macos_runtime_archive.surface(tmp_path, "Runtime") == {
        "Runtime/": stat.S_IFDIR | 0o755,
        "Runtime/bin/": stat.S_IFDIR | 0o755,
        "Runtime/bin/program": stat.S_IFREG | 0o755,
        "Runtime/bin/data": stat.S_IFREG | 0o644,
        "Runtime/bin/alias": stat.S_IFLNK | 0o777,
    }
