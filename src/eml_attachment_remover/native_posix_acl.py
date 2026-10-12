"""Clear inherited macOS ACLs before writing confidential POSIX staging bytes."""

from __future__ import annotations

import ctypes
import errno
import os
import sys


def restrict(directory_fd: int) -> None:
    """Enforce owner-only stage access, including inherited macOS ACL entries.

    Raises:
        OSError: If owner-private permissions cannot be established.
        BaseExceptionGroup: If applying and releasing the native ACL both fail.

    """
    os.fchmod(directory_fd, 0o700)
    if sys.platform != "darwin":
        return
    library = ctypes.CDLL(None, use_errno=True)
    initialize = library.acl_init
    initialize.argtypes = [ctypes.c_int]
    initialize.restype = ctypes.c_void_p
    set_acl = library.acl_set_fd
    set_acl.argtypes = [ctypes.c_int, ctypes.c_void_p]
    set_acl.restype = ctypes.c_int
    release = library.acl_free
    release.argtypes = [ctypes.c_void_p]
    release.restype = ctypes.c_int
    acl = initialize(0)
    if not acl:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))
    written = set_acl(directory_fd, acl)
    set_error = ctypes.get_errno() if written != 0 else 0
    released = release(acl)
    free_error = ctypes.get_errno() if released != 0 else 0
    failures: list[OSError] = []
    # A volume without ACL support cannot inherit ACL permissions.
    if set_error not in {0, errno.ENOTSUP, errno.EOPNOTSUPP}:
        failures.append(OSError(set_error, os.strerror(set_error)))
    if free_error:
        failures.append(OSError(free_error, os.strerror(free_error)))
    if len(failures) == 1:
        raise failures[0]
    if failures:
        combined = BaseExceptionGroup("private staging ACL failures", failures)
        raise combined
