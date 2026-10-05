"""Windows progress access checks preserve caller handles and reject unknown results."""

from __future__ import annotations

import ctypes
from types import SimpleNamespace

import pytest

from eml_attachment_remover import native_windows_pipe, native_windows_runtime


@pytest.mark.parametrize(
    ("status", "length", "access"), [(0, 56, 2), (0, 56, 1), (-1, 56, 2), (0, 55, 2)]
)
def test_object_access_information_uses_exact_abi_and_bounded_results(
    monkeypatch: pytest.MonkeyPatch, status: int, length: int, access: int
) -> None:
    calls: list[tuple[int, int, int]] = []

    def query(
        handle: ctypes.c_void_p,
        kind: int,
        output: ctypes.c_void_p,
        size: int,
        returned: ctypes.c_void_p,
    ) -> int:
        calls.append((int(handle.value or 0), kind, size))
        information = ctypes.cast(
            output, ctypes.POINTER(native_windows_pipe.ObjectBasicInformation)
        ).contents
        information.granted_access = access
        ctypes.cast(returned, ctypes.POINTER(ctypes.c_uint32)).contents.value = length
        return status

    # A callable class keeps ctypes-style writable signature attributes.

    class Operation:
        def __init__(self) -> None:
            """Retain signature fields separately for each native call fixture."""
            self.argtypes: list[object] = []
            self.restype: object = None

        def __call__(
            self,
            handle: ctypes.c_void_p,
            kind: int,
            output: ctypes.c_void_p,
            size: int,
            returned: ctypes.c_void_p,
        ) -> int:
            return query(handle, kind, output, size, returned)

    native = Operation()
    monkeypatch.setattr(
        native_windows_pipe,
        "ctypes_attribute",
        lambda _name: lambda *_args, **_kwargs: SimpleNamespace(NtQueryObject=native),
    )
    monkeypatch.setattr(native_windows_runtime.Msvcrt, "get_osfhandle", lambda _fd: 199)
    if status < 0 or length != 56:
        with pytest.raises(OSError, match="access information is unavailable"):
            native_windows_pipe.has_write_access(19)
    else:
        assert native_windows_pipe.has_write_access(19) is (access == 2)
    assert calls == [(199, 0, 56)]
    assert native.restype is ctypes.c_int32
    assert len(native.argtypes) == 5
    assert native_windows_pipe.ObjectBasicInformation.granted_access.offset == 4
    assert ctypes.sizeof(native_windows_pipe.ObjectBasicInformation) == 56
