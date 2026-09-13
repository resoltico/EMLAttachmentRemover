"""Additional exact failure contracts for the Windows ctypes boundary."""

from __future__ import annotations

import ctypes
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import native_windows
from eml_attachment_remover.native_windows import WindowsApi
from eml_attachment_remover.native_windows_abi import (
    DELETE,
    FILE_CREATE,
    FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_NON_DIRECTORY_FILE,
    FILE_OPEN,
    FILE_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES,
    FILE_READ_DATA,
    FILE_SHARE_ALL,
    FILE_SYNCHRONOUS_IO_NONALERT,
    FILE_TRAVERSE,
    FILE_TYPE_DISK,
    FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA,
    INVALID_HANDLE_VALUE,
    OPEN_EXISTING,
    SYNCHRONIZE,
    IoStatusBlock,
    ObjectAttributes,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


type NativeResponse = Callable[..., int]


@dataclass
class NativeCall:
    """Record one ctypes invocation while allowing its configured signature."""

    response: NativeResponse
    argtypes: object | None = None
    restype: object | None = None
    calls: list[tuple[object, ...]] = field(default_factory=list)

    def __call__(self, *arguments: object) -> int:
        self.calls.append(arguments)
        return self.response(*arguments)


@dataclass
class Kernel:
    """Supply the kernel32 functions the adapter configures and calls."""

    CreateFileW: NativeCall
    CloseHandle: NativeCall
    GetFileInformationByHandleEx: NativeCall
    GetFileType: NativeCall
    GetFinalPathNameByHandleW: NativeCall
    SetFileInformationByHandle: NativeCall
    FlushFileBuffers: NativeCall


@dataclass
class Ntdll:
    """Supply the ntdll functions the adapter configures and calls."""

    NtCreateFile: NativeCall
    NtSetInformationFile: NativeCall
    RtlNtStatusToDosError: NativeCall


def _set_handle(*arguments: object) -> int:
    output = getattr(arguments[0], "_obj", None)
    assert isinstance(output, ctypes.c_void_p)
    output.value = 0xBEEF
    return 0


def _adapter(
    monkeypatch: MonkeyPatch,
) -> tuple[WindowsApi, Kernel, Ntdll, list[tuple[str, dict[str, object]]]]:
    kernel = Kernel(
        NativeCall(lambda *_arguments: 0xCAFE),
        NativeCall(lambda *_arguments: 1),
        NativeCall(lambda *_arguments: 1),
        NativeCall(lambda *_arguments: FILE_TYPE_DISK),
        NativeCall(lambda *_arguments: 0),
        NativeCall(lambda *_arguments: 1),
        NativeCall(lambda *_arguments: 1),
    )
    ntdll = Ntdll(
        NativeCall(_set_handle),
        NativeCall(lambda *_arguments: 0),
        NativeCall(lambda _status: 71),
    )
    libraries = iter((kernel, ntdll))
    loads: list[tuple[str, dict[str, object]]] = []

    def load(name: str, **keywords: object) -> object:
        loads.append((name, keywords))
        return next(libraries)

    monkeypatch.setattr(native_windows.__dict__["os"], "name", "nt")
    monkeypatch.setitem(native_windows.__dict__, "_win_dll", lambda: load)
    monkeypatch.setitem(native_windows.__dict__, "_last_error", lambda: 71)
    monkeypatch.setitem(
        native_windows.__dict__, "_format_error", lambda value: f"win-{value}"
    )
    return WindowsApi(), kernel, ntdll, loads


def test_constructor_loads_named_libraries_with_last_error_isolation(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, ntdll, loads = _adapter(monkeypatch)
    assert cast("object", api.kernel32) is kernel
    assert cast("object", api.ntdll) is ntdll
    assert loads == [
        ("kernel32", {"use_last_error": True}),
        ("ntdll", {"use_last_error": True}),
    ]


def test_rooted_createfile_treats_only_invalid_handle_as_failure(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll, _loads = _adapter(monkeypatch)
    kernel.CreateFileW.response = lambda *_arguments: 0
    assert api.open_directory("C:\\Inbox", None) == 0
    assert kernel.CreateFileW.calls == [
        (
            "C:\\Inbox",
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | FILE_WRITE_DATA | SYNCHRONIZE,
            FILE_SHARE_ALL,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    ]
    kernel.CreateFileW.response = lambda *_arguments: cast("int", INVALID_HANDLE_VALUE)
    with pytest.raises(OSError, match="could not open path parent") as captured:
        api.open_directory("C:\\missing", None)
    assert (captured.value.errno, captured.value.strerror) == (
        71,
        "could not open path parent: win-71",
    )


def test_relative_open_receipts_preserve_mode_specific_access_and_errors(
    monkeypatch: MonkeyPatch,
) -> None:
    api, _kernel, ntdll, _loads = _adapter(monkeypatch)
    receipts: list[tuple[int, bytes, int, int, int]] = []
    monkeypatch.setattr(
        native_windows.__dict__["ctypes"],
        "create_unicode_buffer",
        lambda value: ctypes.create_string_buffer(value.encode("utf-16-le") + b"\0\0"),
    )

    def receive_open(*arguments: object) -> int:
        attributes = getattr(arguments[2], "_obj", None)
        status = getattr(arguments[3], "_obj", None)
        assert isinstance(attributes, ObjectAttributes)
        assert isinstance(status, IoStatusBlock)
        object_name = attributes.ObjectName.contents
        receipts.append((
            cast("int", attributes.RootDirectory),
            ctypes.string_at(object_name.Buffer, object_name.Length),
            cast("int", arguments[1]),
            cast("int", arguments[7]),
            cast("int", arguments[8]),
        ))
        return _set_handle(*arguments)

    ntdll.NtCreateFile.response = receive_open
    assert api.open_child(0x10, "ordinary.eml") == 0xBEEF
    assert api.open_child(0x20, "link.eml", no_follow=True) == 0xBEEF
    assert api.create_child(0x30, "stage.eml") == 0xBEEF
    assert receipts == [
        (
            0x10,
            b"o\0r\0d\0i\0n\0a\0r\0y\0.\0e\0m\0l\0",
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_READ_DATA,
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_NON_DIRECTORY_FILE,
        ),
        (
            0x20,
            b"l\0i\0n\0k\0.\0e\0m\0l\0",
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_READ_DATA,
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT
            | FILE_NON_DIRECTORY_FILE
            | FILE_OPEN_REPARSE_POINT,
        ),
        (
            0x30,
            b"s\0t\0a\0g\0e\0.\0e\0m\0l\0",
            FILE_TRAVERSE
            | FILE_READ_ATTRIBUTES
            | SYNCHRONIZE
            | FILE_READ_DATA
            | FILE_WRITE_DATA
            | FILE_WRITE_ATTRIBUTES
            | DELETE,
            FILE_CREATE,
            FILE_SYNCHRONOUS_IO_NONALERT | FILE_NON_DIRECTORY_FILE,
        ),
    ]
    ntdll.NtCreateFile.response = lambda *_arguments: -0x23
    ntdll.RtlNtStatusToDosError.response = lambda status: 87 if status == -0x23 else 0
    with pytest.raises(OSError, match="NtCreateFile failed") as captured:
        api.open_child(0x10, "rejected.eml")
    assert (
        captured.value.errno,
        captured.value.strerror,
        captured.value.filename,
    ) == (87, "NtCreateFile failed with status -0x23", "rejected.eml")
    assert ntdll.RtlNtStatusToDosError.calls[-1] == (-0x23,)


def test_private_stage_discard_passes_exact_delete_record_and_native_error(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll, _loads = _adapter(monkeypatch)
    api.discard_private_stage(0xD00D)
    call = kernel.SetFileInformationByHandle.calls[-1]
    delete = getattr(call[2], "_obj", None)
    assert cast("ctypes.c_void_p", call[0]).value == 0xD00D
    assert call[1] == FILE_DISPOSITION_INFO
    assert isinstance(delete, ctypes.c_ubyte)
    assert (delete.value, call[3]) == (1, 1)
    kernel.SetFileInformationByHandle.response = lambda *_arguments: 0
    with pytest.raises(
        OSError, match="could not remove private staging file"
    ) as captured:
        api.discard_private_stage(0xD00D)
    assert (captured.value.errno, captured.value.strerror) == (
        71,
        "could not remove private staging file: win-71",
    )


def test_information_and_final_path_fail_closed_at_each_native_boundary(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll, _loads = _adapter(monkeypatch)
    kernel.GetFileType.response = lambda *_arguments: FILE_TYPE_DISK - 1
    with pytest.raises(OSError, match="not a disk") as non_disk:
        api.info(0x111)
    assert non_disk.value.errno is None
    assert non_disk.value.args == ("opened object is not a disk file",)
    assert kernel.GetFileInformationByHandleEx.calls == []
    kernel.GetFileType.response = lambda *_arguments: FILE_TYPE_DISK
    information_calls = 0

    def information(*_arguments: object) -> int:
        nonlocal information_calls
        information_calls += 1
        return int(information_calls < 2)

    kernel.GetFileInformationByHandleEx.response = information
    with pytest.raises(
        OSError, match="could not query handle identity"
    ) as information_error:
        api.info(0x111)
    assert (information_error.value.errno, information_error.value.strerror) == (
        71,
        "could not query handle identity: win-71",
    )
    assert information_calls == 2
    for required, observed in ((0, 0), (4, 0), (4, 4), (4, 5)):
        kernel.GetFinalPathNameByHandleW.calls.clear()

        kernel.GetFinalPathNameByHandleW.response = (
            lambda *arguments, probe=required, result=observed: (
                probe if arguments[1] is None else result
            )
        )
        assert api.final_path(0x222) is None
        if required == 0:
            assert len(kernel.GetFinalPathNameByHandleW.calls) == 1
        else:
            probe, read = kernel.GetFinalPathNameByHandleW.calls
            assert cast("ctypes.c_void_p", probe[0]).value == 0x222
            assert probe[1:] == (None, 0, 0)
            assert read[2:] == (required, 0)
