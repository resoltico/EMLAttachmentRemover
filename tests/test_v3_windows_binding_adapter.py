"""Synthetic Windows binding contracts above the ctypes ABI adapter."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_windows_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    ExitCode,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_windows import WindowsFileInfo

if TYPE_CHECKING:
    from pathlib import Path

    from _pytest.monkeypatch import MonkeyPatch


@dataclass
class BindingApi:
    """Deterministic handle backend used to exercise native binding policy."""

    descriptor: int
    content_size: int
    reparse: bool = False
    missing: bool = False
    directory_child: bool = False
    directory_parent: bool = True
    open_error: bool = False
    changed: bool = False
    info_calls: int = 0
    closed: list[int] = field(default_factory=list)
    published: list[tuple[int, int, str]] = field(default_factory=list)
    discarded: list[int] = field(default_factory=list)
    synced: list[int] = field(default_factory=list)

    @staticmethod
    def open_directory(_path: str, _root: int | None) -> int:
        """Return the stable fake destination directory handle.

        Returns:
            The stable directory handle.

        """
        return 10

    def open_child(self, _parent: int, _name: str, *, no_follow: bool = False) -> int:
        """Return the fake source handle or emulate a missing child.

        Returns:
            A deterministic no-follow or ordinary file handle.

        """
        if self.missing:
            raise FileNotFoundError
        if self.open_error:
            message = "open failed"
            raise OSError(message)
        return 20 if no_follow else 21

    def info(self, handle: int) -> WindowsFileInfo:
        """Return regular-file or directory identity facts for one fake handle.

        Returns:
            The deterministic metadata for the supplied fake handle.

        """
        directory = self.directory_parent if handle == 10 else self.directory_child
        self.info_calls += 1
        change = 5 + self.info_calls if self.changed and handle == 21 else 5
        return WindowsFileInfo(
            4, handle, change, 6, self.content_size, directory, self.reparse
        )

    def close(self, handle: int) -> None:
        """Record native-handle closure."""
        self.closed.append(handle)

    def descriptor_from_handle(self, _handle: int, *, read_only: bool) -> int:
        """Return the actual fixture descriptor used by production read code.

        Returns:
            The supplied fixture descriptor.

        """
        assert isinstance(read_only, bool)
        return self.descriptor

    @staticmethod
    def handle_from_descriptor(_descriptor: int) -> int:
        """Return the regular source identity for the fixture descriptor.

        Returns:
            The fake regular-file handle.

        """
        return 21

    @staticmethod
    def final_path(_handle: int) -> str | None:
        """Return a final-address snapshot.

        Returns:
            The stable fake final Windows path.

        """
        return "\\\\?\\C:\\bound\\message.eml"

    @staticmethod
    def create_child(_parent: int, _name: str) -> int:
        """Return the fake private-stage handle.

        Returns:
            The private stage handle.

        """
        return 22

    def publish_no_replace(self, stage: int, parent: int, name: str) -> None:
        """Record the no-replace handle publication request."""
        self.published.append((stage, parent, name))

    def discard_private_stage(self, handle: int) -> None:
        """Record the exact private-stage cleanup handle."""
        self.discarded.append(handle)

    def sync_directory(self, handle: int) -> None:
        """Record destination directory synchronization."""
        self.synced.append(handle)


def _use_api(monkeypatch: MonkeyPatch, api: BindingApi) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)


def _destination() -> BoundDestination:
    identity = FileIdentity(4, 10, "directory", 5)
    return BoundDestination(
        PathValue("C:\\bound\\out.eml", "out", None, "YQ=="),
        PathValue("C:\\bound", "bound", None, "Yg=="),
        "out.eml",
        identity,
    )


def test_windows_binding_reads_and_publishes_through_fake_handles(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, source.stat().st_size)
    _use_api(monkeypatch, api)
    bound = native_windows_binding.bind_destination(
        "C:\\bound\\out.eml", "C:\\bound\\out.eml"
    )
    assert bound.basename == "out.eml"
    assert (
        native_windows_binding.inspect_source_identity("C:\\bound\\message.eml").inode
        == 21
    )
    snapshot = native_windows_binding.read_source(
        "requested.eml", "C:\\bound\\message.eml"
    )
    assert snapshot.raw == b"body"
    directory = native_windows_binding.open_bound_destination(_destination())
    stage = native_windows_binding.create_private_stage(directory, "stage.tmp")
    assert stage == descriptor
    assert (
        native_windows_binding.publish_stage_no_replace(
            directory, stage, "stage.tmp", "out.eml"
        )
        is False
    )
    assert native_windows_binding.child_lstat(directory, "out.eml") is not None
    native_windows_binding.discard_private_stage(directory, stage, "stage.tmp")
    assert native_windows_binding.sync_bound_directory(directory) == "succeeded"
    native_windows_binding.close_bound_directory(directory)
    assert api.published == [(21, 10, "out.eml")]
    assert api.discarded == [21]
    assert api.synced == [10]


def test_windows_binding_rejects_missing_or_reparse_destinations(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, 0, reparse=True)
    _use_api(monkeypatch, api)
    directory = native_windows_binding.open_bound_destination(_destination())
    with pytest.raises(AppError) as captured:
        native_windows_binding.child_lstat(directory, "linked.eml")
    assert captured.value.code is ExitCode.OUTPUT_CONFLICT
    api.reparse = False
    api.missing = True
    assert native_windows_binding.child_lstat(directory, "absent.eml") is None
    native_windows_binding.close_bound_directory(directory)
    os.close(descriptor)


def test_windows_binding_maps_native_no_replace_collisions_to_output_conflicts(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    """Report native rename collisions through the stable application error code."""
    source = tmp_path / "source.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, source.stat().st_size)
    _use_api(monkeypatch, api)
    directory = native_windows_binding.open_bound_destination(_destination())
    monkeypatch.setattr(
        api,
        "publish_no_replace",
        lambda *_args: (_ for _ in ()).throw(FileExistsError()),
    )
    with pytest.raises(AppError) as raised:
        native_windows_binding.publish_stage_no_replace(
            directory, descriptor, "stage.tmp", "out.eml"
        )
    assert raised.value.code is ExitCode.OUTPUT_CONFLICT
    native_windows_binding.close_bound_directory(directory)
    os.close(descriptor)


def test_windows_binding_exercises_expected_failure_and_stability_edges(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, 4)
    _use_api(monkeypatch, api)
    api.directory_parent = False
    with pytest.raises(AppError) as destination_error:
        native_windows_binding.bind_destination(
            "C:\\bound\\out.eml", "C:\\bound\\out.eml"
        )
    assert destination_error.value.code is ExitCode.WRITE_ERROR
    api.directory_parent = True
    api.open_error = True
    with pytest.raises(AppError) as source_error:
        native_windows_binding.inspect_source_identity("C:\\bound\\message.eml")
    assert source_error.value.code is ExitCode.INPUT_ERROR
    api.open_error = False
    api.changed = True
    api.info_calls = 0
    with pytest.raises(AppError) as change_error:
        native_windows_binding.read_source("requested.eml", "C:\\bound\\message.eml")
    assert change_error.value.code is ExitCode.INPUT_ERROR
    missing_parent = BoundDestination(
        PathValue(None, "none", None, None),
        PathValue(None, "none", None, None),
        "out.eml",
        FileIdentity(4, 10, "directory", 5),
    )
    with pytest.raises(AppError) as parent_error:
        native_windows_binding.open_bound_destination(missing_parent)
    assert parent_error.value.code is ExitCode.WRITE_ERROR


def test_windows_binding_rejects_nonregular_source_and_existing_output(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "source.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, 0, directory_child=True)
    _use_api(monkeypatch, api)
    with pytest.raises(AppError) as source_error:
        native_windows_binding.inspect_source_identity("C:\\bound\\message.eml")
    assert source_error.value.code is ExitCode.INPUT_ERROR
    directory = native_windows_binding.open_bound_destination(_destination())
    with pytest.raises(AppError) as existing_error:
        native_windows_binding.open_child_nofollow(directory, "linked.eml")
    assert existing_error.value.code is ExitCode.OUTPUT_CONFLICT
    native_windows_binding.close_bound_directory(directory)
    os.close(descriptor)


def test_windows_binding_cwd_parent_and_read_error_edges(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, source.stat().st_size)
    _use_api(monkeypatch, api)
    monkeypatch.setattr(native_windows_binding.__dict__["os"], "name", "nt")
    assert native_windows_binding._capture_start_cwd() == 10  # ruff: ignore[private-member-access] - process-start CWD binding.
    monkeypatch.setattr(native_windows_binding.__dict__["os"], "name", "posix")
    assert native_windows_binding._capture_start_cwd() is None  # ruff: ignore[private-member-access] - non-Windows import guard.

    monkeypatch.setattr(native_windows_binding, "_START_CWD_HANDLE", None)
    native_windows_binding._cwd.cache_clear()  # ruff: ignore[private-member-access] - cached CWD error contract.
    with pytest.raises(OSError, match="unavailable"):
        native_windows_binding._cwd()  # ruff: ignore[private-member-access] - cached CWD error contract.

    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 10)
    api.open_error = True
    with pytest.raises(AppError):
        native_windows_binding.read_source("requested", "relative\\message.eml")
    os.close(descriptor)


def test_windows_binding_snapshot_identity_and_directory_receipts(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    source = tmp_path / "message.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, source.stat().st_size, directory_child=True)
    _use_api(monkeypatch, api)
    with pytest.raises(AppError):
        native_windows_binding._snapshot("r", "e", "p", "n", descriptor)  # ruff: ignore[private-member-access] - source-kind snapshot boundary.
    api.directory_child = False
    api.content_size = 1
    with pytest.raises(AppError):
        native_windows_binding._snapshot("r", "e", "p", "n", descriptor)  # ruff: ignore[private-member-access] - source-size stability boundary.

    identity = native_windows_binding.descriptor_identity(descriptor)
    assert identity.inode == 21
    changed = BoundDestination(
        PathValue("C:\\bound\\out.eml", "out", None, "YQ=="),
        PathValue("C:\\bound", "bound", None, "Yg=="),
        "out.eml",
        FileIdentity(99, 10, "directory", 5),
    )
    with pytest.raises(AppError):
        native_windows_binding.open_bound_destination(changed)
    os.close(descriptor)


def test_windows_binding_bounded_read_rejects_oversized_existing_bytes(
    monkeypatch: MonkeyPatch,
) -> None:
    reads = iter((b"one", b"two"))
    monkeypatch.setattr(
        native_windows_binding.__dict__["os"], "read", lambda *_args: next(reads)
    )
    monkeypatch.setattr(native_windows_binding, "MAX_RAW_BYTES", 3)
    with pytest.raises(AppError):
        native_windows_binding._read_all(3)  # ruff: ignore[private-member-access] - existing-output native bound.


def test_windows_binding_cache_parent_and_handle_cleanup_edges(
    tmp_path: Path, monkeypatch: MonkeyPatch
) -> None:
    class ApiFactory:
        pass

    native_windows_binding._api.cache_clear()  # ruff: ignore[private-member-access] - Windows API construction cache.
    monkeypatch.setattr(native_windows_binding, "WindowsApi", ApiFactory)
    assert isinstance(native_windows_binding._api(), ApiFactory)  # ruff: ignore[private-member-access] - Windows API construction cache.
    native_windows_binding._api.cache_clear()  # ruff: ignore[private-member-access] - avoid leaking fake cache.

    monkeypatch.setattr(native_windows_binding, "_START_CWD_HANDLE", 44)
    native_windows_binding._cwd.cache_clear()  # ruff: ignore[private-member-access] - cached CWD handle receipt.
    assert native_windows_binding._cwd() == 44  # ruff: ignore[private-member-access] - cached CWD handle receipt.
    native_windows_binding._cwd.cache_clear()  # ruff: ignore[private-member-access] - avoid leaking cached CWD.

    class ParentFailure:
        @staticmethod
        def open_directory(_path: str, _root: int | None) -> int:
            message = "parent"
            raise OSError(message)

    def parent_api() -> ParentFailure:
        return ParentFailure()

    monkeypatch.setitem(native_windows_binding.__dict__, "_api", parent_api)
    with pytest.raises(AppError):
        native_windows_binding._parent("relative\\message.eml")  # ruff: ignore[private-member-access] - parent native-open context.
    with pytest.raises(AppError) as reopen_error:
        native_windows_binding.open_bound_destination(_destination())
    assert reopen_error.value.code is ExitCode.WRITE_ERROR

    source = tmp_path / "message.eml"
    source.write_bytes(b"body")
    descriptor = os.open(source, os.O_RDONLY)
    api = BindingApi(descriptor, source.stat().st_size)
    _use_api(monkeypatch, api)

    def descriptor_failure(_handle: int, *, read_only: bool) -> int:
        assert read_only is True
        message = "descriptor"
        raise OSError(message)

    monkeypatch.setattr(api, "descriptor_from_handle", descriptor_failure)
    with pytest.raises(AppError):
        native_windows_binding.read_source("requested", "C:\\bound\\message.eml")
    assert 21 in api.closed

    monkeypatch.setattr(
        api,
        "descriptor_from_handle",
        BindingApi.descriptor_from_handle.__get__(api),
    )
    native_windows_binding._require_regular(21)  # ruff: ignore[private-member-access] - regular existing-output acceptance.
    directory = native_windows_binding.open_bound_destination(_destination())
    assert (
        native_windows_binding.open_child_nofollow(directory, "out.eml") == descriptor
    )
    native_windows_binding.close_bound_directory(directory)
    os.close(descriptor)
