"""POSIX descriptor binding error and stability boundary contracts."""

from __future__ import annotations

from dataclasses import replace
from types import SimpleNamespace

import pytest

from eml_attachment_remover import native_posix
from eml_attachment_remover.domain import AppError, BoundDirectory, FileIdentity


def _metadata(*, size: int = 1, inode: int = 2, mtime: int = 3) -> SimpleNamespace:
    return SimpleNamespace(
        st_mode=0,
        st_dev=1,
        st_ino=inode,
        st_ctime_ns=4,
        st_size=size,
        st_mtime_ns=mtime,
    )


def _module_value(name: str) -> object:
    return native_posix.__dict__[name]


def test_directory_parent_and_read_boundaries_are_contextual(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[tuple[object, object]] = []

    def open_directory(
        expression: object,
        _flags: object,
        *,
        dir_fd: object = None,
    ) -> int:
        calls.append((expression, dir_fd))
        return 8

    monkeypatch.setattr(
        _module_value("os"),
        "open",
        open_directory,
    )
    assert native_posix._directory(b"relative") == 8  # ruff: ignore[private-member-access] - captured-CWD descriptor boundary.
    assert calls[0][0] == b"relative"

    monkeypatch.setattr(native_posix, "_directory", lambda _expression: _raise_os())
    with pytest.raises(AppError):
        native_posix._parent("parent/file")  # ruff: ignore[private-member-access] - parent-open context.

    reads = iter((b"body", b""))
    monkeypatch.setattr(_module_value("os"), "read", lambda *_args: next(reads))
    assert native_posix._read_all(3) == b"body"  # ruff: ignore[private-member-access] - source read contract.
    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 3)
    reads = iter((b"body", b"more"))
    monkeypatch.setattr(_module_value("os"), "read", lambda *_args: next(reads))
    with pytest.raises(AppError):
        native_posix._read_all(3)  # ruff: ignore[private-member-access] - raw-size limit.


def test_binding_and_source_identity_reject_nonregular_or_missing_entries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    metadata = _metadata()
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (5, "parent", b"file"))
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: metadata)
    monkeypatch.setattr(_module_value("os"), "close", lambda _fd: None)
    monkeypatch.setattr(_module_value("stat"), "S_ISDIR", lambda _mode: False)
    with pytest.raises(AppError):
        native_posix._bind_destination("request", "expanded")  # ruff: ignore[private-member-access] - destination-kind boundary.

    monkeypatch.setattr(_module_value("os"), "open", lambda *_args, **_kwargs: 6)
    monkeypatch.setattr(_module_value("stat"), "S_ISREG", lambda _mode: False)
    with pytest.raises(AppError):
        native_posix._inspect_source_identity("expanded")  # ruff: ignore[private-member-access] - source-kind boundary.

    monkeypatch.setattr(_module_value("os"), "open", _raise_os)
    with pytest.raises(AppError):
        native_posix._inspect_source_identity("expanded")  # ruff: ignore[private-member-access] - source-inspection context and unopened-descriptor cleanup.
    with pytest.raises(AppError):
        native_posix._read_source("request", "expanded")  # ruff: ignore[private-member-access] - source-read context.


def test_snapshot_rejects_kind_size_stability_and_final_address_faults(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    metadata = _metadata()
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: metadata)
    monkeypatch.setattr(_module_value("stat"), "S_ISREG", lambda _mode: False)
    with pytest.raises(AppError):
        native_posix._snapshot("r", "e", "p", b"n", 3)  # ruff: ignore[private-member-access] - source-kind snapshot boundary.

    monkeypatch.setattr(_module_value("stat"), "S_ISREG", lambda _mode: True)
    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 1)
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: _metadata(size=2))
    with pytest.raises(AppError):
        native_posix._snapshot("r", "e", "p", b"n", 3)  # ruff: ignore[private-member-access] - source-size snapshot boundary.

    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 8)
    changed = iter((_metadata(size=1, inode=2), _metadata(size=1, inode=9)))
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: next(changed))
    monkeypatch.setattr(native_posix, "_read_all", lambda _fd: b"x")
    with pytest.raises(AppError):
        native_posix._snapshot("r", "e", "p", b"n", 3)  # ruff: ignore[private-member-access] - source-identity stability boundary.

    stable = _metadata(size=2)
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: stable)
    monkeypatch.setattr(native_posix, "_read_all", lambda _fd: b"x")
    with pytest.raises(AppError):
        native_posix._snapshot("r", "e", "p", b"n", 3)  # ruff: ignore[private-member-access] - source-size stability boundary.

    monkeypatch.setattr(
        _module_value("os"), "path", SimpleNamespace(realpath=_raise_os)
    )
    assert native_posix._final_address("expanded") is None  # ruff: ignore[private-member-access] - final-address uncertainty receipt.


