"""Exact POSIX native-operation receipts for mutation-resistant safety checks."""

from __future__ import annotations

import hashlib
import os
from types import SimpleNamespace
from typing import cast

import pytest

from eml_attachment_remover import native_posix
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
)


def _metadata() -> SimpleNamespace:
    return SimpleNamespace(
        st_dev=11,
        st_ino=12,
        st_mode=0o100640,
        st_ctime_ns=13,
        st_size=3,
        st_mtime_ns=14,
    )


def _value(value: str) -> PathValue:
    return PathValue(value, f"display:{value}", f"native:{value}")


def _raise_os(*_arguments: object, **_keywords: object) -> object:
    message = "fault"
    raise OSError(message)


def test_posix_identity_split_directory_and_read_receipts_are_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Keep the descriptor root, read budget, and native identity exact."""
    metadata = _metadata()
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "filemode", lambda _mode: "-rw-r-----"
    )
    assert native_posix._identity(  # ruff: ignore[private-member-access] - native identity receipt.
        cast("os.stat_result", metadata)
    ) == FileIdentity(11, 12, "-rw-r-----", 13)
    assert native_posix._same_directory(  # ruff: ignore[private-member-access] - directory identity rule.
        FileIdentity(1, 2, "directory", 3), FileIdentity(1, 2, "directory", 99)
    )
    for changed in (
        FileIdentity(9, 2, "directory", 3),
        FileIdentity(1, 9, "directory", 3),
        FileIdentity(1, 2, "regular", 3),
    ):
        assert not native_posix._same_directory(  # ruff: ignore[private-member-access] - every bound-directory identity component matters.
            FileIdentity(1, 2, "directory", 3), changed
        )
    assert native_posix._split("leaf") == (b".", b"leaf")  # ruff: ignore[private-member-access] - relative parent split.
    assert native_posix._split("/leaf") == (b"/", b"leaf")  # ruff: ignore[private-member-access] - root parent split.
    assert native_posix._split("one/two") == (b"one", b"two")  # ruff: ignore[private-member-access] - nested parent split.

    opens: list[tuple[object, object, object]] = []

    def open_directory(
        expression: object, flags: object, *, dir_fd: object = None
    ) -> int:
        opens.append((expression, flags, dir_fd))
        return 71

    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_directory)
    assert native_posix._directory(b"/root") == 71  # ruff: ignore[private-member-access] - absolute directory binding.
    assert native_posix._directory(b"relative") == 71  # ruff: ignore[private-member-access] - captured-CWD directory binding.
    assert opens == [
        (b"/root", native_posix._CWD_FLAGS, None),  # ruff: ignore[private-member-access] - no root descriptor for absolute paths.
        (b"relative", native_posix._CWD_FLAGS, native_posix._START_CWD_FD),  # ruff: ignore[private-member-access] - captured descriptor for relative paths.
    ]

    reads = iter((b"ab", b"c", b""))
    read_calls: list[tuple[int, int]] = []

    def read(descriptor: int, requested: int) -> bytes:
        read_calls.append((descriptor, requested))
        return next(reads)

    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 3)
    monkeypatch.setattr(native_posix.__dict__["os"], "read", read)
    assert native_posix._read_all(23) == b"abc"  # ruff: ignore[private-member-access] - exact raw read assembly.
    assert read_calls == [(23, 4), (23, 2), (23, 1)]

    large_read_calls: list[tuple[int, int]] = []

    def read_large(descriptor: int, requested: int) -> bytes:
        large_read_calls.append((descriptor, requested))
        return b""

    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 2 * 1024 * 1024)
    monkeypatch.setattr(native_posix.__dict__["os"], "read", read_large)
    assert native_posix._read_all(24) == b""  # ruff: ignore[private-member-access] - fixed maximum native read chunk.
    assert large_read_calls == [(24, 1024 * 1024)]

    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 3)
    monkeypatch.setattr(native_posix.__dict__["os"], "read", lambda *_args: b"four")
    with pytest.raises(AppError) as captured:
        native_posix._read_all(25)  # ruff: ignore[private-member-access] - exact over-limit source failure.
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
    )


def test_posix_binding_and_source_open_receipts_close_every_descriptor(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Bind and inspect with exact native flags, names, and cleanup order."""
    metadata = _metadata()
    closed: list[int] = []
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (31, "parent", b"leaf"))
    monkeypatch.setattr(native_posix, "path_value", _value)
    monkeypatch.setattr(
        native_posix,
        "_identity",
        lambda value: FileIdentity(11, 12, "directory", value.st_ctime_ns),
    )
    monkeypatch.setattr(
        native_posix.__dict__["os"], "fstat", lambda _descriptor: metadata
    )
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "S_ISDIR", lambda mode: mode == metadata.st_mode
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)
    assert native_posix._bind_destination(  # ruff: ignore[private-member-access] - complete destination binding receipt.
        "request", "expanded"
    ) == BoundDestination(
        _value("request"),
        _value("parent"),
        b"leaf",
        FileIdentity(11, 12, "directory", 13),
    )
    assert closed == [31]

    closed.clear()
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_ISDIR", lambda _mode: False)
    with pytest.raises(AppError) as captured:
        native_posix._bind_destination(  # ruff: ignore[private-member-access] - non-directory parent rejection.
            "request", "expanded"
        )
    assert captured.value == AppError(
        ExitCode.WRITE_ERROR, "destination parent is not a directory"
    )
    assert closed == [31]

    opens: list[tuple[object, object, object]] = []

    def open_source(name: object, flags: object, *, dir_fd: object) -> int:
        opens.append((name, flags, dir_fd))
        return 32

    closed.clear()
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_source)
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "S_ISREG", lambda mode: mode == metadata.st_mode
    )
    monkeypatch.setattr(
        native_posix,
        "_identity",
        lambda value: FileIdentity(11, 12, "regular", value.st_ctime_ns),
    )
    assert native_posix._inspect_source_identity("expanded") == FileIdentity(  # ruff: ignore[private-member-access] - complete source identity receipt.
        11, 12, "regular", 13
    )
    expected_flags = (
        os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_CLOEXEC", 0)
    )
    assert opens == [(b"leaf", expected_flags, 31)]
    assert closed == [32, 31]

    closed.clear()
    monkeypatch.setattr(native_posix.__dict__["os"], "open", _raise_os)
    with pytest.raises(AppError) as captured:
        native_posix._inspect_source_identity("expanded")  # ruff: ignore[private-member-access] - source-open failure.
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not inspect source: fault"
    )
    assert closed == [31]


