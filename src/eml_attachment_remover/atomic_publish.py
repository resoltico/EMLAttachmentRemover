"""Descriptor-relative POSIX no-replace publication primitives."""

from __future__ import annotations

import ctypes
import errno
import os
import platform
from typing import Final

from .domain import AppError, ExitCode

RENAME_EXCL: Final = 0x00000004
F_FULLFSYNC: Final = 51
_UNSUPPORTED_SYNC_ERRNOS: Final = frozenset({
    errno.EINVAL,
    errno.ENOTSUP,
    errno.EOPNOTSUPP,
})


def _darwin_rename_exclusive(
    parent_fd: int, staged_name: bytes, destination_name: bytes
) -> None:
    """Move one child name atomically without replacing a destination child.

    Raises:
        AppError: If the destination exists or Darwin's rename primitive fails.

    """
    library = ctypes.CDLL(None, use_errno=True)
    operation = library.renameatx_np
    operation.argtypes = [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    operation.restype = ctypes.c_int
    result = operation(parent_fd, staged_name, parent_fd, destination_name, RENAME_EXCL)
    if result == 0:
        return
    failure = ctypes.get_errno()
    if failure == errno.EEXIST:
        raise AppError(ExitCode.OUTPUT_CONFLICT, "destination already exists")
    code = (
        ExitCode.ATOMIC_PUBLICATION_UNSUPPORTED
        if failure in {errno.ENOTSUP, errno.EOPNOTSUPP}
        else ExitCode.WRITE_ERROR
    )
    message = os.strerror(failure)
    raise AppError(code, f"could not publish candidate: {message}")


def _link_exclusive(
    parent_fd: int, staged_name: bytes, destination_name: bytes
) -> None:
    """Create a second hard link as a portable no-replace publication edge.

    Raises:
        AppError: If the destination exists or the hard-link operation fails.

    """
    try:
        os.link(
            staged_name,
            destination_name,
            src_dir_fd=parent_fd,
            dst_dir_fd=parent_fd,
        )
    except FileExistsError as exc:
        raise AppError(ExitCode.OUTPUT_CONFLICT, "destination already exists") from exc
    except OSError as exc:
        code = (
            ExitCode.ATOMIC_PUBLICATION_UNSUPPORTED
            if exc.errno in {errno.ENOTSUP, errno.EOPNOTSUPP}
            else ExitCode.WRITE_ERROR
        )
        raise AppError(code, f"could not publish candidate: {exc}") from exc


def publish_no_replace(
    parent_fd: int, staged_name: bytes, destination_name: bytes
) -> bool:
    """Publish a staged child without replacement and return link-cleanup need.

    Returns:
        Whether the stage name remains linked and must be unlinked after a successful
        final-address receipt.

    """
    if platform.system() == "Darwin":
        _darwin_rename_exclusive(parent_fd, staged_name, destination_name)
        return False
    _link_exclusive(parent_fd, staged_name, destination_name)
    return True


def sync_descriptor(descriptor: int) -> None:
    """Complete fsync and request a hardware-cache flush where Darwin supports it.

    Unsupported full-flush requests retain the portable fsync contract. Neither
    syscall completion nor hardware acknowledgement proves power-loss survival.

    Raises:
        OSError: If synchronization fails beyond an unsupported full flush.

    """
    os.fsync(descriptor)
    if platform.system() != "Darwin":
        return
    library = ctypes.CDLL(None, use_errno=True)
    operation = library.fcntl
    operation.argtypes = [ctypes.c_int, ctypes.c_int]
    operation.restype = ctypes.c_int
    # F_FULLFSYNC from Apple's sys/fcntl.h; the third argument is ignored.
    if operation(descriptor, F_FULLFSYNC, ctypes.c_int(0)) == 0:
        return
    failure = ctypes.get_errno()
    if failure not in _UNSUPPORTED_SYNC_ERRNOS:
        raise OSError(failure, os.strerror(failure))


def sync_directory(parent_fd: int) -> str:
    """Return the directory-durability receipt after a visible publication.

    Returns:
        ``succeeded`` when fsync completes and ``unsupported`` for known platform
        limitations.

    Raises:
        AppError: If directory synchronization fails.

    """
    try:
        sync_descriptor(parent_fd)
    except OSError as exc:
        if exc.errno in _UNSUPPORTED_SYNC_ERRNOS:
            return "unsupported"
        raise AppError(
            ExitCode.WRITE_ERROR, f"could not sync destination directory: {exc}"
        ) from exc
    return "succeeded"
