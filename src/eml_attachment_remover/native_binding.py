"""Cross-platform dispatch for exact source and destination filesystem binding."""

from __future__ import annotations

import os
import secrets

from . import native_posix as _posix
from . import native_windows_binding as _windows
from .domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExistingEntry,
    ExitCode,
    FileIdentity,
    SourceSnapshot,
)
from .native_values import MAX_RAW_BYTES, validate_argument


def _bind_destination(request: str) -> BoundDestination:
    expanded = validate_argument(request)
    if os.name == "nt":
        return _windows.bind_destination(request, expanded)
    return _posix.bind_destination(request, expanded)


def _inspect_source_identity(request: str) -> FileIdentity:
    expanded = validate_argument(request)
    if os.name == "nt":
        return _windows.inspect_source_identity(expanded)
    return _posix.inspect_source_identity(expanded)


def _read_source(request: str) -> SourceSnapshot:
    expanded = validate_argument(request)
    if os.name == "nt":
        return _windows.read_source(request, expanded)
    return _posix.read_source(request, expanded)


def _open_bound_destination(destination: BoundDestination) -> BoundDirectory:
    if os.name == "nt":
        return _windows.open_bound_destination(destination)
    return _posix.open_bound_destination(destination)


def _close_bound_directory(directory: BoundDirectory) -> None:
    if directory.windows:
        _windows.close_bound_directory(directory)
    else:
        _posix.close_bound_directory(directory)


def _descriptor_identity(descriptor: int) -> FileIdentity:
    if os.name == "nt":
        return _windows.descriptor_identity(descriptor)
    return _posix.descriptor_identity(descriptor)


def _create_private_stage(directory: BoundDirectory, name: bytes | str) -> int:
    if directory.windows:
        return _windows.create_private_stage(directory, _unicode_name(name))
    return _posix.create_private_stage(directory, _byte_name(name))


def _private_stage_name() -> bytes | str:
    value = f".eml-remove-{secrets.token_hex(16)}.tmp"
    return value if os.name == "nt" else value.encode()


def _publish_stage_no_replace(
    directory: BoundDirectory,
    stage_fd: int,
    stage_name: bytes | str,
    destination_name: bytes | str,
) -> bool:
    if directory.windows:
        return _windows.publish_stage_no_replace(
            directory,
            stage_fd,
            _unicode_name(stage_name),
            _unicode_name(destination_name),
        )
    return _posix.publish_stage_no_replace(
        directory,
        stage_fd,
        _byte_name(stage_name),
        _byte_name(destination_name),
    )


def _child_lstat(directory: BoundDirectory, name: bytes | str) -> FileIdentity | None:
    if directory.windows:
        return _windows.child_lstat(directory, _unicode_name(name))
    return _posix.child_lstat(directory, _byte_name(name))


def _open_child_nofollow(directory: BoundDirectory, name: bytes | str) -> int:
    if directory.windows:
        return _windows.open_child_nofollow(directory, _unicode_name(name))
    return _posix.open_child_nofollow(directory, _byte_name(name))


def _discard_private_stage(
    directory: BoundDirectory, stage_fd: int, name: bytes | str
) -> None:
    if directory.windows:
        _windows.discard_private_stage(directory, stage_fd, _unicode_name(name))
    else:
        _posix.discard_private_stage(directory, stage_fd, _byte_name(name))


def _sync_bound_directory(directory: BoundDirectory) -> str:
    if directory.windows:
        return _windows.sync_bound_directory(directory)
    return _posix.sync_bound_directory(directory)


def _existing_identity(destination: BoundDestination) -> FileIdentity | None:
    directory = _open_bound_destination(destination)
    try:
        return _child_lstat(directory, destination.basename)
    finally:
        _close_bound_directory(directory)


def _read_existing(destination: BoundDestination) -> ExistingEntry | None:
    directory = _open_bound_destination(destination)
    descriptor = -1
    try:
        try:
            descriptor = _open_child_nofollow(directory, destination.basename)
        except FileNotFoundError:
            return None
        before = _descriptor_identity(descriptor)
        raw = _read_all(descriptor)
        after = _descriptor_identity(descriptor)
        if before != after or _child_lstat(directory, destination.basename) != before:
            raise AppError(
                ExitCode.OUTPUT_CONFLICT,
                "existing output changed while verified",
            )
        return ExistingEntry(before, raw)
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        _close_bound_directory(directory)


def _read_all(descriptor: int) -> bytes:
    chunks: list[bytes] = []
    size = 0
    while chunk := os.read(descriptor, min(1024 * 1024, MAX_RAW_BYTES - size + 1)):
        size += len(chunk)
        if size > MAX_RAW_BYTES:
            raise AppError(
                ExitCode.OUTPUT_CONFLICT,
                "existing output exceeds source limit",
            )
        chunks.append(chunk)
    return b"".join(chunks)


def _byte_name(value: bytes | str) -> bytes:
    if isinstance(value, bytes):
        return value
    message = "POSIX native name must be bytes"
    raise TypeError(message)


def _unicode_name(value: bytes | str) -> str:
    if isinstance(value, str):
        return value
    message = "Windows native name must be Unicode"
    raise TypeError(message)


bind_destination = _bind_destination
inspect_source_identity = _inspect_source_identity
read_source = _read_source
open_bound_destination = _open_bound_destination
close_bound_directory = _close_bound_directory
descriptor_identity = _descriptor_identity
create_private_stage = _create_private_stage
private_stage_name = _private_stage_name
publish_stage_no_replace = _publish_stage_no_replace
child_lstat = _child_lstat
open_child_nofollow = _open_child_nofollow
discard_private_stage = _discard_private_stage
sync_bound_directory = _sync_bound_directory
existing_identity = _existing_identity
read_existing = _read_existing