def test_posix_snapshot_and_source_read_receipts_are_complete(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Require an immutable source record from the same opened descriptor."""
    metadata = _metadata()
    values = iter((metadata, metadata))
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: next(values))
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "S_ISREG", lambda mode: mode == metadata.st_mode
    )
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_IMODE", lambda _mode: 0o640)
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "filemode", lambda _mode: "-rw-r-----"
    )
    monkeypatch.setattr(
        native_posix,
        "_read_all",
        lambda descriptor: b"raw" if descriptor == 44 else b"",
    )
    monkeypatch.setattr(
        native_posix, "_final_address", lambda path: _value(f"final:{path}")
    )
    monkeypatch.setattr(native_posix, "path_value", _value)
    result = native_posix._snapshot("request", "expanded", "parent", b"leaf", 44)  # ruff: ignore[private-member-access] - full source snapshot receipt.
    assert result.request == _value("request")
    assert result.expanded == _value("expanded")
    assert result.parent == _value("parent")
    assert result.basename == b"leaf"
    assert result.final_address == _value("final:expanded")
    assert result.identity == FileIdentity(11, 12, "-rw-r-----", 13)
    assert result.mode == 0o640
    assert result.raw == b"raw"
    assert result.digest == hashlib.sha256(b"raw").hexdigest()
    assert result.size == 3

    calls: list[tuple[object, ...]] = []
    closed: list[int] = []
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (41, "parent", b"leaf"))
    monkeypatch.setattr(
        native_posix.__dict__["os"], "open", lambda *_args, **_kwargs: 42
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    def snapshot(*arguments: object) -> object:
        calls.append(arguments)
        return "snapshot"

    monkeypatch.setattr(native_posix, "_snapshot", snapshot)
    source_result = native_posix._read_source(  # ruff: ignore[private-member-access] - read uses opened child descriptor.
        "request", "expanded"
    )
    assert cast("object", source_result) == "snapshot"
    assert calls == [("request", "expanded", "parent", b"leaf", 42)]
    assert closed == [42, 41]


def test_posix_publication_primitives_preserve_exact_kernel_arguments(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Require stage creation, inspection, and no-follow reads to be relative."""
    directory = BoundDirectory(51, windows=False)
    opens: list[tuple[object, object, object, object]] = []

    def open_stage(
        name: object, flags: object, mode: object = None, *, dir_fd: object = None
    ) -> int:
        opens.append((name, flags, mode, dir_fd))
        return 52

    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_stage)
    assert native_posix._create_private_stage(directory, b"stage") == 52  # ruff: ignore[private-member-access] - exclusive private-stage create.
    assert opens == [
        (
            b"stage",
            os.O_RDWR | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0),
            0o600,
            51,
        )
    ]

    with monkeypatch.context() as context:
        context.delattr(native_posix.__dict__["os"], "O_CLOEXEC", raising=False)
        opens.clear()
        assert native_posix._create_private_stage(directory, b"portable") == 52  # ruff: ignore[private-member-access] - portable close-on-exec fallback.
        assert opens == [(b"portable", os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600, 51)]

    metadata = _metadata()
    stats: list[tuple[object, object, object]] = []

    def stat_entry(
        name: object, *, dir_fd: object, follow_symlinks: object
    ) -> SimpleNamespace:
        stats.append((name, dir_fd, follow_symlinks))
        return metadata

    monkeypatch.setattr(native_posix.__dict__["os"], "stat", stat_entry)
    monkeypatch.setattr(
        native_posix.__dict__["stat"], "filemode", lambda _mode: "-rw-r-----"
    )
    assert native_posix._child_lstat(directory, b"final") == FileIdentity(  # ruff: ignore[private-member-access] - lstat child identity receipt.
        11, 12, "-rw-r-----", 13
    )
    assert stats == [(b"final", 51, False)]
    monkeypatch.setattr(
        native_posix.__dict__["os"],
        "stat",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(FileNotFoundError()),
    )
    assert native_posix._child_lstat(directory, b"missing") is None  # ruff: ignore[private-member-access] - missing child is not a failure.

    opens.clear()
    assert native_posix._open_child_nofollow(directory, b"final") == 52  # ruff: ignore[private-member-access] - no-follow existing-output read.
    assert opens == [(b"final", os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), None, 51)]


