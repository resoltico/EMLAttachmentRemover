"""Exact mutation receipts for descriptor-rooted POSIX and Windows binding paths."""

from __future__ import annotations

import stat
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_posix, native_windows_binding
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
    """The metadata fields consumed by the POSIX descriptor implementation."""

    st_dev: int
    st_ino: int
    st_mode: int
    st_ctime_ns: int
    st_mtime_ns: int
    st_size: int


def _regular(*, size: int = 3) -> Metadata:
    return Metadata(7, 8, stat.S_IFREG | 0o600, 901, 902, size)


def _windows_info(*, directory: bool, reparse: bool) -> WindowsFileInfo:
    return WindowsFileInfo(
        volume_serial=7,
        file_id=10,
        change_time=901,
        last_write_time=902,
        size=3,
        directory=directory,
        reparse=reparse,
    )


@dataclass
class WindowsBackend:
    """Native binding trace with explicit parent and child ownership events."""

    child_error: OSError | None = None
    parent_error: OSError | None = None
    source_info: WindowsFileInfo = field(
        default_factory=lambda: _windows_info(directory=False, reparse=False)
    )
    parent_info: WindowsFileInfo = field(
        default_factory=lambda: WindowsFileInfo(
            volume_serial=7,
            file_id=9,
            change_time=809,
            last_write_time=810,
            size=0,
            directory=True,
            reparse=False,
        )
    )
    directories: list[tuple[str, int | None]] = field(default_factory=list)
    closed: list[int | str] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        self.directories.append((path, root))
        if self.parent_error is not None:
            raise self.parent_error
        return 9

    def open_child(self, _parent: int, _name: str) -> int:
        if self.child_error is not None:
            raise self.child_error
        return 10

    def info(self, handle: int) -> WindowsFileInfo:
        return self.parent_info if handle == 9 else self.source_info

    def close(self, handle: int | str) -> None:
        self.closed.append(handle)

    @staticmethod
    def handle_from_descriptor(_descriptor: int) -> int:
        return 10

    @staticmethod
    def final_path(_handle: int) -> str | None:
        return None


def _install_windows_backend(monkeypatch: MonkeyPatch, backend: WindowsBackend) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: backend)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)


def _bound(parent: str | None) -> BoundDestination:
    return BoundDestination(
        PathValue("request", "request", None, None),
        PathValue(parent, "parent", None, None),
        "out.eml",
        FileIdentity(7, 9, "directory", 809),
    )


def test_posix_inspection_and_read_use_exact_optional_open_flags_and_close_zero(
    monkeypatch: MonkeyPatch,
) -> None:
    openings: list[tuple[bytes, int, int | None]] = []
    closes: list[int] = []
    metadata = _regular()

    def parent(_path: str) -> tuple[int, str, bytes]:
        return 41, "parent", b"mail.eml"

    def open_file(name: bytes, flags: int, *, dir_fd: int | None = None) -> int:
        openings.append((name, flags, dir_fd))
        return 0

    nonblocking = 0x1000
    close_on_exec = 0x2000
    monkeypatch.setattr(native_posix, "_parent", parent)
    monkeypatch.setattr(native_posix.__dict__["os"], "O_RDONLY", 0x40)
    monkeypatch.setattr(
        native_posix.__dict__["os"], "O_NONBLOCK", nonblocking, raising=False
    )
    monkeypatch.setattr(
        native_posix.__dict__["os"], "O_CLOEXEC", close_on_exec, raising=False
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_file)
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: metadata)
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closes.append)

    assert native_posix._inspect_source_identity("mail.eml") == FileIdentity(  # ruff: ignore[private-member-access] - exact source-open flag receipt.
        7, 8, "-rw-------", 901
    )
    assert openings == [(b"mail.eml", 0x3040, 41)]
    assert closes == [0, 41]

    marker = object()
    monkeypatch.setattr(native_posix, "_snapshot", lambda *_arguments: marker)
    assert native_posix._read_source("request", "mail.eml") is marker  # ruff: ignore[private-member-access] - source-read uses the same native flags.
    assert openings == [(b"mail.eml", 0x3040, 41), (b"mail.eml", 0x3040, 41)]
    assert closes == [0, 41, 0, 41]


def test_posix_source_open_failures_close_only_the_live_parent_descriptor(
    monkeypatch: MonkeyPatch,
) -> None:
    closes: list[int] = []

    def parent(_path: str) -> tuple[int, str, bytes]:
        return 41, "parent", b"mail.eml"

    def open_failure(*_arguments: object, **_keywords: object) -> int:
        message = "open"
        raise OSError(message)

    monkeypatch.setattr(native_posix, "_parent", parent)
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_failure)
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closes.append)

    with pytest.raises(AppError) as inspect_error:
        native_posix._inspect_source_identity("mail.eml")  # ruff: ignore[private-member-access] - failed inspection has no child descriptor to release.
    assert inspect_error.value == AppError(
        ExitCode.INPUT_ERROR, "could not inspect source: open"
    )
    assert closes == [41]

    closes.clear()
    with pytest.raises(AppError) as read_error:
        native_posix._read_source("request", "mail.eml")  # ruff: ignore[private-member-access] - failed snapshot open has no child descriptor to release.
    assert read_error.value == AppError(
        ExitCode.INPUT_ERROR, "could not read source: open"
    )
    assert closes == [41]


