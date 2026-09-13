"""Portability and root-binding receipts for exact native mutation targets."""

from __future__ import annotations

import stat
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_posix, native_values, native_windows_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_windows import WindowsFileInfo

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


@dataclass(frozen=True)
class Metadata:
    """POSIX metadata values read by an opened descriptor."""

    st_dev: int = 7
    st_ino: int = 8
    st_mode: int = stat.S_IFREG | 0o600
    st_ctime_ns: int = 901
    st_mtime_ns: int = 902
    st_size: int = 0


@dataclass
class BindingApi:
    """A strict record of parent roots supplied to the Windows native API."""

    fail_parent: bool = False
    directories: list[tuple[str, int | None]] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        self.directories.append((path, root))
        if self.fail_parent:
            message = "parent"
            raise OSError(message)
        return 11

    @staticmethod
    def info(_handle: int) -> WindowsFileInfo:
        return WindowsFileInfo(
            volume_serial=7,
            file_id=11,
            change_time=809,
            last_write_time=810,
            size=0,
            directory=True,
            reparse=False,
        )


def _install_binding(monkeypatch: MonkeyPatch, api: BindingApi) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)


def _destination(parent: str) -> BoundDestination:
    return BoundDestination(
        PathValue("request", "request", None, None),
        PathValue(parent, "parent", None, None),
        "out.eml",
        FileIdentity(7, 11, "directory", 809),
    )


def test_posix_source_inspection_uses_zero_when_optional_open_flags_are_missing(
    monkeypatch: MonkeyPatch,
) -> None:
    openings: list[tuple[bytes, int, int | None]] = []
    closes: list[int] = []

    def parent(_path: str) -> tuple[int, str, bytes]:
        return 41, "parent", b"mail.eml"

    def open_file(name: bytes, flags: int, *, dir_fd: int | None = None) -> int:
        openings.append((name, flags, dir_fd))
        return 6

    monkeypatch.setattr(native_posix, "_parent", parent)
    monkeypatch.setattr(native_posix.__dict__["os"], "O_RDONLY", 0x40)
    monkeypatch.delattr(native_posix.__dict__["os"], "O_NONBLOCK", raising=False)
    monkeypatch.delattr(native_posix.__dict__["os"], "O_CLOEXEC", raising=False)
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_file)
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: Metadata())
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closes.append)

    assert native_posix._inspect_source_identity("mail.eml") == FileIdentity(  # ruff: ignore[private-member-access] - portable optional flags default to zero.
        7, 8, "-rw-------", 901
    )
    assert openings == [(b"mail.eml", 0x40, 41)]
    assert closes == [6, 41]


def test_posix_source_read_and_nofollow_open_default_missing_flags_to_zero(
    monkeypatch: MonkeyPatch,
) -> None:
    openings: list[tuple[bytes, int, int | None]] = []
    closes: list[int] = []
    marker = object()

    def parent(_path: str) -> tuple[int, str, bytes]:
        return 41, "parent", b"mail.eml"

    def open_file(name: bytes, flags: int, *, dir_fd: int | None = None) -> int:
        openings.append((name, flags, dir_fd))
        return 6

    monkeypatch.setattr(native_posix, "_parent", parent)
    monkeypatch.setattr(native_posix.__dict__["os"], "O_RDONLY", 0x40)
    monkeypatch.delattr(native_posix.__dict__["os"], "O_NONBLOCK", raising=False)
    monkeypatch.delattr(native_posix.__dict__["os"], "O_CLOEXEC", raising=False)
    monkeypatch.delattr(native_posix.__dict__["os"], "O_NOFOLLOW", raising=False)
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_file)
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closes.append)
    monkeypatch.setattr(native_posix, "_snapshot", lambda *_arguments: marker)

    assert native_posix._read_source("request", "mail.eml") is marker  # ruff: ignore[private-member-access] - source read defaults each optional flag.
    # ruff: ignore[private-member-access] - no-follow default is portable zero.
    assert (
        native_posix._open_child_nofollow(BoundDirectory(19, windows=False), b"out")
        == 6
    )
    assert openings == [(b"mail.eml", 0x40, 41), (b"out", 0x40, 19)]
    assert closes == [6, 41]


def test_native_values_keep_platform_defaults_and_precise_basename_diagnostics(
    monkeypatch: MonkeyPatch,
) -> None:
    with pytest.raises(AppError) as captured:
        native_values._validate_posix("nested/")  # ruff: ignore[private-member-access] - trailing separator has no ordinary entry name.
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "path has an empty basename"
    )

    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:\\mail\\message.eml") == (
            "C:\\mail\\message.mime-pruned.eml"
        )
        assert native_values.default_destination("C:\\mail\\message.txt") == (
            "C:\\mail\\message.txt.mime-pruned.eml"
        )


def test_windows_binding_treats_drive_relative_parent_as_cwd_rooted_not_absolute(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi()
    _install_binding(monkeypatch, api)

    assert native_windows_binding._parent("C:inbox\\mail.eml") == (  # ruff: ignore[private-member-access] - drive-relative parent remains CWD rooted.
        11,
        "C:inbox",
        "mail.eml",
    )
    assert native_windows_binding._open_bound_destination(  # ruff: ignore[private-member-access] - rebinding preserves drive-relative semantics.
        _destination("C:inbox")
    ) == BoundDirectory(11, windows=True)
    assert api.directories == [("C:inbox", 77), ("C:inbox", 77)]


def test_windows_binding_parent_and_missing_address_fail_with_exact_context(
    monkeypatch: MonkeyPatch,
) -> None:
    api = BindingApi(fail_parent=True)
    _install_binding(monkeypatch, api)

    with pytest.raises(AppError) as parent_error:
        native_windows_binding._parent("inbox\\mail.eml")  # ruff: ignore[private-member-access] - parent-open failure preserves input context.
    assert parent_error.value == AppError(
        ExitCode.INPUT_ERROR, "could not open path parent: parent"
    )
    assert isinstance(parent_error.value.__cause__, OSError)

    missing = BoundDestination(
        PathValue("request", "request", None, None),
        PathValue(None, "none", None, None),
        "out.eml",
        FileIdentity(7, 11, "directory", 809),
    )
    with pytest.raises(AppError) as missing_error:
        native_windows_binding._open_bound_destination(missing)  # ruff: ignore[private-member-access] - destination binding requires a native parent address.
    assert missing_error.value == AppError(
        ExitCode.WRITE_ERROR, "destination parent has no native address"
    )
