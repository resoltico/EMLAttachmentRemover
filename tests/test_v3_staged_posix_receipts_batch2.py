"""Second receipt batch for POSIX source reads and private staging."""

from __future__ import annotations

import hashlib
import os
from dataclasses import replace
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_posix, staged_output
from eml_attachment_remover.domain import (
    AppError,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import BoundDirectoryHandle, bind_destination

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.staged_output import _PublicationState


def _metadata(
    *,
    inode: int = 12,
    size: int = 2,
    changed: int = 13,
    modified: int = 14,
) -> SimpleNamespace:
    """Return complete metadata for one regular fixture descriptor.

    Returns:
        Complete regular-file metadata suitable for a mocked descriptor.

    """
    return SimpleNamespace(
        st_dev=11,
        st_ino=inode,
        st_mode=0o100640,
        st_ctime_ns=changed,
        st_size=size,
        st_mtime_ns=modified,
    )


def _state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    """Return direct lifecycle state with a bound private destination.

    Returns:
        A state suitable for one direct staging lifecycle receipt.

    """
    return staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct lifecycle receipt.
        bind_destination(str(tmp_path / "output.eml")),
        candidate,
        hashlib.sha256(candidate).hexdigest(),
    )


def test_posix_source_descriptor_zero_is_owned_and_closed_for_both_receipts(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Descriptor zero is a live child descriptor, never an absent sentinel."""
    metadata = _metadata()
    closed: list[int] = []
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (61, "inbox", b"x"))
    monkeypatch.setattr(
        native_posix.__dict__["os"], "open", lambda *_args, **_kwargs: 0
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: metadata)
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_ISREG", lambda _mode: True)
    monkeypatch.setattr(
        native_posix,
        "_identity",
        lambda value: FileIdentity(11, 12, "regular", value.st_ctime_ns),
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    assert native_posix._inspect_source_identity("inbox/x") == FileIdentity(  # ruff: ignore[private-member-access] - zero-descriptor inspection receipt.
        11, 12, "regular", 13
    )
    assert closed == [0, 61]

    closed.clear()
    expected = object()
    monkeypatch.setattr(native_posix, "_snapshot", lambda *_args: expected)
    assert (
        native_posix._read_source(  # ruff: ignore[private-member-access] - zero-descriptor read receipt.
            "request", "inbox/x"
        )
        is expected
    )
    assert closed == [0, 61]


def test_posix_snapshot_accepts_exact_empty_and_limit_sized_sources(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An unchanged empty or exactly limited source is a valid complete snapshot."""
    for raw, limit in ((b"", 0), (b"xy", 2)):
        before = _metadata(size=len(raw))
        observations = iter((before, before))
        monkeypatch.setattr(
            native_posix.__dict__["os"],
            "fstat",
            lambda _fd, observations=observations: next(observations),
        )
        monkeypatch.setattr(
            native_posix.__dict__["stat"], "S_ISREG", lambda _mode: True
        )
        monkeypatch.setattr(
            native_posix.__dict__["stat"], "S_IMODE", lambda _mode: 0o640
        )
        monkeypatch.setattr(
            native_posix.__dict__["stat"], "filemode", lambda _mode: "-rw-r-----"
        )
        monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", limit)
        monkeypatch.setattr(native_posix, "_read_all", lambda _fd, body=raw: body)
        monkeypatch.setattr(native_posix, "_final_address", lambda _path: None)
        snapshot = native_posix._snapshot(  # ruff: ignore[private-member-access] - exact boundary-size receipt.
            "request", "expanded", "parent", b"x", 0
        )
        assert snapshot.identity == FileIdentity(11, 12, "-rw-r-----", 13)
        assert snapshot.mode == 0o640
        assert snapshot.raw == raw
        assert snapshot.digest == hashlib.sha256(raw).hexdigest()
        assert snapshot.size == len(raw)
        assert snapshot.final_address is None


def test_posix_snapshot_rejects_a_source_beyond_the_exact_raw_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The raw-size ceiling is inclusive only at the exact permitted limit."""
    before = _metadata(size=3)
    monkeypatch.setattr(native_posix.__dict__["os"], "fstat", lambda _fd: before)
    monkeypatch.setattr(native_posix.__dict__["stat"], "S_ISREG", lambda _mode: True)
    monkeypatch.setattr(native_posix, "MAX_RAW_BYTES", 2)

    with pytest.raises(AppError) as raised:
        native_posix._snapshot("r", "e", "p", b"n", 0)  # ruff: ignore[private-member-access] - over-limit rejection before reading.

    assert raised.value == AppError(
        ExitCode.INPUT_ERROR, "source exceeds the 128 MiB raw-size limit"
    )


def test_private_stage_retries_the_full_collision_budget_before_success(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Every collision gets one new unpredictable name through attempt sixteen."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(41, windows=False)
    names = [f"stage-{index}".encode() for index in range(16)]
    observed: list[bytes] = []

    monkeypatch.setattr(
        staged_output, "private_stage_name", lambda: names[len(observed)]
    )

    def create(_parent: BoundDirectoryHandle, name: bytes) -> int:
        observed.append(name)
        if len(observed) < len(names):
            raise FileExistsError(name)
        return 77

    monkeypatch.setattr(staged_output, "create_private_stage", create)
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - full staging collision budget.

    assert observed == names
    assert state.stage is not None
    assert state.stage.descriptor == 77
    assert state.stage.name == names[-1]


def test_private_stage_does_not_relabel_noncollision_open_failures(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Only name collisions retry; an I/O failure retains its original cause."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(41, windows=False)
    failure = OSError("disk fault")
    monkeypatch.setattr(staged_output, "private_stage_name", lambda: b"stage")
    monkeypatch.setattr(
        staged_output,
        "create_private_stage",
        lambda *_args: (_ for _ in ()).throw(failure),
    )

    with pytest.raises(OSError, match="disk fault") as raised:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - noncollision opening receipt.

    assert raised.value is failure
    assert state.stage is None


def test_private_stage_constructor_failure_discards_and_closes_exact_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A failed owner transfer immediately cleans only the new stage descriptor."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(41, windows=False)
    failure = RuntimeError("owner transfer failed")
    discarded: list[tuple[BoundDirectoryHandle, int, bytes]] = []
    closed: list[int] = []
    monkeypatch.setattr(staged_output, "private_stage_name", lambda: b"stage")
    monkeypatch.setattr(staged_output, "create_private_stage", lambda *_args: 77)
    monkeypatch.setattr(
        staged_output,
        "_Stage",
        lambda *_args: (_ for _ in ()).throw(failure),
    )
    monkeypatch.setattr(
        staged_output,
        "discard_private_stage",
        lambda parent, descriptor, name: discarded.append((parent, descriptor, name)),
    )
    monkeypatch.setattr(staged_output, "_close_descriptor", closed.append)

    with pytest.raises(RuntimeError) as raised:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - immediate owner-transfer cleanup.

    assert raised.value is failure
    assert discarded == [(state.parent, 77, b"stage")]
    assert closed == [77]
    assert state.stage is None


def test_stage_verification_has_distinct_windows_and_posix_mode_receipts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Windows skips POSIX chmod while both paths retain ordered descriptor evidence."""
    for windows, expected in (
        (
            False,
            [
                ("write", 51, b"xy"),
                ("sync", 51),
                ("chmod", 51, 0o600),
                ("seek", 51, 0, os.SEEK_SET),
                ("read", 51),
            ],
        ),
        (
            True,
            [
                ("write", 51, b"xy"),
                ("sync", 51),
                ("seek", 51, 0, os.SEEK_SET),
                ("read", 51),
            ],
        ),
    ):
        state = _state(tmp_path, b"xy")
        state.parent = BoundDirectoryHandle(41, windows=windows)
        state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - direct verified stage.
        events: list[tuple[object, ...]] = []
        monkeypatch.setattr(
            staged_output.__dict__["os"],
            "write",
            lambda descriptor, data, events=events: (
                events.append(("write", descriptor, data)) or len(data)
            ),
        )
        monkeypatch.setattr(
            staged_output.__dict__["os"],
            "fsync",
            lambda descriptor, events=events: events.append(("sync", descriptor)),
        )
        monkeypatch.setattr(
            staged_output.__dict__["os"],
            "fchmod",
            lambda descriptor, mode, events=events: events.append((
                "chmod",
                descriptor,
                mode,
            )),
        )
        monkeypatch.setattr(
            staged_output.__dict__["os"],
            "lseek",
            lambda descriptor, offset, whence, events=events: events.append((
                "seek",
                descriptor,
                offset,
                whence,
            )),
        )
        monkeypatch.setattr(
            staged_output,
            "_read_all",
            lambda descriptor, events=events: (
                events.append(("read", descriptor)) or b"xy"
            ),
        )

        staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - platform-specific staged mode receipt.
        assert events == expected


def test_close_descriptor_zero_is_not_a_missing_cleanup_handle(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Descriptor zero must be closed, while negative one remains the sentinel."""
    closed: list[int] = []
    monkeypatch.setattr(staged_output.__dict__["os"], "close", closed.append)

    assert staged_output._close_descriptor(0) is None  # ruff: ignore[private-member-access] - valid zero descriptor cleanup.
    assert staged_output._close_descriptor(-1) is None  # ruff: ignore[private-member-access] - absent descriptor sentinel.
    assert closed == [0]


def test_publish_stops_before_visibility_after_a_stage_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A pre-edge stage failure never calls verification or the visibility edge."""
    destination = bind_destination(str(tmp_path / "output.eml"))
    failure = AppError(ExitCode.WRITE_ERROR, "stage unavailable")
    events: list[str] = []

    def bind(state: _PublicationState) -> None:
        events.append("bind")
        state.parent = BoundDirectory(41, windows=False)

    def create(_state: _PublicationState) -> None:
        events.append("stage")
        raise failure

    def cleanup(_state: _PublicationState) -> tuple[str, None]:
        events.append("cleanup")
        return ("succeeded", None)

    monkeypatch.setattr(staged_output, "_bind_parent", bind)
    monkeypatch.setattr(staged_output, "_create_stage", create)
    monkeypatch.setattr(
        staged_output,
        "_verify_staged",
        lambda _state: events.append("verify"),
    )
    monkeypatch.setattr(
        staged_output,
        "_publish_edge",
        lambda _state: events.append("edge"),
    )
    monkeypatch.setattr(staged_output, "_cleanup", cleanup)

    with pytest.raises(AppError) as raised:
        staged_output.publish(destination, b"candidate")

    assert raised.value is failure
    assert events == ["bind", "stage", "cleanup"]


def test_publish_returns_the_complete_post_edge_receipt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The publication boundary returns only its fully accepted lifecycle receipt."""
    destination = bind_destination(str(tmp_path / "output.eml"))
    candidate = b"candidate"
    digest = hashlib.sha256(candidate).hexdigest()
    receipt = PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(1, 2, "regular", 3),
        digest=digest,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=destination.request,
        temp_cleanup="pending",
    )
    events: list[str] = []

    def bind(state: _PublicationState) -> None:
        events.append("bind")
        state.parent = BoundDirectory(41, windows=False)

    def create(state: _PublicationState) -> None:
        events.append("stage")
        state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - owned artificial stage.

    def verify(_state: _PublicationState) -> None:
        events.append("verify")

    def edge(state: _PublicationState) -> None:
        events.append("edge")
        state.kernel_published = True
        state.receipt = receipt

    def cleanup(_state: _PublicationState) -> tuple[str, None]:
        events.append("cleanup")
        return ("succeeded", None)

    monkeypatch.setattr(staged_output, "_bind_parent", bind)
    monkeypatch.setattr(staged_output, "_create_stage", create)
    monkeypatch.setattr(staged_output, "_verify_staged", verify)
    monkeypatch.setattr(staged_output, "_publish_edge", edge)
    monkeypatch.setattr(staged_output, "_cleanup", cleanup)

    assert staged_output.publish(destination, candidate) == replace(
        receipt, temp_cleanup="succeeded"
    )
    assert events == ["bind", "stage", "verify", "edge", "cleanup"]