def test_posix_snapshot_rejection_has_stable_source_kind_diagnostics(
    monkeypatch: MonkeyPatch,
) -> None:
    directory_metadata = Metadata(7, 8, stat.S_IFDIR | 0o700, 901, 902, 0)
    monkeypatch.setattr(
        native_posix.__dict__["os"], "fstat", lambda _fd: directory_metadata
    )

    with pytest.raises(AppError) as captured:
        native_posix._snapshot("r", "e", "p", b"mail.eml", 0)  # ruff: ignore[private-member-access] - nonregular snapshot rejection contract.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "selected source is not a regular file"
    )


def test_posix_nofollow_open_preserves_nonzero_native_flag_value(
    monkeypatch: MonkeyPatch,
) -> None:
    calls: list[tuple[bytes, int, int | None]] = []

    def open_file(name: bytes, flags: int, *, dir_fd: int | None = None) -> int:
        calls.append((name, flags, dir_fd))
        return 13

    monkeypatch.setattr(native_posix.__dict__["os"], "O_RDONLY", 0x40)
    monkeypatch.setattr(native_posix.__dict__["os"], "O_NOFOLLOW", 0x800, raising=False)
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_file)

    # ruff: ignore[private-member-access] - output reads must request O_NOFOLLOW exactly.
    assert (
        native_posix._open_child_nofollow(BoundDirectory(17, windows=False), b"out")
        == 13
    )
    assert calls == [(b"out", 0x840, 17)]


def test_posix_parent_open_failure_keeps_exact_input_error_context(
    monkeypatch: MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        native_posix,
        "_directory",
        lambda _expression: (_ for _ in ()).throw(OSError("parent")),
    )

    with pytest.raises(AppError) as captured:
        native_posix._parent("inbox/mail.eml")  # ruff: ignore[private-member-access] - parent descriptor open failure context.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not open path parent: parent"
    )
    assert isinstance(captured.value.__cause__, OSError)


def test_windows_binding_uses_exact_unc_reopen_roots_and_failure_context(
    monkeypatch: MonkeyPatch,
) -> None:
    backend = WindowsBackend()
    _install_windows_backend(monkeypatch, backend)

    # ruff: ignore[private-member-access] - backslash UNC roots independently of CWD.
    assert native_windows_binding._open_bound_destination(
        _bound("\\\\server\\share")
    ) == BoundDirectory(9, windows=True)
    # ruff: ignore[private-member-access] - forward-slash UNC roots independently of CWD.
    assert native_windows_binding._open_bound_destination(_bound("//server/share")) == (
        BoundDirectory(9, windows=True)
    )
    # ruff: ignore[private-member-access] - drive-rooted directory roots independently of CWD.
    assert native_windows_binding._open_bound_destination(_bound("D:/inbox")) == (
        BoundDirectory(9, windows=True)
    )
    assert backend.directories == [
        ("\\\\server\\share", None),
        ("//server/share", None),
        ("D:/inbox", None),
    ]

    backend.parent_error = OSError("reopen")
    with pytest.raises(AppError) as captured:
        native_windows_binding._open_bound_destination(_bound("relative"))  # ruff: ignore[private-member-access] - reopen errors retain caller-safe context.
    assert captured.value == AppError(
        ExitCode.WRITE_ERROR, "could not reopen destination parent: reopen"
    )
    assert isinstance(captured.value.__cause__, OSError)


def test_windows_binding_preserves_none_sentinel_and_snapshot_kind_message(
    monkeypatch: MonkeyPatch,
) -> None:
    backend = WindowsBackend(child_error=OSError("child"))
    _install_windows_backend(monkeypatch, backend)

    with pytest.raises(AppError) as inspect_error:
        native_windows_binding._inspect_source_identity("C:\\Inbox\\mail.eml")  # ruff: ignore[private-member-access] - an unopened child cannot be closed as an invented handle.
    assert inspect_error.value == AppError(
        ExitCode.INPUT_ERROR, "could not inspect source: child"
    )
    assert backend.closed == [9]

    backend.source_info = _windows_info(directory=True, reparse=False)
    with pytest.raises(AppError) as snapshot_error:
        native_windows_binding._snapshot("r", "e", "p", "mail", 0)  # ruff: ignore[private-member-access] - directory snapshots retain exact diagnostics.
    assert snapshot_error.value == AppError(
        ExitCode.INPUT_ERROR, "selected source is not a regular file"
    )
