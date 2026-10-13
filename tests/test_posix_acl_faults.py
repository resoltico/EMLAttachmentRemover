"""Private stage ACL clearing, including native failures and unsupported volumes."""

from __future__ import annotations

import ctypes
import errno
import os
import platform
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from eml_attachment_remover import native_posix_acl


def _native(
    monkeypatch: pytest.MonkeyPatch,
    *,
    initialized: int = 41,
    written: int = 0,
    released: int = 0,
    failure: int = errno.EACCES,
) -> tuple[Mock, Mock, Mock]:
    """Install signature-bearing fakes of Darwin's ACL operations.

    Returns:
        The fake allocation, set, and release operations, in that order.

    """
    init = Mock(return_value=initialized)
    set_fd = Mock(return_value=written)
    free = Mock(return_value=released)
    library = SimpleNamespace(acl_init=init, acl_set_fd=set_fd, acl_free=free)
    monkeypatch.setattr(platform, "system", lambda: "Darwin")
    monkeypatch.setattr(
        ctypes, "CDLL", lambda *_args, **_kwargs: library
    )
    monkeypatch.setattr(ctypes, "get_errno", lambda: failure)
    return init, set_fd, free


def test_non_darwin_restricts_mode_without_loading_acl_library(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The portable POSIX branch needs no platform-specific native library."""
    modes: list[tuple[int, int]] = []
    monkeypatch.setattr(platform, "system", lambda: "Linux")
    monkeypatch.setattr(
        os,
        "fchmod",
        lambda fd, mode: modes.append((fd, mode)),
        raising=False,
    )
    monkeypatch.setattr(
        ctypes,
        "CDLL",
        lambda *_args, **_kwargs: pytest.fail("unexpected ACL library load"),
    )
    native_posix_acl.restrict(7)
    assert modes == [(7, 0o700)]


@pytest.mark.parametrize(
    ("written", "released", "error", "expected"),
    [
        (0, 0, errno.EACCES, None),
        (-1, 0, errno.ENOTSUP, None),
        (-1, 0, errno.EACCES, OSError),
        (0, -1, errno.EACCES, OSError),
        (-1, -1, errno.EACCES, BaseExceptionGroup),
    ],
)
def test_darwin_acl_clearing_preserves_native_errors(
    monkeypatch: pytest.MonkeyPatch,
    written: int,
    released: int,
    error: int,
    expected: type[BaseException] | None,
) -> None:
    """Unsupported ACL volumes use mode bits; all other errors fail closed."""
    chmod = Mock()
    monkeypatch.setattr(os, "fchmod", chmod, raising=False)
    init, set_fd, free = _native(
        monkeypatch, written=written, released=released, failure=error
    )
    if expected is None:
        native_posix_acl.restrict(17)
    else:
        with pytest.raises(expected):
            native_posix_acl.restrict(17)
    chmod.assert_called_once_with(17, 0o700)
    init.assert_called_once_with(0)
    set_fd.assert_called_once_with(17, 41)
    free.assert_called_once_with(41)
    assert set_fd.argtypes is not None
    assert set_fd.restype is not None


def test_acl_init_failure_never_attempts_write_or_free(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A missing ACL descriptor fails before staging candidate creation."""
    monkeypatch.setattr(
        os, "fchmod", lambda *_args: None, raising=False
    )
    init, set_fd, free = _native(monkeypatch, initialized=0)
    with pytest.raises(OSError, match=os.strerror(errno.EACCES)):
        native_posix_acl.restrict(9)
    init.assert_called_once_with(0)
    set_fd.assert_not_called()
    free.assert_not_called()


def test_chmod_failure_never_loads_acl_library(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A failed restrictive BSD mode cannot be ignored even on macOS."""
    monkeypatch.setattr(platform, "system", lambda: "Darwin")
    monkeypatch.setattr(
        os,
        "fchmod",
        lambda *_args: (_ for _ in ()).throw(OSError(errno.EPERM, "chmod")),
        raising=False,
    )
    with pytest.raises(OSError, match="chmod"):
        native_posix_acl.restrict(18)