def _successful_posix_backend(
    monkeypatch: pytest.MonkeyPatch,
) -> tuple[SimpleNamespace, list[int], list[tuple[object, object, object]]]:
    """Install portable fakes for successful POSIX descriptor operations.

    Returns:
        Shared metadata plus closure and open-call receipts.

    """
    metadata = _metadata()
    closed: list[int] = []
    opened: list[tuple[object, object, object]] = []

    def open_path(
        name: object,
        flags: object,
        mode: object | None = None,
        *,
        dir_fd: object = None,
    ) -> int:
        assert mode is None or mode == 0o600
        opened.append((name, flags, dir_fd))
        return 7

    monkeypatch.setattr(native_posix, "_parent", lambda _path: (6, "parent", b"x"))
    monkeypatch.setattr(_module_value("os"), "open", open_path)
    monkeypatch.setattr(_module_value("os"), "fstat", lambda _fd: metadata)
    monkeypatch.setattr(_module_value("os"), "close", closed.append)
    monkeypatch.setattr(_module_value("stat"), "S_ISDIR", lambda _mode: True)
    monkeypatch.setattr(_module_value("stat"), "S_ISREG", lambda _mode: True)
    return metadata, closed, opened


def test_posix_binding_and_snapshot_success_paths_are_platform_independent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Exercise POSIX binding and snapshots through fakes on every CI platform."""
    metadata, closed, opened = _successful_posix_backend(monkeypatch)
    assert native_posix._directory(b"/root") == 7  # ruff: ignore[private-member-access] - absolute descriptor root.
    assert native_posix._directory(b"relative") == 7  # ruff: ignore[private-member-access] - captured descriptor root.
    bound = native_posix.bind_destination("request", "expanded")
    assert bound.basename == b"x"
    assert native_posix.inspect_source_identity("expanded").inode == metadata.st_ino
    marker = object()
    original_snapshot = native_posix._snapshot  # ruff: ignore[private-member-access] - source receipt boundary.
    monkeypatch.setattr(native_posix, "_snapshot", lambda *_args: marker)
    assert native_posix.read_source("request", "expanded") is marker
    monkeypatch.setattr(native_posix, "_snapshot", original_snapshot)
    monkeypatch.setattr(native_posix, "_read_all", lambda _fd: b"x")
    monkeypatch.setattr(native_posix, "_final_address", lambda _path: None)
    snapshot = native_posix._snapshot("r", "e", "p", b"n", 7)  # ruff: ignore[private-member-access] - stable descriptor receipt.
    assert snapshot.raw == b"x"
    assert opened
    assert closed


def test_posix_publication_primitives_are_platform_independent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Exercise POSIX publication primitive delegation on every CI platform."""
    metadata, closed, opened = _successful_posix_backend(monkeypatch)
    bound = native_posix.bind_destination("request", "expanded")
    directory = native_posix.open_bound_destination(bound)
    assert directory == BoundDirectory(7, windows=False)
    changed = replace(bound, directory_identity=FileIdentity(0, 0, "other", 0))
    with pytest.raises(AppError):
        native_posix.open_bound_destination(changed)
    assert native_posix.descriptor_identity(7).inode == metadata.st_ino
    assert native_posix.create_private_stage(directory, b"stage") == 7
    monkeypatch.setattr(native_posix, "publish_no_replace", lambda *_args: False)
    assert (
        native_posix.publish_stage_no_replace(directory, 7, b"stage", b"final") is False
    )
    monkeypatch.setattr(_module_value("os"), "stat", lambda *_args, **_kwargs: metadata)
    assert native_posix.child_lstat(directory, b"final") is not None
    monkeypatch.setattr(
        _module_value("os"),
        "stat",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(FileNotFoundError()),
    )
    assert native_posix.child_lstat(directory, b"missing") is None
    assert native_posix.open_child_nofollow(directory, b"final") == 7
    unlinked: list[tuple[object, object]] = []
    monkeypatch.setattr(
        _module_value("os"),
        "unlink",
        lambda name, *, dir_fd: unlinked.append((name, dir_fd)),
    )
    native_posix.discard_private_stage(directory, 7, b"stage")
    monkeypatch.setattr(native_posix, "sync_directory", lambda _fd: "succeeded")
    assert native_posix.sync_bound_directory(directory) == "succeeded"
    assert opened
    assert closed
    assert unlinked == [(b"stage", 7)]


def _raise_os(*_arguments: object, **_keywords: object) -> object:
    message = "fault"
    raise OSError(message)
