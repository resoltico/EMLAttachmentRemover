"""Failure-ownership receipts for the Windows handle binding layer."""
# ruff: file-ignore[docstring-missing-returns]

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, NoReturn

import pytest

from eml_attachment_remover import native_windows_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    SourceSnapshot,
)
from eml_attachment_remover.native_values import path_value
from eml_attachment_remover.native_windows import WindowsFileInfo

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


def _directory(
    *, device: int = 7, inode: int = 101, changed: int = 809
) -> WindowsFileInfo:
    """Construct one ordinary directory metadata receipt."""
    return WindowsFileInfo(
        volume_serial=device,
        file_id=inode,
        change_time=changed,
        last_write_time=810,
        size=0,
        directory=True,
        reparse=False,
    )


def _regular(
    *, device: int = 7, inode: int = 202, changed: int = 901, size: int = 7
) -> WindowsFileInfo:
    """Construct one ordinary regular-file metadata receipt."""
    return WindowsFileInfo(
        volume_serial=device,
        file_id=inode,
        change_time=changed,
        last_write_time=902,
        size=size,
        directory=False,
        reparse=False,
    )


@dataclass
class TraceApi:
    """A deterministic native adapter that preserves resource-operation receipts."""

    information: dict[int, list[WindowsFileInfo | OSError]] = field(
        default_factory=lambda: {101: [_directory()], 202: [_regular()]}
    )
    child_result: int | OSError = 202
    descriptor_result: int | OSError = 41
    final: str | None = "\\\\?\\C:\\Inbox\\source.eml"
    directories: list[tuple[str, int | None]] = field(default_factory=list)
    children: list[tuple[int, str, bool]] = field(default_factory=list)
    closed: list[int] = field(default_factory=list)
    final_handles: list[int] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        """Record and return the stable parent handle."""
        self.directories.append((path, root))
        return 101

    def open_child(self, parent: int, name: str, *, no_follow: bool = False) -> int:
        """Record a child-open request or raise its configured OS failure."""
        self.children.append((parent, name, no_follow))
        if isinstance(self.child_result, OSError):
            raise self.child_result
        return self.child_result

    def info(self, handle: int) -> WindowsFileInfo:
        """Return consecutive immutable native metadata observations."""
        values = self.information[handle]
        value = values.pop(0) if len(values) > 1 else values[0]
        if isinstance(value, OSError):
            raise value
        return value

    def close(self, handle: int) -> None:
        """Record native-handle ownership release."""
        self.closed.append(handle)

    def descriptor_from_handle(self, _handle: int, *, read_only: bool) -> int:
        """Transfer a configured descriptor or raise its configured OS failure."""
        assert read_only is True
        if isinstance(self.descriptor_result, OSError):
            raise self.descriptor_result
        return self.descriptor_result

    @staticmethod
    def handle_from_descriptor(_descriptor: int) -> int:
        """Map every fixture descriptor back to the source handle."""
        return 202

    def final_path(self, handle: int) -> str | None:
        """Record the final-address query and return its configured result."""
        self.final_handles.append(handle)
        return self.final


def _install(monkeypatch: MonkeyPatch, api: TraceApi) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)


def _destination(parent: str, identity: FileIdentity) -> BoundDestination:
    return BoundDestination(
        path_value("request"), path_value(parent), "out.eml", identity
    )


def test_source_inspection_wraps_metadata_failure_and_closes_each_open_handle(
    monkeypatch: MonkeyPatch,
) -> None:
    api = TraceApi(information={101: [_directory()], 202: [OSError("metadata")]})
    _install(monkeypatch, api)

    with pytest.raises(AppError) as captured:
        native_windows_binding._inspect_source_identity("C:\\Inbox\\source.eml")  # ruff: ignore[private-member-access] - metadata failure ownership boundary.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not inspect source: metadata"
    )
    assert isinstance(captured.value.__cause__, OSError)
    assert api.children == [(101, "source.eml", False)]
    assert api.closed == [202, 101]


