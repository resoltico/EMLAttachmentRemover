"""Exact ABI receipts for the Windows-only ctypes adapter."""

from __future__ import annotations

import ctypes
from collections.abc import Callable
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import native_windows
from eml_attachment_remover.native_windows import WindowsApi, WindowsFileInfo
from eml_attachment_remover.native_windows_abi import (
    DELETE,
    ERROR_FILE_EXISTS,
    FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO,
    FILE_CREATE,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_ID_INFO,
    FILE_NON_DIRECTORY_FILE,
    FILE_OPEN,
    FILE_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES,
    FILE_READ_DATA,
    FILE_RENAME_INFORMATION_EX,
    FILE_SHARE_ALL,
    FILE_STANDARD_INFO,
    FILE_SYNCHRONOUS_IO_NONALERT,
    FILE_TRAVERSE,
    FILE_TYPE_DISK,
    FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA,
    MAX_RENAME_BUFFER_BYTES,
    OPEN_EXISTING,
    SYNCHRONIZE,
    FileAttributeTagInfo,
    FileBasicInfo,
    FileIdInfo,
    FileStandardInfo,
    IoStatusBlock,
    ObjectAttributes,
    UnicodeString,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


type NativeResponse = Callable[..., int]


@dataclass
class NativeCall:
    """A strict recording fake for one configured ctypes entry point."""

    response: NativeResponse
    argtypes: object | None = None
    restype: object | None = None
    calls: list[tuple[object, ...]] = field(default_factory=list)

    def __call__(self, *arguments: object) -> int:
        self.calls.append(arguments)
        return self.response(*arguments)


@dataclass
class Kernel:
    """The entire kernel32 entry-point surface required by the adapter."""

    CreateFileW: NativeCall
    CloseHandle: NativeCall
    GetFileInformationByHandleEx: NativeCall
    GetFileType: NativeCall
    GetFinalPathNameByHandleW: NativeCall
    SetFileInformationByHandle: NativeCall
    FlushFileBuffers: NativeCall


@dataclass
class Ntdll:
    """The complete ntdll entry-point surface required by the adapter."""

    NtCreateFile: NativeCall
    NtSetInformationFile: NativeCall
    RtlNtStatusToDosError: NativeCall


def _write_info(*arguments: object) -> int:
    kind = arguments[1]
    target = getattr(arguments[2], "_obj", None)
    assert type(kind) is int
    if kind == FILE_ID_INFO:
        assert isinstance(target, FileIdInfo)
        target.VolumeSerialNumber = 0xA0B0C0D0
        for index, value in enumerate(range(16)):
            target.FileId[index] = value
    elif kind == FILE_BASIC_INFO:
        assert isinstance(target, FileBasicInfo)
        target.LastWriteTime = 0x1020
        target.ChangeTime = 0x3040
    elif kind == FILE_STANDARD_INFO:
        assert isinstance(target, FileStandardInfo)
        target.EndOfFile = 0x5060
        target.Directory = 0
    elif kind == FILE_ATTRIBUTE_TAG_INFO:
        assert isinstance(target, FileAttributeTagInfo)
        target.FileAttributes = FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT
    else:
        pytest.fail(f"unexpected information kind {kind!r}")
    return 1


def _set_open_handle(*arguments: object) -> int:
    target = getattr(arguments[0], "_obj", None)
    assert isinstance(target, ctypes.c_void_p)
    target.value = 0xA1B2
    return 0


def _adapter(monkeypatch: MonkeyPatch) -> tuple[WindowsApi, Kernel, Ntdll]:
    kernel = Kernel(
        NativeCall(lambda *_arguments: 0x1010),
        NativeCall(lambda *_arguments: 1),
        NativeCall(_write_info),
        NativeCall(lambda *_arguments: FILE_TYPE_DISK),
        NativeCall(lambda *_arguments: 0),
        NativeCall(lambda *_arguments: 1),
        NativeCall(lambda *_arguments: 1),
    )
    ntdll = Ntdll(
        NativeCall(_set_open_handle),
        NativeCall(lambda *_arguments: 0),
        NativeCall(lambda _status: 5),
    )
    libraries = iter((kernel, ntdll))
    monkeypatch.setattr(native_windows.__dict__["os"], "name", "nt")
    monkeypatch.setitem(
        native_windows.__dict__,
        "_win_dll",
        lambda: lambda *_arguments, **_keywords: next(libraries),
    )
    monkeypatch.setitem(native_windows.__dict__, "_last_error", lambda: 999)
    monkeypatch.setitem(native_windows.__dict__, "_format_error", str)
    return WindowsApi(), kernel, ntdll


def test_configure_calls_declares_every_ctypes_signature(
    monkeypatch: MonkeyPatch,
) -> None:
    _api, kernel, ntdll = _adapter(monkeypatch)
    assert kernel.CreateFileW.argtypes == [
        ctypes.c_wchar_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_void_p,
    ]
    assert kernel.CreateFileW.restype is ctypes.c_void_p
    assert kernel.CloseHandle.argtypes == [ctypes.c_void_p]
    assert kernel.CloseHandle.restype is ctypes.c_int
    assert kernel.GetFileInformationByHandleEx.argtypes == [
        ctypes.c_void_p,
        ctypes.c_int,
        ctypes.c_void_p,
        ctypes.c_ulong,
    ]
    assert kernel.GetFileInformationByHandleEx.restype is ctypes.c_int
    assert kernel.GetFileType.argtypes == [ctypes.c_void_p]
    assert kernel.GetFileType.restype is ctypes.c_ulong
    assert kernel.GetFinalPathNameByHandleW.argtypes == [
        ctypes.c_void_p,
        ctypes.c_wchar_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
    ]
    assert kernel.GetFinalPathNameByHandleW.restype is ctypes.c_ulong
    assert kernel.SetFileInformationByHandle.argtypes == [
        ctypes.c_void_p,
        ctypes.c_int,
        ctypes.c_void_p,
        ctypes.c_ulong,
    ]
    assert kernel.SetFileInformationByHandle.restype is ctypes.c_int
    assert kernel.FlushFileBuffers.argtypes == [ctypes.c_void_p]
    assert kernel.FlushFileBuffers.restype is ctypes.c_int
    assert ntdll.NtCreateFile.argtypes == [
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.c_ulong,
        ctypes.POINTER(ObjectAttributes),
        ctypes.POINTER(IoStatusBlock),
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_ulong,
        ctypes.c_void_p,
        ctypes.c_ulong,
    ]
    assert ntdll.NtCreateFile.restype is ctypes.c_long
    assert ntdll.NtSetInformationFile.argtypes == [
        ctypes.c_void_p,
        ctypes.POINTER(IoStatusBlock),
        ctypes.c_void_p,
        ctypes.c_ulong,
        ctypes.c_ulong,
    ]
    assert ntdll.NtSetInformationFile.restype is ctypes.c_long
    assert ntdll.RtlNtStatusToDosError.argtypes == [ctypes.c_long]
    assert ntdll.RtlNtStatusToDosError.restype is ctypes.c_ulong


def test_rooted_nt_opens_encode_exact_unicode_object_and_create_receipts(
    monkeypatch: MonkeyPatch,
) -> None:
    api, _kernel, ntdll = _adapter(monkeypatch)
    encoded_names: list[bytes] = []

    def receive_open(*arguments: object) -> int:
        attributes = getattr(arguments[2], "_obj", None)
        assert isinstance(attributes, ObjectAttributes)
        string = attributes.ObjectName.contents
        encoded_names.append(ctypes.string_at(string.Buffer, string.Length))
        return _set_open_handle(*arguments)

    ntdll.NtCreateFile.response = receive_open
    monkeypatch.setattr(
        native_windows.__dict__["ctypes"],
        "create_unicode_buffer",
        lambda value: ctypes.create_string_buffer(value.encode("utf-16-le") + b"\0\0"),
    )
    assert api.open_directory("folder", 0x11) == 0xA1B2
    assert api.open_child(0x22, "child-🔐.eml", no_follow=True) == 0xA1B2
    assert api.create_child(0x33, "stage.tmp") == 0xA1B2
    expected = [
        (
            "folder",
            0x11,
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_WRITE_DATA,
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT,
        ),
        (
            "child-🔐.eml",
            0x22,
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE | FILE_READ_DATA,
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT
            | FILE_NON_DIRECTORY_FILE
            | FILE_OPEN_REPARSE_POINT,
        ),
        (
            "stage.tmp",
            0x33,
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
    assert len(ntdll.NtCreateFile.calls) == len(expected)
    for index, (call, (name, parent, access, disposition, options)) in enumerate(
        zip(ntdll.NtCreateFile.calls, expected, strict=True)
    ):
        output = getattr(call[0], "_obj", None)
        attributes = getattr(call[2], "_obj", None)
        status = getattr(call[3], "_obj", None)
        assert isinstance(output, ctypes.c_void_p)
        assert output.value == 0xA1B2
        assert isinstance(attributes, ObjectAttributes)
        assert isinstance(status, IoStatusBlock)
        assert attributes.Length == ctypes.sizeof(ObjectAttributes)
        assert attributes.RootDirectory == parent
        assert attributes.Attributes == 0
        assert attributes.SecurityDescriptor is None
        assert attributes.SecurityQualityOfService is None
        string = attributes.ObjectName.contents
        encoded = name.encode("utf-16-le")
        assert isinstance(string, UnicodeString)
        assert (string.Length, string.MaximumLength) == (len(encoded), len(encoded))
        assert encoded_names[index] == encoded
        assert (status.Status, status.Information) == (0, 0)
        assert call[1:] == (
            access,
            call[2],
            call[3],
            None,
            0,
            FILE_SHARE_ALL,
            disposition,
            options,
            None,
            0,
        )


def test_directory_create_uses_full_createfile_contract(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll = _adapter(monkeypatch)
    assert api.open_directory("C:\\Inbox", None) == 0x1010
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


def test_info_requires_each_native_record_and_preserves_derived_flags(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll = _adapter(monkeypatch)
    assert api.info(0x44) == WindowsFileInfo(
        volume_serial=0xA0B0C0D0,
        file_id=int.from_bytes(bytes(range(16)), "little"),
        change_time=0x3040,
        last_write_time=0x1020,
        size=0x5060,
        directory=True,
        reparse=True,
    )
    assert len(kernel.GetFileType.calls) == 1
    assert cast("ctypes.c_void_p", kernel.GetFileType.calls[0][0]).value == 0x44
    assert [call[1] for call in kernel.GetFileInformationByHandleEx.calls] == [
        FILE_ID_INFO,
        FILE_BASIC_INFO,
        FILE_STANDARD_INFO,
        FILE_ATTRIBUTE_TAG_INFO,
    ]
    for call in kernel.GetFileInformationByHandleEx.calls:
        target = getattr(call[2], "_obj", None)
        assert isinstance(
            target, (FileIdInfo, FileBasicInfo, FileStandardInfo, FileAttributeTagInfo)
        )
        assert cast("ctypes.c_void_p", call[0]).value == 0x44
        assert call[3] == ctypes.sizeof(target)


def test_final_path_uses_required_capacity_and_rejects_truncated_results(
    monkeypatch: MonkeyPatch,
) -> None:
    api, kernel, _ntdll = _adapter(monkeypatch)

    def normal_path(*arguments: object) -> int:
        buffer = arguments[1]
        if buffer is None:
            return 5
        assert isinstance(buffer, ctypes.Array)
        assert len(buffer) == 5
        cast("ctypes.Array[ctypes.c_wchar]", buffer).value = "C:\\x"
        return 4

    kernel.GetFinalPathNameByHandleW.response = normal_path
    assert api.final_path(0x55) == "C:\\x"
    probe, read = kernel.GetFinalPathNameByHandleW.calls
    assert cast("ctypes.c_void_p", probe[0]).value == 0x55
    assert probe[1:] == (None, 0, 0)
    assert cast("ctypes.c_void_p", read[0]).value == 0x55
    assert isinstance(read[1], ctypes.Array)
    assert read[2:] == (5, 0)
    kernel.GetFinalPathNameByHandleW.calls.clear()
    kernel.GetFinalPathNameByHandleW.response = lambda *_arguments: 0
    assert api.final_path(0x55) is None
    kernel.GetFinalPathNameByHandleW.response = lambda *_arguments: 5
    assert api.final_path(0x55) is None


def test_publish_uses_fixed_rename_buffer_and_translated_native_errors(
    monkeypatch: MonkeyPatch,
) -> None:
    api, _kernel, ntdll = _adapter(monkeypatch)
    name = "final-🔐.eml"
    encoded = name.encode("utf-16-le")
    api.publish_no_replace(0x66, 0x77, name)
    api.publish_no_replace(0x66, 0x77, "x")
    first, second = ntdll.NtSetInformationFile.calls
    assert cast("ctypes.c_void_p", first[0]).value == 0x66
    status = getattr(first[1], "_obj", None)
    buffer = getattr(first[2], "_obj", None)
    assert isinstance(status, IoStatusBlock)
    assert isinstance(buffer, ctypes.Array)
    assert (status.Status, status.Information) == (0, 0)
    assert len(buffer) == MAX_RENAME_BUFFER_BYTES + 1
    contents = bytes(buffer)
    assert int.from_bytes(contents[0:4], "little") == 0
    assert int.from_bytes(contents[8:16], "little") == 0x77
    assert int.from_bytes(contents[16:20], "little") == len(encoded)
    assert contents[20 : 20 + len(encoded)] == encoded
    assert contents[20 + len(encoded) : MAX_RENAME_BUFFER_BYTES] == bytes(
        MAX_RENAME_BUFFER_BYTES - 20 - len(encoded)
    )
    assert contents[MAX_RENAME_BUFFER_BYTES:] == b"\0"
    assert first[3:] == (20 + len(encoded), FILE_RENAME_INFORMATION_EX)
    assert second[3:] == (24, FILE_RENAME_INFORMATION_EX)

    ntdll.NtSetInformationFile.response = lambda *_arguments: -0x40000001
    ntdll.RtlNtStatusToDosError.response = lambda _status: ERROR_FILE_EXISTS
    with pytest.raises(FileExistsError) as collision:
        api.publish_no_replace(0x66, 0x77, name)
    assert (
        collision.value.errno,
        collision.value.strerror,
        collision.value.filename,
    ) == (
        ERROR_FILE_EXISTS,
        "destination already exists",
        name,
    )

    ntdll.RtlNtStatusToDosError.response = lambda _status: 5
    monkeypatch.setitem(
        native_windows.__dict__, "_format_error", lambda value: f"win-{value}"
    )
    with pytest.raises(OSError, match="could not publish candidate: win-5") as failure:
        api.publish_no_replace(0x66, 0x77, name)
    assert failure.value.errno == 5
    assert failure.value.strerror == "could not publish candidate: win-5"
