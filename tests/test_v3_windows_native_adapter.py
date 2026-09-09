"""Synthetic Windows API contracts for the handle-rooted native adapter."""

from __future__ import annotations

import ctypes
import os
import struct
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import native_windows
from eml_attachment_remover.native_paths import MAX_PATH_BYTES
from eml_attachment_remover.native_windows import WindowsApi
from eml_attachment_remover.native_windows_abi import FILE_WRITE_DATA

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


type Response = Callable[..., int]


@dataclass
class NativeCall:
    """One permissive fake ctypes function retaining ABI declarations and calls."""

    response: Response
    argtypes: object | None = None
    restype: object | None = None
    calls: list[tuple[object, ...]] = field(default_factory=list)

    def __call__(self, *arguments: object) -> int:
        """Record and dispatch one native-call invocation.

        Returns:
            The fake native integer result.

        """
        self.calls.append(arguments)
        return self.response(*arguments)


@dataclass
class Kernel:
    """The exact kernel entry-point surface consumed by ``WindowsApi``."""

    CreateFileW: NativeCall
    CloseHandle: NativeCall
    GetFileInformationByHandleEx: NativeCall
    GetFileType: NativeCall
    GetFinalPathNameByHandleW: NativeCall
    SetFileInformationByHandle: NativeCall
    FlushFileBuffers: NativeCall


@dataclass
class Ntdll:
    """The exact ntdll entry-point surface consumed by ``WindowsApi``."""

    NtCreateFile: NativeCall
    NtSetInformationFile: NativeCall
    RtlNtStatusToDosError: NativeCall


@dataclass
class Msvcrt:
    """Minimal CRT bridge used to prove handle-to-descriptor ownership wiring."""

    opened: list[tuple[int, int]] = field(default_factory=list)

    @staticmethod
    def get_osfhandle(descriptor: int) -> int:
        """Return a deterministic native handle for one fake descriptor.

        Returns:
            The deterministic native handle associated with ``descriptor``.

        """
        return descriptor + 100

    def open_osfhandle(self, handle: int, flags: int) -> int:
        """Record one handle transfer and return a deterministic descriptor.

        Returns:
            The deterministic descriptor that owns ``handle``.

        """
        self.opened.append((handle, flags))
        return handle + 200


def _adapter(monkeypatch: MonkeyPatch) -> tuple[WindowsApi, Kernel, Ntdll]:
    kernel = Kernel(
        NativeCall(lambda *_arguments: 11),
        NativeCall(lambda *_arguments: 1),
        NativeCall(_fill_information),
        NativeCall(lambda *_arguments: 1),
        NativeCall(_final_path),
        NativeCall(lambda *_arguments: 1),
        NativeCall(lambda *_arguments: 1),
    )
    ntdll = Ntdll(
        NativeCall(_open_relative),
        NativeCall(lambda *_arguments: 0),
        NativeCall(lambda *_arguments: 5),
    )
    libraries = iter((kernel, ntdll))
    monkeypatch.setattr(os, "name", "nt")
    monkeypatch.setitem(
        native_windows.__dict__,
        "_win_dll",
        lambda: lambda *_args, **_kwargs: next(libraries),
    )
    monkeypatch.setitem(native_windows.__dict__, "_last_error", lambda: 5)
    monkeypatch.setitem(
        native_windows.__dict__, "_format_error", lambda _value: "denied"
    )
    return WindowsApi(), kernel, ntdll


def _open_relative(*arguments: object) -> int:
    target = ctypes.cast(
        cast("ctypes.c_void_p", arguments[0]), ctypes.POINTER(ctypes.c_void_p)
    )
    target[0] = ctypes.c_void_p(23)
    return 0


def _fill_information(*arguments: object) -> int:
    kind = cast("int", arguments[1])
    target = cast("int", arguments[2])
    if kind == 18:
        ctypes.memmove(target, struct.pack("<Q", 7) + bytes(range(16)), 24)
    elif kind == 0:
        ctypes.memmove(target, struct.pack("<qqqqI", 0, 0, 9, 11, 0), 36)
    elif kind == 1:
        ctypes.memmove(target, struct.pack("<qqIB", 0, 13, 1, 0), 21)
    elif kind == 9:
        ctypes.memmove(target, struct.pack("<II", 0, 0), 8)
    return 1


def _final_path(*arguments: object) -> int:
    buffer = arguments[1]
    if buffer is None:
        return 6
    cast("ctypes.Array[ctypes.c_wchar]", buffer).value = "C:\\x"
    return 4


