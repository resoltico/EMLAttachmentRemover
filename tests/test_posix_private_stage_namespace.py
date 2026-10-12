"""Live POSIX stage security against substituted directory entries."""

from __future__ import annotations

import errno
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
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-private-stage-test.tmp"
    stage = native_posix_stage.create(parent, name)
    original = tmp_path / os.fsdecode(name)
    displaced = tmp_path / "displaced-stage"
    try:
        os.write(stage, b"trusted candidate")
        assert stat.S_IMODE(original.stat().st_mode) & 0o077 == 0
        original.rename(displaced)
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
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
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


same_directory = native_posix_stage._same_directory  # ruff: ignore[private-member-access] - verify every private-directory identity guard.
creation_cleanup = native_posix_stage._creation_cleanup  # ruff: ignore[private-member-access] - verify partial-acquisition cleanup faults.


def _alter_stage(original: Path, displaced: Path, fault: str) -> None:
    """Apply one stale-entry, mode, or identity adversary."""
    if fault in {"missing", "non-directory-entry", "substituted-entry"}:
        original.rename(displaced)
    if fault == "non-directory-entry":
        original.write_bytes(b"unrelated")
    elif fault == "substituted-entry":
        original.mkdir(mode=0o700)
    elif fault == "public-permissions":
        original.chmod(0o750)


def _restore_stage(original: Path, displaced: Path, fault: str) -> None:
    """Restore the real stage entry before releasing live descriptors."""
    if displaced.exists():
        if original.is_dir():
            original.rmdir()
        elif original.exists():
            original.unlink()
        displaced.rename(original)
    if fault == "public-permissions":
        original.chmod(0o700)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
