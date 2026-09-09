"""Exact behavioral receipts for selected native Windows mutation targets."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Protocol

import pytest

from eml_attachment_remover import native_windows, native_windows_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_values import path_value
from eml_attachment_remover.native_windows import WindowsApi, WindowsFileInfo
from eml_attachment_remover.native_windows_abi import (
    O_BINARY,
    O_READ_ONLY,
    O_READ_WRITE,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


class Pointer(Protocol):
    """Expose the nullable integral payload of a ctypes void pointer."""

    value: int | None


def _directory() -> WindowsFileInfo:
    return WindowsFileInfo(
        volume_serial=7,
        file_id=101,
        change_time=809,
        last_write_time=810,
        size=0,
        directory=True,
        reparse=False,
    )


def _regular(*, size: int = 3) -> WindowsFileInfo:
    return WindowsFileInfo(
        volume_serial=7,
        file_id=0,
        change_time=901,
        last_write_time=902,
        size=size,
        directory=False,
        reparse=False,
    )


@dataclass
class BindingApi:
    """A traceable Windows binding adapter with deliberately valid zero handles."""

    directory_info: WindowsFileInfo = field(default_factory=_directory)
    child_info: WindowsFileInfo = field(default_factory=_regular)
    child_result: int | OSError = 0
    directories: list[tuple[str, int | None]] = field(default_factory=list)
    children: list[tuple[int, str, bool]] = field(default_factory=list)
    closed: list[int] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        self.directories.append((path, root))
        return 101

    def open_child(self, parent: int, name: str, *, no_follow: bool = False) -> int:
        self.children.append((parent, name, no_follow))
        if isinstance(self.child_result, OSError):
            raise self.child_result
        return self.child_result

    def info(self, handle: int) -> WindowsFileInfo:
        return self.directory_info if handle == 101 else self.child_info

    def close(self, handle: int) -> None:
        self.closed.append(handle)

    @staticmethod
    def handle_from_descriptor(_descriptor: int) -> int:
        return 0

    @staticmethod
    def final_path(_handle: int) -> str | None:
        return None

    @staticmethod
    def descriptor_from_handle(_handle: int, *, read_only: bool) -> int:
        assert read_only is True
        return 0


def _install_binding(monkeypatch: MonkeyPatch, api: BindingApi) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)


def _bound(parent: str | None) -> BoundDestination:
    return BoundDestination(
        request=path_value("request"),
        parent=PathValue(parent, "parent", None, None),
        basename="out.eml",
        directory_identity=FileIdentity(7, 101, "directory", 809),
    )


def test_windows_binding_distinguishes_unc_slashes_and_drive_relative_parent_roots(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi()
    _install_binding(monkeypatch, api)

    assert native_windows_binding._parent("//server/share/mail.eml") == (  # ruff: ignore[private-member-access] - forward-slash UNC must bypass the captured root.
        101,
        "//server/share/",
        "mail.eml",
    )
    assert native_windows_binding._parent("D:inbox\\mail.eml") == (  # ruff: ignore[private-member-access] - drive-relative paths remain rooted at the captured CWD.
        101,
        "D:inbox",
        "mail.eml",
    )
    assert api.directories == [("//server/share/", None), ("D:inbox", 77)]


def test_windows_binding_preserves_exact_rejection_messages_and_zero_handle_cleanup(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi()
    _install_binding(monkeypatch, api)

    api.directory_info = WindowsFileInfo(
        volume_serial=7,
        file_id=101,
        change_time=809,
        last_write_time=810,
        size=0,
        directory=False,
        reparse=False,
    )
    with pytest.raises(AppError) as bound_error:
        native_windows_binding._bind_destination("request", "C:\\Inbox\\out.eml")  # ruff: ignore[private-member-access] - destination directory-kind receipt.
    assert bound_error.value == AppError(
        ExitCode.WRITE_ERROR, "destination parent is not a directory"
    )
    assert api.closed == [101]

    api.directory_info = _directory()
    api.child_info = WindowsFileInfo(
        volume_serial=7,
        file_id=0,
        change_time=901,
        last_write_time=902,
        size=3,
        directory=True,
        reparse=False,
    )
    with pytest.raises(AppError) as source_error:
        native_windows_binding._inspect_source_identity("C:\\Inbox\\mail.eml")  # ruff: ignore[private-member-access] - source-kind diagnostic receipt.
    assert source_error.value == AppError(
        ExitCode.INPUT_ERROR, "selected source is not a regular file"
    )
    assert api.closed == [101, 0, 101]

    api.child_info = WindowsFileInfo(
        volume_serial=7,
        file_id=0,
        change_time=901,
        last_write_time=902,
        size=3,
        directory=False,
        reparse=True,
    )
    with pytest.raises(AppError) as child_error:
        native_windows_binding._child_lstat(BoundDirectory(101, windows=True), "out")  # ruff: ignore[private-member-access] - reparse output is rejected under no-follow.
    assert child_error.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination is not a regular file"
    )
    assert api.closed == [101, 0, 101, 0]


def test_windows_binding_open_failure_does_not_close_unacquired_sentinel_handles(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi(child_result=FileNotFoundError("gone"))
    _install_binding(monkeypatch, api)

    assert (
        native_windows_binding._child_lstat(  # ruff: ignore[private-member-access] - raced absence must not close the sentinel handle.
            BoundDirectory(101, windows=True), "out"
        )
        is None
    )
    assert api.closed == []

    api.child_result = OSError("open")
    descriptor_closes: list[int] = []
    monkeypatch.setattr(
        native_windows_binding.__dict__["os"], "close", descriptor_closes.append
    )
    with pytest.raises(AppError) as read_error:
        native_windows_binding._read_source("request", "C:\\Inbox\\mail.eml")  # ruff: ignore[private-member-access] - failed child open owns only its parent handle.
    assert read_error.value == AppError(
        ExitCode.INPUT_ERROR, "could not read source: open"
    )
    assert descriptor_closes == []
    assert api.closed == [101]


def test_windows_binding_retains_zero_native_handles_and_accepts_the_raw_size_limit(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi()
    _install_binding(monkeypatch, api)

    assert native_windows_binding._inspect_source_identity("C:\\Inbox\\mail.eml") == (  # ruff: ignore[private-member-access] - zero is a valid acquired fixture handle.
        FileIdentity(7, 0, "regular", 901)
    )
    assert api.closed == [0, 101]

    original_read_all = native_windows_binding._read_all  # ruff: ignore[private-member-access] - restore the real bounded-read implementation after the snapshot seam.
    monkeypatch.setattr(native_windows_binding, "MAX_RAW_BYTES", 3)
    monkeypatch.setattr(native_windows_binding, "_read_all", lambda _fd: b"abc")
    assert native_windows_binding._snapshot("r", "e", "p", "n", 0).size == 3  # ruff: ignore[private-member-access] - inclusive raw-size ceiling receipt.

    requests: list[int] = []
    monkeypatch.setattr(native_windows_binding, "_read_all", original_read_all)
    monkeypatch.setattr(native_windows_binding, "MAX_RAW_BYTES", 2 * 1024 * 1024)

    def empty_read(_fd: int, size: int) -> bytes:
        requests.append(size)
        return b""

    monkeypatch.setattr(native_windows_binding.__dict__["os"], "read", empty_read)
    assert native_windows_binding._read_all(0) == b""  # ruff: ignore[private-member-access] - native read chunk is exactly one MiB.
    assert requests == [1024 * 1024]


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("\\\\server\\share", True),
        ("//server/share", True),
        ("C:\\Inbox", True),
        ("C:/Inbox", True),
        ("C:inbox", False),
    ],
)
def test_windows_absolute_classification_accepts_only_rooted_windows_forms(
    value: str, expected: object
) -> None:
    assert type(expected) is bool
    assert native_windows._is_absolute(value) is expected  # ruff: ignore[private-member-access] - native absolute-path classifier.


def test_windows_native_adapter_keeps_handle_value_and_binary_descriptor_flags(
    monkeypatch: MonkeyPatch,
) -> None:
    close_values: list[int | None] = []
    descriptor_calls: list[tuple[int, int]] = []

    class Kernel:
        @staticmethod
        # ruff: ignore[invalid-function-name] - mirrors the native entry point.
        def CloseHandle(value: Pointer) -> int:
            close_values.append(value.value)
            return 1

    class CRunTime:
        @staticmethod
        def open_osfhandle(handle: int, flags: int) -> int:
            descriptor_calls.append((handle, flags))
            return 31

    api = object.__new__(WindowsApi)
    api.__dict__["kernel32"] = Kernel()
    monkeypatch.setattr(native_windows, "_msvcrt", CRunTime)

    api.close(0x1234)
    assert close_values == [0x1234]
    assert WindowsApi.descriptor_from_handle(11, read_only=True) == 31
    assert WindowsApi.descriptor_from_handle(12, read_only=False) == 31
    assert descriptor_calls == [
        (11, O_BINARY | O_READ_ONLY),
        (12, O_BINARY | O_READ_WRITE),
    ]


def test_windows_native_relative_open_keeps_boolean_intent_and_null_handle_diagnostic(
    monkeypatch: MonkeyPatch,
) -> None:
    calls: list[tuple[int, str, bool, bool, bool]] = []
    original_create_relative = WindowsApi._create_relative  # ruff: ignore[private-member-access] - retain the production null-handle branch after dispatch spying.

    def create_relative(
        _self: WindowsApi,
        parent: int,
        name: str,
        *,
        directory: bool,
        no_follow: bool,
        create: bool,
    ) -> int:
        calls.append((parent, name, directory, no_follow, create))
        return 42

    @dataclass
    class Ntdll:
        calls: list[tuple[object, ...]] = field(default_factory=list)

        # ruff: ignore[invalid-function-name] - mirrors the native entry point.
        def NtCreateFile(self, *arguments: object) -> int:
            self.calls.append(arguments)
            return 0

    api = object.__new__(WindowsApi)
    monkeypatch.setattr(WindowsApi, "_create_relative", create_relative)
    assert api.open_directory("relative", 99) == 42
    assert calls == [(99, "relative", True, False, False)]
    with pytest.raises(OSError, match=r"^relative directory requires the captured"):
        api.open_directory("relative", None)

    api.__dict__["ntdll"] = Ntdll()
    with pytest.raises(OSError, match=r"^NtCreateFile returned a null handle$"):
        original_create_relative(
            api, 99, "name", directory=False, no_follow=False, create=False
        )
