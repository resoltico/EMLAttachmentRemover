"""Publish verified files atomically without replacing existing entries."""

from __future__ import annotations

import ctypes
import errno
import os
import sys
from typing import TYPE_CHECKING, Final

from .models import CliError, ExitCode

if TYPE_CHECKING:
    from pathlib import Path

AT_FDCWD: Final = -100
RENAME_EXCL: Final = 0x00000004
RENAME_NOREPLACE: Final = 1


def _native_no_replace(temporary: Path, destination: Path) -> bool:
    """Attempt a platform-native atomic no-replace rename.

    Returns:
        ``True`` when native publication was attempted, otherwise ``False``.

    Raises:
        OSError: If the native operation fails.

    """
    library = ctypes.CDLL(None, use_errno=True)
    source = os.fsencode(temporary)
    target = os.fsencode(destination)
    if sys.platform == "darwin":
        try:
            operation = library.renamex_np
        except AttributeError:
            return False
        operation.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        operation.restype = ctypes.c_int
        result = operation(source, target, RENAME_EXCL)
    elif sys.platform.startswith("linux"):
        try:
            operation = library.renameat2
        except AttributeError:
            return False
        operation.argtypes = [
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.c_char_p,
            ctypes.c_uint,
        ]
        operation.restype = ctypes.c_int
        result = operation(AT_FDCWD, source, AT_FDCWD, target, RENAME_NOREPLACE)
    else:
        return False
    if result == 0:
        return True
    error_number = ctypes.get_errno()
    if error_number in {errno.ENOSYS, errno.ENOTSUP}:
        return False
    raise OSError(error_number, os.strerror(error_number), destination)


def _link_and_remove_temporary(temporary: Path, destination: Path) -> None:
    """Hard-link one output and require sensitive temporary-file cleanup.

    Raises:
        CliError: If the published output exists but temporary cleanup fails.

    """
    destination.hardlink_to(temporary)
    try:
        temporary.unlink()
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"published verified output at {destination}, but could not remove "
            f"sensitive temporary file {temporary}: {exc}",
        ) from exc


def publish_without_clobber(temporary: Path, destination: Path) -> None:
    """Publish one file atomically without replacing another actor's entry.

    Raises:
        CliError: If another entry wins the race or publication fails.

    """
    try:
        if os.name == "nt":
            temporary.rename(destination)
        elif not _native_no_replace(temporary, destination):
            _link_and_remove_temporary(temporary, destination)
    except FileExistsError as exc:
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            f"output was created by another process: {destination}",
        ) from exc
    except OSError as exc:
        if _entry_exists(destination):
            raise CliError(
                ExitCode.OUTPUT_CONFLICT,
                f"output was created by another process: {destination}",
            ) from exc
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not publish verified output at {destination}: {exc}",
        ) from exc


def _entry_exists(destination: Path) -> bool:
    """Return whether an entry exists after a failed publication attempt.

    Returns:
        ``True`` only when the entry can be inspected.

    """
    try:
        destination.lstat()
    except OSError:
        return False
    return True