@pytest.mark.parametrize(
    "fault",
    [
        "missing",
        "non-directory-entry",
        "substituted-entry",
        "public-permissions",
        "non-directory-handle",
        "foreign-owner",
    ],
)
def test_private_stage_identity_requires_every_guard(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, fault: str
) -> None:
    """A directory name, kind, inode, private mode and owner all matter."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-identity-stage.tmp"
    stage = native_posix_stage.create(parent, name)
    directory_fd = parent.private_stage_directory_fd
    assert directory_fd is not None
    original = tmp_path / os.fsdecode(name)
    displaced = tmp_path / "relocated-stage"
    try:
        _alter_stage(original, displaced, fault)
        if fault == "foreign-owner":
            actual_uid = os.geteuid()
            with monkeypatch.context() as context:
                context.setattr(os, "geteuid", lambda: actual_uid + 1)
                assert not same_directory(parent, name, directory_fd)
        else:
            descriptor = stage if fault == "non-directory-handle" else directory_fd
            assert not same_directory(parent, name, descriptor)
    finally:
        _restore_stage(original, displaced, fault)
        native_posix_stage.discard(parent, stage, name)
        os.close(stage)
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_refuses_a_second_owned_namespace(tmp_path: Path) -> None:
    """A bound parent must never silently abandon its first staging directory."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-first-stage.tmp"
    stage = native_posix_stage.create(parent, name)
    try:
        with pytest.raises(AppError) as failure:
            native_posix_stage.create(parent, b".eml-remove-second-stage.tmp")
        assert failure.value.code is ExitCode.INTERNAL_ERROR
        assert not (tmp_path / ".eml-remove-second-stage.tmp").exists()
    finally:
        native_posix_stage.discard(parent, stage, name)
        os.close(stage)
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
@pytest.mark.parametrize("failure_point", ["directory", "candidate"])
def test_private_stage_create_preserves_failed_acquisition(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, failure_point: str
) -> None:
    """A failed open cannot establish ownership or silently publish anything."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-failed-acquisition.tmp"
    original_open = os.open

    def fail_selected_open(
        path: str | bytes,
        flags: int,
        mode: int = 0o777,
        *,
        dir_fd: int | None = None,
    ) -> int:
        selected = name if failure_point == "directory" else b"candidate"
        if path == selected:
            raise OSError(errno.EIO, "simulated acquisition failure")
        return original_open(path, flags, mode, dir_fd=dir_fd)

    try:
        with monkeypatch.context() as context:
            context.setattr(os, "open", fail_selected_open)
            with pytest.raises(OSError, match="simulated acquisition failure"):
                native_posix_stage.create(parent, name)
        assert parent.private_stage_directory_fd is None
        outer = tmp_path / os.fsdecode(name)
        if failure_point == "directory":
            # Without a held descriptor, a raced outer name cannot be deleted safely.
            assert outer.is_dir()
            assert stat.S_IMODE(outer.stat().st_mode) & 0o077 == 0
            outer.rmdir()
        else:
            assert not outer.exists()
    finally:
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_preserves_both_acquisition_and_cleanup_failures(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A changed directory and failed descriptor close remain distinct causes."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-failed-close.tmp"
    original_close = os.close

    def close_then_fail(descriptor: int) -> None:
        original_close(descriptor)
        raise OSError(errno.EIO, "simulated stage close failure")

    try:
        with monkeypatch.context() as context:
            context.setattr(native_posix_stage, "_same_directory", lambda *_: False)
            context.setattr(os, "close", close_then_fail)
            with pytest.raises(BaseExceptionGroup) as caught:
                native_posix_stage.create(parent, name)
        assert len(caught.value.exceptions) == 2
        assert isinstance(caught.value.exceptions[0], AppError)
        assert isinstance(caught.value.exceptions[1], OSError)
        assert parent.private_stage_directory_fd is None
        (tmp_path / os.fsdecode(name)).rmdir()
    finally:
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
@pytest.mark.parametrize("faults", [False, True])
def test_partial_acquisition_cleanup_releases_handles_and_reports_faults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, faults: bool
) -> None:
    """Failure cleanup records independent file, unlink, rmdir and fd errors."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-partial-acquisition.tmp"
    outer = tmp_path / os.fsdecode(name)
    os.mkdir(name, mode=0o700, dir_fd=parent.descriptor)
    directory_fd = os.open(outer, os.O_RDONLY | os.O_DIRECTORY)
    file_fd = os.open(
        b"candidate", os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=directory_fd
    )
    original_close = os.close

    def close_then_fail(descriptor: int) -> None:
        original_close(descriptor)
        raise OSError(errno.EIO, "simulated close failure")

    def fail_unlink(_name: bytes, *, dir_fd: int) -> None:
        assert dir_fd == directory_fd
        raise OSError(errno.EIO, "simulated unlink failure")

    def fail_rmdir(_name: bytes, *, dir_fd: int) -> None:
        assert dir_fd == parent.descriptor
        raise OSError(errno.EIO, "simulated rmdir failure")

    try:
        with monkeypatch.context() as context:
            if faults:
                context.setattr(os, "close", close_then_fail)
                context.setattr(os, "unlink", fail_unlink)
                context.setattr(os, "rmdir", fail_rmdir)
            failures = creation_cleanup(parent, name, directory_fd, file_fd)
        assert len(failures) == (4 if faults else 0)
        if faults:
            assert outer.is_dir()
            assert (outer / "candidate").is_file()
            (outer / "candidate").unlink()
            outer.rmdir()
        else:
            assert not outer.exists()
    finally:
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
@pytest.mark.parametrize(
    "fault",
    ["missing-namespace", "directory-handle", "symlink-entry", "replaced-entry"],
)
def test_private_stage_publish_rejects_untrusted_source(
    tmp_path: Path, fault: str
) -> None:
    """Neither a wrong handle nor a swapped payload can cross visibility."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-untrusted-candidate.tmp"
    stage = native_posix_stage.create(parent, name)
    source_fd = parent.private_stage_directory_fd
    assert source_fd is not None
    borrowed_fd: int | None = None
    os.write(stage, b"trusted")
    try:
        if fault == "missing-namespace":
            parent.private_stage_directory_fd = None
        elif fault == "directory-handle":
            borrowed_fd = os.dup(source_fd)
        elif fault in {"symlink-entry", "replaced-entry"}:
            os.unlink(b"candidate", dir_fd=source_fd)
            if fault == "symlink-entry":
                os.symlink(b"unrelated", b"candidate", dir_fd=source_fd)
            else:
                substitute = os.open(
                    b"candidate",
                    os.O_WRONLY | os.O_CREAT | os.O_EXCL,
                    0o600,
                    dir_fd=source_fd,
                )
                os.write(substitute, b"untrusted")
                os.close(substitute)
        candidate_fd = borrowed_fd if borrowed_fd is not None else stage
        with pytest.raises(AppError) as failure:
            native_posix_stage.publish(parent, candidate_fd, name, b"out.eml")
        expected = (
            ExitCode.INTERNAL_ERROR
            if fault == "missing-namespace"
            else ExitCode.OUTPUT_CONFLICT
        )
        assert failure.value.code is expected
        assert not (tmp_path / "out.eml").exists()
    finally:
        parent.private_stage_directory_fd = source_fd
        native_posix_stage.discard(parent, stage, name)
        if borrowed_fd is not None:
            os.close(borrowed_fd)
        os.close(stage)
        native_posix.close_bound_directory(parent)


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_discard_requires_owner_but_accepts_moved_candidate(
    tmp_path: Path,
) -> None:
    """The Darwin rename case leaves an empty stage directory to remove."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-renamed-candidate.tmp"
    stage = native_posix_stage.create(parent, name)
    source_fd = parent.private_stage_directory_fd
    assert source_fd is not None
    try:
        parent.private_stage_directory_fd = None
        with pytest.raises(AppError) as failure:
            native_posix_stage.discard(parent, stage, name)
        assert failure.value.code is ExitCode.INTERNAL_ERROR
        parent.private_stage_directory_fd = source_fd
        os.unlink(b"candidate", dir_fd=source_fd)
        native_posix_stage.discard(parent, stage, name)
        assert not (tmp_path / os.fsdecode(name)).exists()
    finally:
        parent.private_stage_directory_fd = source_fd
        os.close(stage)
        native_posix.close_bound_directory(parent)