def test_source_read_accepts_descriptor_zero_and_retires_it_after_snapshot_os_failure(
    monkeypatch: MonkeyPatch,
) -> None:
    api = TraceApi(descriptor_result=0)
    descriptor_closures: list[int] = []
    cause = OSError("snapshot")

    def snapshot_failure(*_arguments: object) -> NoReturn:
        raise cause

    _install(monkeypatch, api)
    monkeypatch.setattr(native_windows_binding, "_snapshot", snapshot_failure)
    monkeypatch.setattr(
        native_windows_binding.__dict__["os"], "close", descriptor_closures.append
    )

    with pytest.raises(AppError) as captured:
        native_windows_binding._read_source("request", "C:\\Inbox\\source.eml")  # ruff: ignore[private-member-access] - descriptor-transfer cleanup boundary.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not read source: snapshot"
    )
    assert captured.value.__cause__ is cause
    assert descriptor_closures == [0]
    assert api.closed == [101]


def test_snapshot_rejects_changed_metadata_before_resolving_final_address(
    monkeypatch: MonkeyPatch,
) -> None:
    api = TraceApi(
        information={101: [_directory()], 202: [_regular(), _regular(changed=902)]}
    )
    _install(monkeypatch, api)
    monkeypatch.setattr(native_windows_binding, "_read_all", lambda _fd: b"payload")

    with pytest.raises(AppError) as captured:
        native_windows_binding._snapshot("r", "e", "p", "n", 41)  # ruff: ignore[private-member-access] - immutable descriptor metadata receipt.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "source changed while it was being read"
    )
    assert api.final_handles == []


def test_snapshot_preserves_absent_final_address_and_complete_content_evidence(
    monkeypatch: MonkeyPatch,
) -> None:
    api = TraceApi(final=None)
    _install(monkeypatch, api)
    monkeypatch.setattr(native_windows_binding, "_read_all", lambda _fd: b"payload")

    snapshot = native_windows_binding._snapshot("r", "e", "p", "n", 41)  # ruff: ignore[private-member-access] - complete accepted Windows snapshot.

    assert snapshot == SourceSnapshot(
        path_value("r"),
        path_value("e"),
        path_value("p"),
        "n",
        None,
        FileIdentity(7, 202, "regular", 901),
        0o600,
        b"payload",
        hashlib.sha256(b"payload").hexdigest(),
        7,
    )
    assert api.final_handles == [202]


def test_reopening_relative_destination_accepts_ctime_only_change_but_closes_type_swap(
    monkeypatch: MonkeyPatch,
) -> None:
    expected = FileIdentity(7, 101, "directory", 809)
    api = TraceApi(information={101: [_directory(changed=999)], 202: [_regular()]})
    _install(monkeypatch, api)

    opened = native_windows_binding._open_bound_destination(  # ruff: ignore[private-member-access] - directory rebinding identity boundary.
        _destination("relative", expected)
    )

    assert opened == BoundDirectory(101, windows=True)
    assert api.directories == [("relative", 77)]
    assert api.closed == []

    api.information[101] = [
        WindowsFileInfo(
            volume_serial=7,
            file_id=101,
            change_time=999,
            last_write_time=810,
            size=0,
            directory=False,
            reparse=False,
        )
    ]
    with pytest.raises(AppError) as captured:
        # ruff: ignore[private-member-access] - type replacement must invalidate a bound parent.
        native_windows_binding._open_bound_destination(
            _destination("relative", expected)
        )

    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination parent changed after binding"
    )
    assert api.closed == [101]


def test_child_lstat_closes_on_metadata_not_found_and_nofollow_descriptor_fault(
    monkeypatch: MonkeyPatch,
) -> None:
    api = TraceApi(information={101: [_directory()], 202: [FileNotFoundError()]})
    _install(monkeypatch, api)
    directory = BoundDirectory(101, windows=True)

    assert native_windows_binding._child_lstat(directory, "out.eml") is None  # ruff: ignore[private-member-access] - raced final-entry absence is nonfatal.
    assert api.children == [(101, "out.eml", True)]
    assert api.closed == [202]

    descriptor_failure = OSError("descriptor")
    api.information[202] = [_regular()]
    api.descriptor_result = descriptor_failure
    with pytest.raises(OSError, match=r"^descriptor$") as captured:
        native_windows_binding._open_child_nofollow(directory, "out.eml")  # ruff: ignore[private-member-access] - descriptor conversion retains native-handle ownership.

    assert captured.value is descriptor_failure
    assert api.closed == [202, 202]
