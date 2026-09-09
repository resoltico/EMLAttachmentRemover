"""Descriptor-relative POSIX implementation behind the cross-platform binder."""

from __future__ import annotations

import hashlib
import os
import stat
from typing import Final

from .atomic_publish import publish_no_replace, sync_directory
from .domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
    SourceSnapshot,
)
from .native_values import MAX_RAW_BYTES, path_value

_CWD_FLAGS: Final = (
    os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_CLOEXEC", 0)
)
_START_CWD_FD: Final = os.open(b".", _CWD_FLAGS)


def _identity(metadata: os.stat_result) -> FileIdentity:
    return FileIdentity(
        metadata.st_dev,
        metadata.st_ino,
        stat.filemode(metadata.st_mode),
        metadata.st_ctime_ns,
    )


def _same_directory(left: FileIdentity, right: FileIdentity) -> bool:
    return (left.device, left.inode, left.file_type) == (
        right.device,
        right.inode,
        right.file_type,
    )


def _split(path: str) -> tuple[bytes, bytes]:
    raw = os.fsencode(path)
    parent, separator, basename = raw.rpartition(b"/")
    return (b"." if not separator else parent or b"/"), basename


def _directory(expression: bytes) -> int:
    if expression.startswith(b"/"):
        return os.open(expression, _CWD_FLAGS)
    return os.open(expression, _CWD_FLAGS, dir_fd=_START_CWD_FD)


def _parent(path: str) -> tuple[int, str, bytes]:
    expression, basename = _split(path)
    try:
        return _directory(expression), os.fsdecode(expression), basename
    except OSError as exc:
        raise AppError(
            ExitCode.INPUT_ERROR, f"could not open path parent: {exc}"
        ) from exc


def _read_all(descriptor: int) -> bytes:
    chunks: list[bytes] = []
    size = 0
    while chunk := os.read(descriptor, min(1024 * 1024, MAX_RAW_BYTES - size + 1)):
        size += len(chunk)
        if size > MAX_RAW_BYTES:
            raise AppError(
                ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
            )
        chunks.append(chunk)
    return b"".join(chunks)


def _bind_destination(request: str, expanded: str) -> BoundDestination:
    descriptor, parent, basename = _parent(expanded)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISDIR(metadata.st_mode):
            raise AppError(
                ExitCode.WRITE_ERROR, "destination parent is not a directory"
            )
        return BoundDestination(
            path_value(request), path_value(parent), basename, _identity(metadata)
        )
    finally:
        os.close(descriptor)


def _inspect_source_identity(expanded: str) -> FileIdentity:
    parent, _parent_text, basename = _parent(expanded)
    descriptor = -1
    try:
        descriptor = os.open(
            basename,
            os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_CLOEXEC", 0),
            dir_fd=parent,
        )
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise AppError(
                ExitCode.INPUT_ERROR, "selected source is not a regular file"
            )
        return _identity(metadata)
    except OSError as exc:
        raise AppError(
            ExitCode.INPUT_ERROR, f"could not inspect source: {exc}"
        ) from exc
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        os.close(parent)


def _read_source(request: str, expanded: str) -> SourceSnapshot:
    parent, parent_text, basename = _parent(expanded)
    descriptor = -1
    try:
        descriptor = os.open(
            basename,
            os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_CLOEXEC", 0),
            dir_fd=parent,
        )
        return _snapshot(request, expanded, parent_text, basename, descriptor)
    except OSError as exc:
        raise AppError(ExitCode.INPUT_ERROR, f"could not read source: {exc}") from exc
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        os.close(parent)


def _snapshot(
    request: str, expanded: str, parent: str, basename: bytes, descriptor: int
) -> SourceSnapshot:
    before = os.fstat(descriptor)
    if not stat.S_ISREG(before.st_mode):
        raise AppError(ExitCode.INPUT_ERROR, "selected source is not a regular file")
    if before.st_size > MAX_RAW_BYTES:
        raise AppError(
            ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
        )
    raw = _read_all(descriptor)
    after = os.fstat(descriptor)
    if _identity(before) != _identity(after) or before.st_mtime_ns != after.st_mtime_ns:
        raise AppError(ExitCode.INPUT_ERROR, "source changed while it was being read")
    if len(raw) != before.st_size:
        raise AppError(ExitCode.INPUT_ERROR, "source read size changed during snapshot")
    return SourceSnapshot(
        path_value(request),
        path_value(expanded),
        path_value(parent),
        basename,
        _final_address(expanded),
        _identity(before),
        stat.S_IMODE(before.st_mode),
        raw,
        hashlib.sha256(raw).hexdigest(),
        len(raw),
    )


def _final_address(expanded: str) -> PathValue | None:
    try:
        return path_value(os.path.realpath(expanded))
    except OSError:
        return None


def _open_bound_destination(destination: BoundDestination) -> BoundDirectory:
    parent = destination.parent.text
    if parent is None:
        raise AppError(ExitCode.WRITE_ERROR, "destination parent has no native address")
    descriptor = _directory(os.fsencode(parent))
    if not _same_directory(
        _identity(os.fstat(descriptor)), destination.directory_identity
    ):
        os.close(descriptor)
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "destination parent changed after binding"
        )
    return BoundDirectory(descriptor, windows=False)


def _close_bound_directory(directory: BoundDirectory) -> None:
    os.close(directory.descriptor)


def _descriptor_identity(descriptor: int) -> FileIdentity:
    return _identity(os.fstat(descriptor))


def _create_private_stage(directory: BoundDirectory, name: bytes) -> int:
    return os.open(
        name,
        os.O_RDWR | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0),
        0o600,
        dir_fd=directory.descriptor,
    )


def _publish_stage_no_replace(
    directory: BoundDirectory,
    _stage_fd: int,
    stage_name: bytes,
    destination_name: bytes,
) -> bool:
    return publish_no_replace(directory.descriptor, stage_name, destination_name)


def _child_lstat(directory: BoundDirectory, name: bytes) -> FileIdentity | None:
    try:
        return _identity(
            os.stat(name, dir_fd=directory.descriptor, follow_symlinks=False)
        )
    except FileNotFoundError:
        return None


def _open_child_nofollow(directory: BoundDirectory, name: bytes) -> int:
    return os.open(
        name,
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
        dir_fd=directory.descriptor,
    )


def _discard_private_stage(
    directory: BoundDirectory, _stage_fd: int, name: bytes
) -> None:
    os.unlink(name, dir_fd=directory.descriptor)


def _sync_bound_directory(directory: BoundDirectory) -> str:
    return sync_directory(directory.descriptor)


bind_destination = _bind_destination
inspect_source_identity = _inspect_source_identity
read_source = _read_source
open_bound_destination = _open_bound_destination
close_bound_directory = _close_bound_directory
descriptor_identity = _descriptor_identity
create_private_stage = _create_private_stage
publish_stage_no_replace = _publish_stage_no_replace
child_lstat = _child_lstat
open_child_nofollow = _open_child_nofollow
discard_private_stage = _discard_private_stage
sync_bound_directory = _sync_bound_directory