def test_posix_bound_directory_failures_preserve_exact_receipts(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reject incomplete or swapped parents before a publication can proceed."""
    missing_parent = BoundDestination(
        _value("request"),
        PathValue(None, "none", None),
        b"leaf",
        FileIdentity(1, 2, "directory", 3),
    )
    with pytest.raises(AppError) as captured:
        native_posix._open_bound_destination(missing_parent)  # ruff: ignore[private-member-access] - parent address is mandatory.
    assert captured.value == AppError(
        ExitCode.WRITE_ERROR, "destination parent has no native address"
    )

    destination = BoundDestination(
        _value("request"), _value("parent"), b"leaf", FileIdentity(1, 2, "directory", 3)
    )
    directories: list[bytes] = []
    closed: list[int] = []

    def directory(value: bytes) -> int:
        directories.append(value)
        return 61

    monkeypatch.setattr(native_posix, "_directory", directory)
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: _metadata())
    monkeypatch.setattr(
        native_posix, "_identity", lambda _value: FileIdentity(9, 9, "directory", 9)
    )
    monkeypatch.setattr(native_posix, "_same_directory", lambda _left, _right: False)
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)
    with pytest.raises(AppError) as captured:
        native_posix._open_bound_destination(destination)  # ruff: ignore[private-member-access] - bound parent identity must remain stable.
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination parent changed after binding"
    )
    assert directories == [b"parent"]
    assert closed == [61]
