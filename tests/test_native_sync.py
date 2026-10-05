"""Platform synchronization requests preserve unsupported and failure distinctions."""

from __future__ import annotations

import ctypes
import errno
from types import SimpleNamespace

import pytest

from eml_attachment_remover import atomic_publish


class FcntlOperation:
    """Record the exact Darwin variadic ABI call."""

    def __init__(self, result: int) -> None:
        """Start with a prescribed native return value."""
        self.result = result
        self.calls: list[tuple[int, int, int]] = []
        self.argtypes: list[object] = []
        self.restype: object = None

    def __call__(self, descriptor: int, command: int, argument: ctypes.c_int) -> int:
        self.calls.append((descriptor, command, argument.value))
        return self.result


@pytest.mark.parametrize("failure", [0, errno.EINVAL, errno.ENOTSUP, errno.EIO])
def test_darwin_sync_requests_full_flush_and_does_not_hide_io_errors(
    monkeypatch: pytest.MonkeyPatch, failure: int
) -> None:
    operation = FcntlOperation(0 if failure == 0 else -1)
    calls: list[int] = []
    monkeypatch.setattr(atomic_publish.__dict__["os"], "fsync", calls.append)
    monkeypatch.setattr(atomic_publish.__dict__["platform"], "system", lambda: "Darwin")
    monkeypatch.setattr(
        atomic_publish.__dict__["ctypes"],
        "CDLL",
        lambda *_args, **_kwargs: SimpleNamespace(fcntl=operation),
    )
    monkeypatch.setattr(atomic_publish.__dict__["ctypes"], "get_errno", lambda: failure)
    if failure == errno.EIO:
        with pytest.raises(OSError, match="Input/output error") as captured:
            atomic_publish.sync_descriptor(37)
        assert captured.value.errno == errno.EIO
    else:
        atomic_publish.sync_descriptor(37)
    assert calls == [37]
    assert operation.calls == [(37, 51, 0)]
    assert operation.argtypes == [ctypes.c_int, ctypes.c_int]
    assert operation.restype is ctypes.c_int


def test_other_platform_sync_does_not_load_darwin_apis(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[int] = []
    monkeypatch.setattr(atomic_publish.__dict__["platform"], "system", lambda: "Linux")
    monkeypatch.setattr(atomic_publish.__dict__["os"], "fsync", calls.append)
    atomic_publish.sync_descriptor(37)
    assert calls == [37]
