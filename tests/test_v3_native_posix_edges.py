"""POSIX descriptor binding error and stability boundary contracts."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from eml_attachment_remover import native_posix
from eml_attachment_remover.domain import AppError


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


def _raise_os(*_arguments: object, **_keywords: object) -> object:
    message = "fault"
    raise OSError(message)
