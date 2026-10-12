"""Descriptor-held, owner-private POSIX staging for atomic no-replace publication."""

from __future__ import annotations

import os
import stat

from . import native_posix_acl
from .atomic_publish import publish_no_replace
from .domain import AppError, BoundDirectory, ExitCode

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
    """Check whether the outer name still resolves to the protected directory."""
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


def _creation_cleanup(
    parent: BoundDirectory, name: bytes, directory_fd: int, file_fd: int
) -> list[BaseException]:
    """Release acquired handles, removing only our identified private namespace."""
    failures: list[BaseException] = []
    if file_fd >= 0:
        try:
            os.close(file_fd)
        except BaseException as exc:
            failures.append(exc)
    if directory_fd >= 0:
        try:
            os.unlink(_PAYLOAD, dir_fd=directory_fd)
        except FileNotFoundError:
            pass
        except BaseException as exc:
            failures.append(exc)
        try:
            if _same_directory(parent, name, directory_fd):
                os.rmdir(name, dir_fd=parent.descriptor)
        except BaseException as exc:
            failures.append(exc)
        try:
            os.close(directory_fd)
        except BaseException as exc:
            failures.append(exc)
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
        if not _same_directory(parent, name, directory_fd):
            raise AppError(
                ExitCode.OUTPUT_CONFLICT, "private staging directory changed"
            )
        file_fd = os.open(_PAYLOAD, _FILE_FLAGS, 0o600, dir_fd=directory_fd)
    except BaseException as primary:
        failures = _creation_cleanup(parent, name, directory_fd, file_fd)
        if failures:
            raise BaseExceptionGroup(
                "private-stage acquisition and cleanup failures", [primary, *failures]
            ) from primary
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
    """Remove only the protected candidate and its original private directory."""
    source_fd = parent.private_stage_directory_fd
    if source_fd is None:
        raise AppError(ExitCode.INTERNAL_ERROR, "private stage directory is absent")
    try:
        os.unlink(_PAYLOAD, dir_fd=source_fd)
    except FileNotFoundError:
        # Darwin rename moves the inner candidate, leaving this name absent.
        pass
    if not _same_directory(parent, name, source_fd):
        raise AppError(
            ExitCode.OUTPUT_CONFLICT, "private stage directory was renamed or replaced"
        )
    os.rmdir(name, dir_fd=parent.descriptor)
