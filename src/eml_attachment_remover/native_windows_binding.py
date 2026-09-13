"""Windows handle-rooted implementation behind the cross-platform binder."""

from __future__ import annotations

import hashlib
import ntpath
import os
from functools import cache
from pathlib import Path

from .domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    SourceSnapshot,
)
from .native_values import MAX_RAW_BYTES, path_value
from .native_windows import WindowsApi


@cache
def _api() -> WindowsApi:
    return WindowsApi()


def _capture_start_cwd() -> int | None:
    if os.name != "nt":
        return None
    return _api().open_directory(os.fspath(Path.cwd()), None)


_START_CWD_HANDLE = _capture_start_cwd()


@cache
def _cwd() -> int:
    if _START_CWD_HANDLE is None:
        message = "Windows working-directory handle is unavailable"
        raise OSError(message)
    return _START_CWD_HANDLE


def _parent(path: str) -> tuple[int, str, str]:
    parent, basename = ntpath.split(path)
    parent = parent or "."
    drive, tail = ntpath.splitdrive(parent)
    absolute = bool(drive) and tail.startswith(("\\", "/"))
    root = None if absolute else _cwd()
    try:
        return _api().open_directory(parent, root), parent, basename
    except OSError as exc:
        raise AppError(
            ExitCode.INPUT_ERROR, f"could not open path parent: {exc}"
        ) from exc


def _identity(handle: int) -> FileIdentity:
    info = _api().info(handle)
    file_type = "directory" if info.directory else "regular"
    return FileIdentity(info.volume_serial, info.file_id, file_type, info.change_time)


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
    handle, parent, basename = _parent(expanded)
    try:
        if not _api().info(handle).directory:
            raise AppError(
                ExitCode.WRITE_ERROR, "destination parent is not a directory"
            )
        return BoundDestination(
            path_value(request), path_value(parent), basename, _identity(handle)
        )
    finally:
        _api().close(handle)


def _inspect_source_identity(expanded: str) -> FileIdentity:
    parent, _parent_text, basename = _parent(expanded)
    handle: int | None = None
    try:
        handle = _api().open_child(parent, basename)
        if _api().info(handle).directory:
            raise AppError(
                ExitCode.INPUT_ERROR, "selected source is not a regular file"
            )
        return _identity(handle)
    except OSError as exc:
        raise AppError(
            ExitCode.INPUT_ERROR, f"could not inspect source: {exc}"
        ) from exc
    finally:
        if handle is not None:
            _api().close(handle)
        _api().close(parent)


def _read_source(request: str, expanded: str) -> SourceSnapshot:
    parent, parent_text, basename = _parent(expanded)
    handle: int | None = None
    descriptor: int | None = None
    try:
        handle = _api().open_child(parent, basename)
        descriptor = _api().descriptor_from_handle(handle, read_only=True)
        handle = None
        return _snapshot(request, expanded, parent_text, basename, descriptor)
    except OSError as exc:
        raise AppError(ExitCode.INPUT_ERROR, f"could not read source: {exc}") from exc
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if handle is not None:
            _api().close(handle)
        _api().close(parent)


def _snapshot(
    request: str, expanded: str, parent: str, basename: str, descriptor: int
) -> SourceSnapshot:
    reopened = _api().handle_from_descriptor(descriptor)
    before = _api().info(reopened)
    if before.directory or before.size > MAX_RAW_BYTES:
        raise AppError(ExitCode.INPUT_ERROR, "selected source is not a regular file")
    raw = _read_all(descriptor)
    after = _api().info(reopened)
    if before != after or len(raw) != before.size:
        raise AppError(ExitCode.INPUT_ERROR, "source changed while it was being read")
    final = _api().final_path(reopened)
    return SourceSnapshot(
        path_value(request),
        path_value(expanded),
        path_value(parent),
        basename,
        None if final is None else path_value(final),
        _identity(reopened),
        0o600,
        raw,
        hashlib.sha256(raw).hexdigest(),
        len(raw),
    )


def _open_bound_destination(destination: BoundDestination) -> BoundDirectory:
    parent = destination.parent.text
    if parent is None:
        raise AppError(ExitCode.WRITE_ERROR, "destination parent has no native address")
    drive, tail = ntpath.splitdrive(parent)
    absolute = parent.startswith(("\\\\", "//")) or (
        bool(drive) and tail.startswith(("\\", "/"))
    )
    try:
        handle = _api().open_directory(parent, None if absolute else _cwd())
    except OSError as exc:
        raise AppError(
            ExitCode.WRITE_ERROR, f"could not reopen destination parent: {exc}"
        ) from exc
    observed = _identity(handle)
    expected = destination.directory_identity
    if (observed.device, observed.inode, observed.file_type) != (
        expected.device,
        expected.inode,
        expected.file_type,
    ):
        _api().close(handle)
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "destination parent changed after binding"
        )
    return BoundDirectory(handle, windows=True)


def _close_bound_directory(directory: BoundDirectory) -> None:
    _api().close(directory.descriptor)


def _descriptor_identity(descriptor: int) -> FileIdentity:
    return _identity(_api().handle_from_descriptor(descriptor))


def _create_private_stage(directory: BoundDirectory, name: str) -> int:
    handle = _api().create_child(directory.descriptor, name)
    return _api().descriptor_from_handle(handle, read_only=False)


def _publish_stage_no_replace(
    directory: BoundDirectory, stage_fd: int, _stage_name: str, destination_name: str
) -> bool:
    try:
        _api().publish_no_replace(
            _api().handle_from_descriptor(stage_fd),
            directory.descriptor,
            destination_name,
        )
    except FileExistsError as exc:
        raise AppError(ExitCode.OUTPUT_CONFLICT, "destination already exists") from exc
    return False


def _child_lstat(directory: BoundDirectory, name: str) -> FileIdentity | None:
    handle: int | None = None
    try:
        handle = _api().open_child(directory.descriptor, name, no_follow=True)
        info = _api().info(handle)
        if info.directory or info.reparse:
            raise AppError(
                ExitCode.OUTPUT_CONFLICT, "destination is not a regular file"
            )
        return _identity(handle)
    except FileNotFoundError:
        return None
    finally:
        if handle is not None:
            _api().close(handle)


def _open_child_nofollow(directory: BoundDirectory, name: str) -> int:
    handle = _api().open_child(directory.descriptor, name, no_follow=True)
    try:
        _require_regular(handle)
        descriptor = _api().descriptor_from_handle(handle, read_only=True)
    except BaseException:
        _api().close(handle)
        raise
    return descriptor


def _discard_private_stage(
    _directory: BoundDirectory, stage_fd: int, _name: str
) -> None:
    _api().discard_private_stage(_api().handle_from_descriptor(stage_fd))


def _require_regular(handle: int) -> None:
    info = _api().info(handle)
    if info.directory or info.reparse:
        raise AppError(ExitCode.OUTPUT_CONFLICT, "destination is not a regular file")


def _sync_bound_directory(directory: BoundDirectory) -> str:
    _api().sync_directory(directory.descriptor)
    return "succeeded"


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