@pytest.mark.parametrize("both_fail", [False, True])
def test_posix_directory_close_keeps_independent_handle_failures(
    monkeypatch: pytest.MonkeyPatch, *, both_fail: bool
) -> None:
    """Both held descriptors close even if closing the first already failed."""
    parent = BoundDirectory(901, windows=False, private_stage_directory_fd=902)
    seen: list[int] = []

    def close_with_fault(descriptor: int) -> None:
        seen.append(descriptor)
        if descriptor == 902 or both_fail:
            raise OSError(errno.EIO, "simulated close failure")

    with monkeypatch.context() as context:
        context.setattr(os, "close", close_with_fault)
        if both_fail:
            with pytest.raises(BaseExceptionGroup) as caught:
                native_posix.close_bound_directory(parent)
            assert len(caught.value.exceptions) == 2
        else:
            with pytest.raises(OSError, match="simulated close failure"):
                native_posix.close_bound_directory(parent)
    assert seen == [902, 901]
    assert parent.private_stage_directory_fd is None


@pytest.mark.skipif(os.name == "nt", reason="POSIX directory-descriptor primitives")
def test_private_stage_refuses_failed_acl_clearing_before_writing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Failure to remove an inherited ACL cannot expose confidential bytes."""
    parent = BoundDirectory(os.open(tmp_path, os.O_RDONLY | os.O_DIRECTORY), windows=False)
    name = b".eml-remove-acl-failure.tmp"
    try:
        with monkeypatch.context() as context:
            context.setattr(
                native_posix_stage.native_posix_acl,
                "restrict",
                lambda _fd: (_ for _ in ()).throw(OSError(errno.EACCES, "ACL denied")),
            )
            with pytest.raises(OSError, match="ACL denied"):
                native_posix_stage.create(parent, name)
        assert parent.private_stage_directory_fd is None
        assert not (tmp_path / os.fsdecode(name)).exists()
    finally:
        native_posix.close_bound_directory(parent)
