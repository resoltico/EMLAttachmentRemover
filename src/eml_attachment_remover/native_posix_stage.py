"""Descriptor-held, owner-private POSIX staging for atomic no-replace publication."""

from __future__ import annotations

import os
import stat
from contextlib import suppress
from typing import TYPE_CHECKING

from . import native_posix_acl
from .atomic_publish import publish_no_replace
from .domain import AppError, BoundDirectory, ExitCode

if TYPE_CHECKING:
    from collections.abc import Callable

_PAYLOAD = b"candidate"
_DIRECTORY_FLAGS = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)
_FILE_FLAGS = (
    os.O_RDWR
    | os.O_CREAT
    | os.O_EXCL
    | getattr(os, "O_NOFOLLOW", 0)
    | getattr(os, "O_CLOEXEC", 0)
)


def _same_directory(parent: BoundDirectory, name: bytes, descriptor: int) -> bool:
    """Check whether the outer name still resolves to the protected directory.

    Returns:
        Whether the external entry still identifies our private directory.

    """
    try:
        entry = os.stat(name, dir_fd=parent.descriptor, follow_symlinks=False)
    except FileNotFoundError:
        return False
    held = os.fstat(descriptor)
    return (
        stat.S_ISDIR(held.st_mode)
        and stat.S_ISDIR(entry.st_mode)
        and (held.st_dev, held.st_ino) == (entry.st_dev, entry.st_ino)
        and held.st_uid == os.geteuid()
        and stat.S_IMODE(held.st_mode) & 0o077 == 0
    )


def _attempt(failures: list[BaseException], action: Callable[[], None]) -> None:
    """Attempt one cleanup without suppressing any independent failure."""
    try:
        action()
    except BaseException as exc:  # ruff: ignore[blind-except] - preserve cancellation during independent cleanup.
        failures.append(exc)


def _unlink_candidate(directory_fd: int) -> None:
    """Remove the inner candidate only if its name still exists."""
    with suppress(FileNotFoundError):
        os.unlink(_PAYLOAD, dir_fd=directory_fd)


def _remove_private_directory(
    parent: BoundDirectory, name: bytes, directory_fd: int
) -> None:
    """Remove only our original owner-identified private directory."""
    if _same_directory(parent, name, directory_fd):
        os.rmdir(name, dir_fd=parent.descriptor)


def _require_private_directory(
    parent: BoundDirectory, name: bytes, directory_fd: int
) -> None:
    """Reject an external name that no longer identifies our owned directory.

    Raises:
        AppError: If ownership, identity or access permissions are inadequate.

    """
    if not _same_directory(parent, name, directory_fd):
        raise AppError(ExitCode.OUTPUT_CONFLICT, "private staging directory changed")


def _creation_cleanup(
    parent: BoundDirectory, name: bytes, directory_fd: int, file_fd: int
) -> list[BaseException]:
    """Release acquired owners and record all independent failures.

    Returns:
        The caught release errors, including any cancellation.

    """
    failures: list[BaseException] = []
    if file_fd >= 0:
        _attempt(failures, lambda: os.close(file_fd))
    if directory_fd >= 0:
        _attempt(failures, lambda: _unlink_candidate(directory_fd))
        _attempt(
            failures,
            lambda: _remove_private_directory(parent, name, directory_fd),
        )
        _attempt(failures, lambda: os.close(directory_fd))
    return failures


def create(parent: BoundDirectory, name: bytes) -> int:
    """Create one private stage directory with a descriptor-held candidate.

    Returns:
        The new candidate file descriptor, within the held stage directory.

    Raises:
        AppError: If its private directory identity or ownership cannot be proved.

    """
    if parent.private_stage_directory_fd is not None:
        raise AppError(ExitCode.INTERNAL_ERROR, "private stage already owned")
    os.mkdir(name, mode=0o700, dir_fd=parent.descriptor)
    directory_fd = -1
    file_fd = -1
    try:
        directory_fd = os.open(name, _DIRECTORY_FLAGS, dir_fd=parent.descriptor)
        native_posix_acl.restrict(directory_fd)
        _require_private_directory(parent, name, directory_fd)
        file_fd = os.open(_PAYLOAD, _FILE_FLAGS, 0o600, dir_fd=directory_fd)
    except BaseException as primary:
        failures = _creation_cleanup(parent, name, directory_fd, file_fd)
        if failures:
            combined = BaseExceptionGroup(
                "private-stage acquisition and cleanup failures", [primary, *failures]
            )
            raise combined from primary
        raise
    parent.private_stage_directory_fd = directory_fd
    return file_fd


def publish(
    parent: BoundDirectory, stage_fd: int, _stage_name: bytes, destination: bytes
) -> bool:
    """Publish the held namespace's candidate, never its movable outer name.

    Returns:
        Whether the source link remains for cleanup after publication.

    Raises:
        AppError: If the private file's directory entry differs from its handle.

    """
    source_fd = parent.private_stage_directory_fd
    if source_fd is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "private stage directory is absent")
    held = os.fstat(stage_fd)
    entry = os.stat(_PAYLOAD, dir_fd=source_fd, follow_symlinks=False)
    if (
        not stat.S_ISREG(held.st_mode)
        or not stat.S_ISREG(entry.st_mode)
        or (held.st_dev, held.st_ino) != (entry.st_dev, entry.st_ino)
    ):
        raise AppError(ExitCode.OUTPUT_CONFLICT, "private staged file changed")
    return publish_no_replace(source_fd, parent.descriptor, _PAYLOAD, destination)


def discard(parent: BoundDirectory, _stage_fd: int, name: bytes) -> None:
    """Remove only the protected candidate and its original private directory.

    Raises:
        AppError: If the entry no longer names the held protected directory.

    """
    source_fd = parent.private_stage_directory_fd
    if source_fd is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "private stage directory is absent")
    # Darwin rename moves the candidate and leaves no source name to remove.
    _unlink_candidate(source_fd)
    if not _same_directory(parent, name, source_fd):
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "private stage directory was renamed or replaced"
        )
    os.rmdir(name, dir_fd=parent.descriptor)
