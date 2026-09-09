"""Failure and cleanup receipts for POSIX source descriptors."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from eml_attachment_remover import native_posix
from eml_attachment_remover.domain import AppError, ExitCode, FileIdentity


def _metadata(
    *,
    inode: int = 12,
    changed: int = 13,
    size: int = 3,
    modified: int = 14,
) -> SimpleNamespace:
    return SimpleNamespace(
        st_dev=11,
        st_ino=inode,
        st_mode=0o100640,
        st_ctime_ns=changed,
        st_size=size,
        st_mtime_ns=modified,
    )


def test_source_inspection_wraps_descriptor_failures_and_closes_every_handle(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The child and its parent are closed even when descriptor inspection fails."""
    closed: list[int] = []
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (61, "inbox", b"x"))
    monkeypatch.setattr(
        native_posix.__dict__["os"], "open", lambda *_args, **_kwargs: 62
    )
    monkeypatch.setattr(
        native_posix.__dict__["os"],
        "fstat",
        lambda _descriptor: (_ for _ in ()).throw(OSError("unstatable")),
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    with pytest.raises(AppError) as captured:
        native_posix._inspect_source_identity("inbox/x")  # ruff: ignore[private-member-access] - descriptor inspection cleanup boundary.

    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not inspect source: unstatable"
    )
    assert isinstance(captured.value.__cause__, OSError)
    assert closed == [62, 61]


def test_source_read_preserves_snapshot_domain_errors_and_closes_in_order(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A validation rejection is not relabelled as I/O and cannot leak handles."""
    closed: list[int] = []
    expected = AppError(ExitCode.INPUT_ERROR, "source changed while it was being read")
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (71, "inbox", b"x"))
    monkeypatch.setattr(
        native_posix.__dict__["os"], "open", lambda *_args, **_kwargs: 72
    )
    monkeypatch.setattr(
        native_posix, "_snapshot", lambda *_args: (_ for _ in ()).throw(expected)
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    with pytest.raises(AppError) as captured:
        native_posix._read_source("x", "inbox/x")  # ruff: ignore[private-member-access] - domain failure preservation boundary.

    assert captured.value is expected
    assert captured.value.__cause__ is None
    assert closed == [72, 71]


def test_source_read_wraps_snapshot_os_errors_with_exact_cleanup_and_context(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """I/O faults after opening the source retain user-facing read context."""
    closed: list[int] = []
    opens: list[tuple[object, object, object]] = []

    def open_source(name: object, flags: object, *, dir_fd: object) -> int:
        opens.append((name, flags, dir_fd))
        return 82

    monkeypatch.setattr(native_posix, "_parent", lambda _path: (81, "inbox", b"x"))
    monkeypatch.setattr(native_posix.__dict__["os"], "open", open_source)
    monkeypatch.setattr(
        native_posix,
        "_snapshot",
        lambda *_args: (_ for _ in ()).throw(OSError("read failed")),
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    with pytest.raises(AppError) as captured:
        native_posix._read_source("request", "inbox/x")  # ruff: ignore[private-member-access] - post-open I/O receipt.

    expected_flags = (
        native_posix.__dict__["os"].O_RDONLY
        | getattr(native_posix.__dict__["os"], "O_NONBLOCK", 0)
        | getattr(native_posix.__dict__["os"], "O_CLOEXEC", 0)
    )
    assert opens == [(b"x", expected_flags, 81)]
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "could not read source: read failed"
    )
    assert isinstance(captured.value.__cause__, OSError)
    assert closed == [82, 81]


@pytest.mark.parametrize(
    ("before", "after", "raw", "message"),
    [
        (
            _metadata(modified=14),
            _metadata(modified=15),
            b"raw",
            "source changed while it was being read",
        ),
        (
            _metadata(changed=13),
            _metadata(changed=99),
            b"raw",
            "source changed while it was being read",
        ),
        (
            _metadata(size=4),
            _metadata(size=4),
            b"raw",
            "source read size changed during snapshot",
        ),
    ],
)
def test_snapshot_rejects_each_independent_stability_receipt(
    before: SimpleNamespace,
    after: SimpleNamespace,
    raw: bytes,
    message: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Metadata and payload evidence must agree independently before acceptance."""
    observations = iter((before, after))
    monkeypatch.setattr(
        native_posix.__dict__["os"], "fstat", lambda _fd: next(observations)
    )
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_ISREG", lambda _mode: True)
    monkeypatch.setattr(native_posix, "_read_all", lambda _fd: raw)
    monkeypatch.setattr(
        native_posix,
        "_identity",
        lambda value: FileIdentity(
            value.st_dev, value.st_ino, "regular", value.st_ctime_ns
        ),
    )

    with pytest.raises(AppError) as captured:
        native_posix._snapshot("request", "expanded", "parent", b"x", 91)  # ruff: ignore[private-member-access] - each immutable-source receipt matters.

    assert captured.value == AppError(ExitCode.INPUT_ERROR, message)