def test_windows_adapter_uses_rooted_nt_open_identity_and_no_replace_rename(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, ntdll = _adapter(monkeypatch)
    assert api.open_directory("C:\\parent", None) == 11
    assert api.open_child(11, "source.eml") == 23
    assert api.create_child(11, "stage.tmp") == 23
    info = api.info(23)
    assert (info.volume_serial, info.size, info.change_time) == (7, 13, 11)
    assert api.final_path(23) == "C:\\x"
    api.publish_no_replace(23, 11, "final.eml")
    api.publish_no_replace(23, 11, "")
    api.discard_private_stage(23)
    api.sync_directory(11)
    access = cast("int", kernel.CreateFileW.calls[0][1])
    assert access & FILE_WRITE_DATA
    assert ntdll.NtCreateFile.calls
    assert ntdll.NtSetInformationFile.calls
    rename_buffer = ctypes.cast(
        cast("int", ntdll.NtSetInformationFile.calls[0][2]),
        ctypes.POINTER(ctypes.c_ubyte * (MAX_PATH_BYTES + 24)),
    ).contents
    assert len(rename_buffer) == MAX_PATH_BYTES + 24


def test_windows_adapter_surfaces_no_replace_collision_and_native_failures(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, ntdll = _adapter(monkeypatch)
    ntdll.NtSetInformationFile.response = lambda *_arguments: -1
    ntdll.RtlNtStatusToDosError.response = lambda _status: 80
    with pytest.raises(FileExistsError):
        api.publish_no_replace(23, 11, "final.eml")
    kernel.CloseHandle.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="denied"):
        api.close(23)
    kernel.FlushFileBuffers.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="denied"):
        api.sync_directory(11)


def test_windows_adapter_exercises_handle_conversion_and_error_boundaries(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, ntdll = _adapter(monkeypatch)
    bridge = Msvcrt()
    monkeypatch.setitem(native_windows.__dict__, "_msvcrt", lambda: bridge)
    assert api.descriptor_from_handle(23, read_only=True) == 223
    assert api.descriptor_from_handle(23, read_only=False) == 223
    assert api.handle_from_descriptor(9) == 109
    with pytest.raises(OSError, match="captured"):
        api.open_directory("relative", None)
    kernel.GetFileType.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="not a disk"):
        api.info(23)
    kernel.GetFileType.response = lambda *_arguments: 1
    kernel.GetFinalPathNameByHandleW.response = lambda *_arguments: 0
    assert api.final_path(23) is None
    kernel.CreateFileW.response = lambda *_arguments: cast(
        "int", ctypes.c_void_p(-1).value
    )
    with pytest.raises(OSError, match="denied"):
        api.open_directory("C:\\missing", None)
    ntdll.NtCreateFile.response = lambda *_arguments: -1
    with pytest.raises(OSError, match="NtCreateFile"):
        api.open_child(11, "missing.eml", no_follow=True)


def test_windows_adapter_covers_dynamic_ctypes_and_native_null_handle(
    monkeypatch: MonkeyPatch,
) -> None:
    attribute = cast(
        "Callable[[str], object]", native_windows.__dict__["_ctypes_attribute"]
    )
    assert attribute("c_int") is ctypes.c_int
    monkeypatch.setattr(ctypes, "WinDLL", lambda *_arguments: object(), raising=False)
    monkeypatch.setattr(ctypes, "get_last_error", lambda: 19, raising=False)
    monkeypatch.setattr(ctypes, "FormatError", str, raising=False)
    loader_function = cast("Callable[[], object]", native_windows.__dict__["_win_dll"])
    error_function = cast("Callable[[], int]", native_windows.__dict__["_last_error"])
    formatter = cast("Callable[[int], str]", native_windows.__dict__["_format_error"])
    assert callable(loader_function())
    assert error_function() == 19
    assert formatter(19) == "19"
    bridge = Msvcrt()
    original_import = cast("Callable[..., object]", __import__)

    def import_fake(name: str, *arguments: object, **keywords: object) -> object:
        if name == "msvcrt":
            return bridge
        return original_import(name, *arguments, **keywords)

    monkeypatch.setattr("builtins.__import__", import_fake)
    loader = cast("Callable[[], object]", native_windows.__dict__["_msvcrt"])
    assert loader() is bridge
    api, _kernel, ntdll = _adapter(monkeypatch)
    ntdll.NtCreateFile.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="null handle"):
        api.open_child(11, "null.eml")


def test_windows_adapter_covers_normal_and_noncollision_error_edges(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, ntdll = _adapter(monkeypatch)
    api.close(23)
    assert api.open_directory("relative", 11) == 23
    relative_access = cast("int", ntdll.NtCreateFile.calls[-1][1])
    assert relative_access & FILE_WRITE_DATA
    ntdll.NtSetInformationFile.response = lambda *_arguments: -1
    ntdll.RtlNtStatusToDosError.response = lambda _status: 5
    monkeypatch.setitem(native_windows.__dict__, "_last_error", lambda: 5)
    with pytest.raises(OSError, match="publish"):
        api.publish_no_replace(23, 11, "final.eml")
    kernel.SetFileInformationByHandle.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="staging"):
        api.discard_private_stage(23)
    kernel.GetFileInformationByHandleEx.response = lambda *_arguments: 0
    with pytest.raises(OSError, match="identity"):
        api.info(23)


def test_windows_adapter_rejects_nonwindows_construction(
    monkeypatch: MonkeyPatch,
) -> None:
    monkeypatch.setattr(os, "name", "posix")
    with pytest.raises(OSError, match="unavailable"):
        WindowsApi()
