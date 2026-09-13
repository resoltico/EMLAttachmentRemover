"""Exact handle-receipt contracts for the Windows binding layer."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field

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


@dataclass
class ReceiptApi:
    """A strict in-memory record of every native adapter request."""

    directory_info: WindowsFileInfo = field(
        default_factory=lambda: WindowsFileInfo(
            volume_serial=7,
            file_id=101,
            change_time=809,
            last_write_time=810,
            size=0,
            directory=True,
            reparse=False,
        )
    )
    source_info: WindowsFileInfo = field(
        default_factory=lambda: WindowsFileInfo(
            volume_serial=7,
            file_id=202,
            change_time=901,
            last_write_time=902,
            size=7,
            directory=False,
            reparse=False,
        )
    )
    stage_info: WindowsFileInfo = field(
        default_factory=lambda: WindowsFileInfo(
            volume_serial=7,
            file_id=303,
            change_time=903,
            last_write_time=904,
            size=0,
            directory=False,
            reparse=False,
        )
    )
    final: str | None = "\\\\?\\C:\\Inbox\\source.eml"
    publish_error: OSError | None = None
    open_directories: list[tuple[str, int | None]] = field(default_factory=list)
    children: list[tuple[int, str, bool]] = field(default_factory=list)
    descriptors: list[tuple[int, bool]] = field(default_factory=list)
    descriptor_handles: list[int] = field(default_factory=list)
    created: list[tuple[int, str]] = field(default_factory=list)
    published: list[tuple[int, int, str]] = field(default_factory=list)
    discarded: list[int] = field(default_factory=list)
    closed: list[int] = field(default_factory=list)
    synced: list[int] = field(default_factory=list)

    def open_directory(self, path: str, root: int | None) -> int:
        self.open_directories.append((path, root))
        return 101

    def open_child(self, parent: int, name: str, *, no_follow: bool = False) -> int:
        self.children.append((parent, name, no_follow))
        return 202

    def info(self, handle: int) -> WindowsFileInfo:
        return {
            101: self.directory_info,
            202: self.source_info,
            303: self.stage_info,
        }[handle]

    def close(self, handle: int) -> None:
        self.closed.append(handle)

    def descriptor_from_handle(self, handle: int, *, read_only: bool) -> int:
        self.descriptors.append((handle, read_only))
        return 41 if handle == 202 else 42

    def handle_from_descriptor(self, descriptor: int) -> int:
        self.descriptor_handles.append(descriptor)
        return 202 if descriptor == 41 else 303

    def final_path(self, handle: int) -> str | None:
        assert handle == 202
        return self.final

    def create_child(self, parent: int, name: str) -> int:
        self.created.append((parent, name))
        return 303

    def publish_no_replace(self, stage: int, parent: int, name: str) -> None:
        self.published.append((stage, parent, name))
        if self.publish_error is not None:
            raise self.publish_error

    def discard_private_stage(self, handle: int) -> None:
        self.discarded.append(handle)

    def sync_directory(self, handle: int) -> None:
        self.synced.append(handle)


def _install(monkeypatch: pytest.MonkeyPatch) -> ReceiptApi:
    api = ReceiptApi()
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)
    return api


@pytest.mark.parametrize(
    ("path", "parent", "basename", "root"),
    [
        ("source.eml", ".", "source.eml", 77),
        ("inbox\\source.eml", "inbox", "source.eml", 77),
        ("C:\\Inbox\\source.eml", "C:\\Inbox", "source.eml", None),
        (
            "\\\\server\\share\\Inbox\\source.eml",
            "\\\\server\\share\\Inbox",
            "source.eml",
            None,
        ),
    ],
)
def test_parent_uses_captured_root_only_for_relative_windows_paths(
    path: str,
    parent: str,
    basename: str,
    root: int | None,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    api = _install(monkeypatch)
    assert native_windows_binding._parent(path) == (101, parent, basename)  # ruff: ignore[private-member-access] - exact parent-handle boundary.
    assert api.open_directories == [(parent, root)]


def test_read_all_requests_one_extra_byte_and_preserves_chunk_order(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    requests: list[tuple[int, int]] = []
    chunks = iter((b"ab", b"c", b""))

    def read(descriptor: int, size: int) -> bytes:
        requests.append((descriptor, size))
        return next(chunks)

    monkeypatch.setattr(native_windows_binding, "MAX_RAW_BYTES", 3)
    monkeypatch.setattr(native_windows_binding.__dict__["os"], "read", read)
    assert native_windows_binding._read_all(41) == b"abc"  # ruff: ignore[private-member-access] - inclusive raw-size ceiling.
    assert requests == [(41, 4), (41, 2), (41, 1)]

    oversized = iter((b"abc", b"d"))
    monkeypatch.setattr(
        native_windows_binding.__dict__["os"], "read", lambda *_args: next(oversized)
    )
    with pytest.raises(AppError) as captured:
        native_windows_binding._read_all(41)  # ruff: ignore[private-member-access] - over-ceiling raw-size rejection.
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
    )


def test_destination_binding_and_reopen_preserve_address_identity_and_closure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    api = _install(monkeypatch)
    bound = native_windows_binding.bind_destination(
        "requested.eml", "C:\\Inbox\\out.eml"
    )
    assert bound == BoundDestination(
        path_value("requested.eml"),
        path_value("C:\\Inbox"),
        "out.eml",
        FileIdentity(7, 101, "directory", 809),
    )
    assert api.open_directories == [("C:\\Inbox", None)]
    assert api.closed == [101]

    opened = native_windows_binding.open_bound_destination(bound)
    assert opened == BoundDirectory(101, windows=True)
    assert api.open_directories == [("C:\\Inbox", None), ("C:\\Inbox", None)]

    api.directory_info = WindowsFileInfo(
        volume_serial=9,
        file_id=101,
        change_time=809,
        last_write_time=810,
        size=0,
        directory=True,
        reparse=False,
    )
    with pytest.raises(AppError) as captured:
        native_windows_binding.open_bound_destination(bound)
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination parent changed after binding"
    )
    assert api.closed == [101, 101]
    native_windows_binding.close_bound_directory(opened)
    assert api.closed == [101, 101, 101]


def test_source_identity_and_snapshot_transfer_and_close_ownership(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    api = _install(monkeypatch)
    expected_identity = FileIdentity(7, 202, "regular", 901)
    assert (
        native_windows_binding.inspect_source_identity("C:\\Inbox\\source.eml")
        == expected_identity
    )
    assert api.children == [(101, "source.eml", False)]
    assert api.closed == [202, 101]

    api.children.clear()
    api.closed.clear()
    descriptor_closures: list[int] = []
    monkeypatch.setattr(native_windows_binding, "_read_all", lambda _fd: b"payload")
    monkeypatch.setattr(
        native_windows_binding.__dict__["os"], "close", descriptor_closures.append
    )
    snapshot = native_windows_binding.read_source(
        "requested.eml", "C:\\Inbox\\source.eml"
    )
    assert snapshot == SourceSnapshot(
        path_value("requested.eml"),
        path_value("C:\\Inbox\\source.eml"),
        path_value("C:\\Inbox"),
        "source.eml",
        path_value("\\\\?\\C:\\Inbox\\source.eml"),
        expected_identity,
        0o600,
        b"payload",
        hashlib.sha256(b"payload").hexdigest(),
        7,
    )
    assert api.children == [(101, "source.eml", False)]
    assert api.descriptors == [(202, True)]
    assert api.descriptor_handles == [41]
    assert descriptor_closures == [41]
    assert api.closed == [101]


def test_nofollow_and_stage_publication_helpers_preserve_handle_receipts(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    api = _install(monkeypatch)
    directory = BoundDirectory(101, windows=True)
    assert native_windows_binding.child_lstat(directory, "out.eml") == FileIdentity(
        7, 202, "regular", 901
    )
    assert api.children == [(101, "out.eml", True)]
    assert api.closed == [202]

    descriptor = native_windows_binding.open_child_nofollow(directory, "out.eml")
    assert descriptor == 41
    assert api.children == [(101, "out.eml", True), (101, "out.eml", True)]
    assert api.descriptors == [(202, True)]

    stage = native_windows_binding.create_private_stage(directory, "stage.tmp")
    assert stage == 42
    assert api.created == [(101, "stage.tmp")]
    assert api.descriptors == [(202, True), (303, False)]
    assert (
        native_windows_binding.publish_stage_no_replace(
            directory, stage, "stage.tmp", "out.eml"
        )
        is False
    )
    native_windows_binding.discard_private_stage(directory, stage, "stage.tmp")
    assert native_windows_binding.sync_bound_directory(directory) == "succeeded"
    assert api.published == [(303, 101, "out.eml")]
    assert api.discarded == [303]
    assert api.synced == [101]

    api.publish_error = FileExistsError("already present")
    with pytest.raises(AppError) as captured:
        native_windows_binding.publish_stage_no_replace(
            directory, stage, "stage.tmp", "out.eml"
        )
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination already exists"
    )
    assert isinstance(captured.value.__cause__, FileExistsError)
