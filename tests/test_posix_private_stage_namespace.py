"""Live POSIX stage security against substituted directory entries."""

from __future__ import annotations

import os
import shutil
import stat
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_posix, native_posix_stage
from eml_attachment_remover.domain import AppError, BoundDirectory, ExitCode

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_survives_outer_name_substitution(tmp_path: Path) -> None:
    """Renaming a private directory cannot redirect its held candidate descriptor."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), False)
    name = b".eml-remove-private-stage-test.tmp"
    stage = native_posix_stage.create(parent, name)
    original = tmp_path / os.fsdecode(name)
    displaced = tmp_path / "displaced-stage"
    try:
        os.write(stage, b"trusted candidate")
        assert stat.S_IMODE(original.stat().st_mode) & 0o077 == 0
        os.rename(original, displaced)
        original.mkdir(mode=0o700)
        (original / "candidate").write_bytes(b"substituted payload")
        native_posix_stage.publish(parent, stage, name, b"output.eml")
        assert (tmp_path / "output.eml").read_bytes() == b"trusted candidate"
        with pytest.raises(AppError) as failure:
            native_posix_stage.discard(parent, stage, name)
        assert failure.value.code is ExitCode.OUTPUT_CONFLICT
        assert (original / "candidate").read_bytes() == b"substituted payload"
    finally:
        os.close(stage)
        native_posix.close_bound_directory(parent)
        if displaced.exists():
            shutil.rmtree(displaced)
        if original.exists():
            shutil.rmtree(original)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_normal_publication_removes_namespace(tmp_path: Path) -> None:
    """Successful publication removes both the protected file and its directory."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), False)
    name = b".eml-remove-clean-stage-test.tmp"
    stage = native_posix_stage.create(parent, name)
    try:
        os.write(stage, b"source candidate")
        native_posix_stage.publish(parent, stage, name, b"out.eml")
        native_posix_stage.discard(parent, stage, name)
        assert (tmp_path / "out.eml").read_bytes() == b"source candidate"
        assert not (tmp_path / os.fsdecode(name)).exists()
    finally:
        os.close(stage)
        native_posix.close_bound_directory(parent)
