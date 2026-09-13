"""Exact state-machine receipts for v3 staged publication."""

from __future__ import annotations

import hashlib
import os
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import staged_output
from eml_attachment_remover.domain import (
    AppError,
    ExitCode,
    FileIdentity,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import BoundDirectoryHandle, bind_destination
from eml_attachment_remover.staged_output import (
    PublishedWithError,
    _PublicationState,  # ruff: ignore[import-private-name] - direct state-machine receipt contract.
)

if TYPE_CHECKING:
    from pathlib import Path


def _state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    """Build a state with stable candidate facts and no live native resources.

    Returns:
        An unbound publication state suitable for direct transition tests.

    """
    return _PublicationState(
        bind_destination(str(tmp_path / "output.eml")),
        candidate,
        hashlib.sha256(candidate).hexdigest(),
    )


def _visible_receipt(state: _PublicationState) -> PublicationReceipt:
    """Build the complete receipt expected after a re-addressed publication.

    Returns:
        The precise verified receipt for the supplied candidate state.

    """
    return PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(1, 2, "regular", len(state.candidate)),
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="pending",
    )


def test_read_all_uses_the_bounded_chunk_size_until_the_exact_eof(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The receipt reader must make ordered fixed-size reads through EOF."""
    calls: list[tuple[int, int]] = []
    chunks = iter((b"first", b"second", b""))

    def read(descriptor: int, size: int) -> bytes:
        calls.append((descriptor, size))
        return next(chunks)

    monkeypatch.setattr(os, "read", read)
    assert staged_output._read_all(41) == b"firstsecond"  # ruff: ignore[private-member-access] - direct bounded-read contract.
    assert calls == [(41, 1024 * 1024)] * 3


def test_final_receipt_has_a_complete_ordered_live_handle_evidence_chain(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Require every identity, digest, and address receipt fact exactly once."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(11, windows=False)
    state.stage = staged_output._Stage(12, b"private")  # ruff: ignore[private-member-access] - direct state-machine receipt.
    expected = FileIdentity(1, 2, "regular", len(state.candidate))
    calls: list[tuple[str, object]] = []

    def identity(descriptor: int) -> FileIdentity:
        calls.append(("identity", descriptor))
        return expected

    def lstat(parent: BoundDirectoryHandle, basename: bytes | str) -> FileIdentity:
        calls.append(("lstat", (parent, basename)))
        return expected

    def open_final(parent: BoundDirectoryHandle, basename: bytes | str) -> int:
        calls.append(("open", (parent, basename)))
        return 13

    def read_final(descriptor: int) -> bytes:
        calls.append(("read", descriptor))
        return state.candidate

    def close_final(descriptor: int) -> BaseException | None:
        calls.append(("close", descriptor))
        return None

    monkeypatch.setattr(staged_output, "descriptor_identity", identity)
    monkeypatch.setattr(staged_output, "child_lstat", lstat)
    monkeypatch.setattr(staged_output, "open_child_nofollow", open_final)
    monkeypatch.setattr(staged_output, "_read_all", read_final)
    monkeypatch.setattr(staged_output, "_close_descriptor", close_final)

    receipt = staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - direct final-address receipt.

    assert receipt == _visible_receipt(state)
    assert calls == [
        ("identity", 12),
        ("lstat", (state.parent, state.destination.basename)),
        ("open", (state.parent, state.destination.basename)),
        ("identity", 13),
        ("read", 13),
        ("identity", 13),
        ("lstat", (state.parent, state.destination.basename)),
        ("close", 13),
    ]


def test_stage_creation_retries_exactly_sixteen_collisions_then_preserves_owner(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Bound collisions while preserving the successful descriptor/name pair exactly."""
    collision = _state(tmp_path)
    collision.parent = BoundDirectoryHandle(21, windows=False)
    collision_names: list[bytes] = []

    def collision_name() -> bytes:
        name = f"private-{len(collision_names)}".encode()
        collision_names.append(name)
        return name

    monkeypatch.setattr(staged_output, "private_stage_name", collision_name)
    monkeypatch.setattr(
        staged_output,
        "create_private_stage",
        lambda *_arguments: (_ for _ in ()).throw(FileExistsError()),
    )
    with pytest.raises(AppError) as exhausted:
        staged_output._create_stage(collision)  # ruff: ignore[private-member-access] - bounded name-allocation contract.
    assert exhausted.value == AppError(
        ExitCode.WRITE_ERROR, "could not allocate a private staging name"
    )
    assert collision_names == [f"private-{index}".encode() for index in range(16)]
    assert collision.stage is None

    successful = _state(tmp_path)
    successful.parent = BoundDirectoryHandle(22, windows=False)
    names = iter((b"first", b"second"))
    calls: list[tuple[BoundDirectoryHandle, bytes | str]] = []

    def create(parent: BoundDirectoryHandle, name: bytes | str) -> int:
        calls.append((parent, name))
        if name == b"first":
            raise FileExistsError
        return 23

    monkeypatch.setattr(staged_output, "private_stage_name", lambda: next(names))
    monkeypatch.setattr(staged_output, "create_private_stage", create)
    staged_output._create_stage(successful)  # ruff: ignore[private-member-access] - owner transfer contract.
    assert successful.stage == staged_output._Stage(23, b"second")  # ruff: ignore[private-member-access] - exact transferred private owner.
    assert calls == [(successful.parent, b"first"), (successful.parent, b"second")]


def test_publish_edge_records_exact_link_cleanup_and_sync_sequence(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Portable link publication must clean its remaining private link then resync."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(31, windows=False)
    state.stage = staged_output._Stage(32, b"private")  # ruff: ignore[private-member-access] - direct visibility-edge state.
    receipt = _visible_receipt(state)
    calls: list[tuple[str, object]] = []

    def publish(
        parent: BoundDirectoryHandle,
        descriptor: int,
        stage_name: bytes | str,
        destination_name: bytes | str,
    ) -> bool:
        calls.append(("publish", (parent, descriptor, stage_name, destination_name)))
        return True

    def read_receipt(observed: _PublicationState) -> PublicationReceipt:
        calls.append(("receipt", observed))
        return receipt

    def sync(parent: BoundDirectoryHandle) -> str:
        calls.append(("sync", parent))
        return "unsupported" if len(calls) == 3 else "succeeded"

    def remove(observed: _PublicationState) -> BaseException | None:
        calls.append(("remove", observed))
        return None

    def reconcile(
        observed: _PublicationState, directory_sync: str
    ) -> PublicationReceipt:
        calls.append(("reconcile", (observed, directory_sync)))
        return receipt

    monkeypatch.setattr(staged_output, "publish_stage_no_replace", publish)
    monkeypatch.setattr(staged_output, "_read_final_receipt", read_receipt)
    monkeypatch.setattr(staged_output, "sync_bound_directory", sync)
    monkeypatch.setattr(staged_output, "_remove_stage_entry", remove)
    monkeypatch.setattr(staged_output, "_reconcile", reconcile)

    staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - direct publication-edge contract.

    assert state.kernel_published is True
    assert state.receipt is receipt
    assert calls == [
        (
            "publish",
            (state.parent, 32, b"private", state.destination.basename),
        ),
        ("receipt", state),
        ("sync", state.parent),
        ("remove", state),
        ("sync", state.parent),
        ("reconcile", (state, "succeeded")),
    ]


def test_cleanup_retains_every_independent_failure_and_releases_owners(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Cleanup must not let one failure skip closing the stage or parent owner."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(41, windows=False)
    state.stage = staged_output._Stage(42, b"private")  # ruff: ignore[private-member-access] - direct cleanup state.
    entry = OSError("entry")
    stage_close = OSError("stage close")
    parent_close = OSError("parent close")
    calls: list[tuple[str, object]] = []

    def remove(observed: _PublicationState) -> BaseException | None:
        calls.append(("remove", observed))
        return entry

    def close_descriptor(descriptor: int) -> BaseException | None:
        calls.append(("stage", descriptor))
        return stage_close

    def close_parent(parent: BoundDirectoryHandle) -> BaseException | None:
        calls.append(("parent", parent))
        return parent_close

    monkeypatch.setattr(staged_output, "_remove_stage_entry", remove)
    monkeypatch.setattr(staged_output, "_close_descriptor", close_descriptor)
    monkeypatch.setattr(staged_output, "_close_directory", close_parent)

    outcome, cause = staged_output._cleanup(state)  # ruff: ignore[private-member-access] - direct independent-cleanup contract.

    assert outcome == "failed"
    assert isinstance(cause, BaseExceptionGroup)
    assert cause.exceptions == (entry, stage_close, parent_close)
    assert calls == [
        ("remove", state),
        ("stage", 42),
        ("parent", BoundDirectoryHandle(41, windows=False)),
    ]
    assert state.stage.descriptor == -1
    assert cast("BoundDirectoryHandle | None", state.parent) is None


def test_reconcile_and_finish_preserve_complete_post_edge_receipts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A post-edge failure retains its final receipt and cleanup truth verbatim."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(51, windows=False)
    state.stage = staged_output._Stage(52, b"private")  # ruff: ignore[private-member-access] - direct reconciliation state.
    final = _visible_receipt(state)
    monkeypatch.setattr(staged_output, "_read_final_receipt", lambda _state: final)
    reconciled = staged_output._reconcile(state, "unsupported")  # ruff: ignore[private-member-access] - exact successful reconciliation.
    assert reconciled == PublicationReceipt(
        visibility="visible",
        identity=final.identity,
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="unsupported",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="pending",
    )

    state.kernel_published = True
    state.receipt = reconciled
    primary = AppError(ExitCode.WRITE_ERROR, "post-edge")
    with pytest.raises(PublishedWithError) as raised:
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - final receipt/cause boundary.
            state, primary, ("failed", None)
        )
    assert raised.value.cause is primary
    assert raised.value.cleanup_cause is None
    assert raised.value.receipt == PublicationReceipt(
        visibility="visible",
        identity=final.identity,
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="unsupported",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="failed",
    )
