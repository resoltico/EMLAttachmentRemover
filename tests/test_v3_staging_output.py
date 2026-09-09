"""Direct race and fault contracts for v3 private staging/publication."""

from __future__ import annotations

import ctypes
import hashlib
import os
import platform
import signal
from concurrent.futures import ThreadPoolExecutor
from dataclasses import replace
from threading import Barrier
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from eml_attachment_remover import atomic_publish, staged_output
from eml_attachment_remover.domain import AppError, ExitCode, FileIdentity, PathValue
from eml_attachment_remover.native_paths import BoundDirectoryHandle, bind_destination
from eml_attachment_remover.staged_output import PublishedWithError, publish

if TYPE_CHECKING:
    from pathlib import Path


def _destination(tmp_path: Path, name: str = "output.eml") -> Path:
    return tmp_path / name


def _temporary_names(directory: Path) -> list[Path]:
    return sorted(directory.glob(".eml-remove-*.tmp"))


def test_publish_readdresses_live_final_handle_and_digest(tmp_path: Path) -> None:
    destination = _destination(tmp_path)
    candidate = b"Content-Type: text/plain\r\n\r\nbody\r\n"
    receipt = publish(bind_destination(str(destination)), candidate)
    metadata = destination.stat()
    assert receipt.visibility == "visible"
    assert receipt.address_verified is True
    assert receipt.final_address is not None
    assert receipt.final_address.text == str(destination)
    assert receipt.identity is not None
    assert receipt.identity.inode == metadata.st_ino
    assert receipt.digest == hashlib.sha256(candidate).hexdigest()
    assert destination.read_bytes() == candidate
    assert stat_mode(destination) == 0o600
    assert _temporary_names(tmp_path) == []


def stat_mode(path: Path) -> int:
    return path.stat().st_mode & 0o777


def test_competing_publish_has_one_winner_and_one_truthful_conflict(
    tmp_path: Path,
) -> None:
    destination = _destination(tmp_path)
    barrier = Barrier(2)

    def publish_at_edge(candidate: bytes) -> PublicationOrError:
        barrier.wait()
        try:
            return publish(bind_destination(str(destination)), candidate)
        except AppError as error:
            return error

    candidates = (b"winner-one", b"winner-two")
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(publish_at_edge, candidates))
    created = [result for result in results if not isinstance(result, AppError)]
    conflicts = [result for result in results if isinstance(result, AppError)]
    assert len(created) == 1
    assert len(conflicts) == 1
    assert conflicts[0].code is ExitCode.OUTPUT_CONFLICT
    assert destination.read_bytes() in candidates
    assert _temporary_names(tmp_path) == []


type PublicationOrError = object


def test_post_edge_sync_failure_reconciles_before_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = _destination(tmp_path)
    candidate = b"post-edge"

    def fail_sync(_parent: BoundDirectoryHandle) -> str:
        raise AppError(ExitCode.WRITE_ERROR, "injected directory sync failure")

    monkeypatch.setattr(staged_output, "sync_bound_directory", fail_sync)
    with pytest.raises(PublishedWithError) as raised:
        publish(bind_destination(str(destination)), candidate)
    error = raised.value
    assert isinstance(error.cause, AppError)
    assert error.receipt.visibility == "visible"
    assert error.receipt.address_verified is True
    assert error.receipt.final_address is not None
    assert destination.read_bytes() == candidate
    assert _temporary_names(tmp_path) == []


def test_nested_cancellation_keeps_primary_and_all_close_failures(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = _destination(tmp_path)
    candidate = b"interrupted-after-edge"
    original_close = staged_output._close_descriptor  # ruff: ignore[private-member-access] - direct cleanup fault injection.
    original_directory_close = staged_output._close_directory  # ruff: ignore[private-member-access] - direct cleanup fault injection.
    close_count = 0

    def fail_sync(_parent: BoundDirectoryHandle) -> str:
        raise KeyboardInterrupt

    def close_with_receipt(descriptor: int) -> BaseException | None:
        nonlocal close_count
        close_count += 1
        actual = original_close(descriptor)
        assert actual is None
        if close_count > 2:
            return OSError(f"injected close {close_count}")
        return None

    def directory_close_with_receipt(
        directory: BoundDirectoryHandle,
    ) -> BaseException | None:
        actual = original_directory_close(directory)
        assert actual is None
        return OSError("injected directory close")

    monkeypatch.setattr(staged_output, "sync_bound_directory", fail_sync)
    monkeypatch.setattr(staged_output, "_close_descriptor", close_with_receipt)
    monkeypatch.setattr(staged_output, "_close_directory", directory_close_with_receipt)
    with pytest.raises(PublishedWithError) as raised:
        publish(bind_destination(str(destination)), candidate)
    error = raised.value
    assert isinstance(error.cause, KeyboardInterrupt)
    assert isinstance(error.cleanup_cause, BaseExceptionGroup)
    assert error.receipt.visibility == "visible"
    assert error.receipt.temp_cleanup == "succeeded"
    assert destination.read_bytes() == candidate
    assert _temporary_names(tmp_path) == []


def test_stage_owner_construction_interrupt_leaves_no_sensitive_temp(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = _destination(tmp_path)

    def interrupt_owner(_descriptor: int, _name: bytes) -> object:
        raise KeyboardInterrupt

    monkeypatch.setattr(staged_output, "_Stage", interrupt_owner)
    with pytest.raises(KeyboardInterrupt):
        publish(bind_destination(str(destination)), b"sensitive staging bytes")
    assert not destination.exists()
    assert _temporary_names(tmp_path) == []


def test_cleanup_closes_each_descriptor_after_entry_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = bind_destination(str(_destination(tmp_path)))
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct cleanup state test.
        destination, b"x", "digest"
    )
    state.parent = BoundDirectoryHandle(101, windows=False)
    state.stage = staged_output._Stage(  # ruff: ignore[private-member-access] - direct cleanup state test.
        102, b"private.tmp"
    )
    closed: list[int] = []

    def record_close(descriptor: int) -> None:
        closed.append(descriptor)

    def fail_entry(_state: object) -> BaseException | None:
        return OSError("entry cleanup failed")

    monkeypatch.setattr(os, "close", record_close)
    monkeypatch.setattr(staged_output, "_remove_stage_entry", fail_entry)
    temp_cleanup, cause = staged_output._cleanup(  # ruff: ignore[private-member-access] - direct cleanup state test.
        state
    )
    assert temp_cleanup == "failed"
    assert isinstance(cause, OSError)
    assert closed == [102, 101]


def test_atomic_backend_selects_darwin_and_portable_edges(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    darwin_calls: list[tuple[int, bytes, bytes]] = []
    portable_calls: list[tuple[int, bytes, bytes]] = []

    def darwin(parent: int, stage: bytes, destination: bytes) -> None:
        darwin_calls.append((parent, stage, destination))

    def portable(parent: int, stage: bytes, destination: bytes) -> None:
        portable_calls.append((parent, stage, destination))

    monkeypatch.setattr(atomic_publish, "_darwin_rename_exclusive", darwin)
    monkeypatch.setattr(atomic_publish, "_link_exclusive", portable)
    monkeypatch.setattr(platform, "system", lambda: "Darwin")
    assert atomic_publish.publish_no_replace(1, b"stage", b"destination") is False
    monkeypatch.setattr(platform, "system", lambda: "Linux")
    assert atomic_publish.publish_no_replace(2, b"other", b"final") is True
    assert darwin_calls == [(1, b"stage", b"destination")]
    assert portable_calls == [(2, b"other", b"final")]


@pytest.mark.parametrize(
    ("result", "failure", "code"),
    [(0, 0, None), (-1, 17, ExitCode.OUTPUT_CONFLICT), (-1, 5, ExitCode.WRITE_ERROR)],
)
def test_darwin_rename_result_is_classified_truthfully(
    monkeypatch: pytest.MonkeyPatch,
    result: int,
    failure: int,
    code: ExitCode | None,
) -> None:
    operation = Mock(return_value=result)
    library = SimpleNamespace(renameatx_np=operation)
    monkeypatch.setattr(ctypes, "CDLL", lambda *_args, **_kwargs: library)
    monkeypatch.setattr(ctypes, "get_errno", lambda: failure)
    if code is None:
        atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - direct native primitive contract.
            1, b"stage", b"final"
        )
    else:
        with pytest.raises(AppError) as raised:
            atomic_publish._darwin_rename_exclusive(  # ruff: ignore[private-member-access] - direct native primitive contract.
                1, b"stage", b"final"
            )
        assert raised.value.code is code
    assert operation.call_args.args == (1, b"stage", 1, b"final", 4)


@pytest.mark.parametrize("failure", [None, FileExistsError(), OSError("link failed")])
def test_portable_link_result_is_classified_truthfully(
    monkeypatch: pytest.MonkeyPatch, failure: OSError | None
) -> None:
    def link(*_args: object, **_kwargs: object) -> None:
        if failure is not None:
            raise failure

    monkeypatch.setattr(os, "link", link)
    if failure is None:
        atomic_publish._link_exclusive(  # ruff: ignore[private-member-access] - direct portable primitive contract.
            1, b"stage", b"final"
        )
        return
    with pytest.raises(AppError) as raised:
        atomic_publish._link_exclusive(  # ruff: ignore[private-member-access] - direct portable primitive contract.
            1, b"stage", b"final"
        )
    expected = (
        ExitCode.OUTPUT_CONFLICT
        if isinstance(failure, FileExistsError)
        else ExitCode.WRITE_ERROR
    )
    assert raised.value.code is expected


@pytest.mark.parametrize(
    ("failure", "expected"),
    [(None, "succeeded"), (OSError(22, "unsupported"), "unsupported")],
)
def test_directory_sync_accepts_only_success_or_known_unsupported(
    monkeypatch: pytest.MonkeyPatch, failure: OSError | None, expected: str
) -> None:
    def fsync(_descriptor: int) -> None:
        if failure is not None:
            raise failure

    monkeypatch.setattr(os, "fsync", fsync)
    assert atomic_publish.sync_directory(1) == expected


def test_directory_sync_surfaces_unknown_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        os, "fsync", lambda _descriptor: (_ for _ in ()).throw(OSError(5, "bad"))
    )
    with pytest.raises(AppError) as raised:
        atomic_publish.sync_directory(1)
    assert raised.value.code is ExitCode.WRITE_ERROR


def test_signal_deferral_and_parent_binding_fail_closed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    masks: list[int] = []

    def mask(operation: int, _signals: set[int]) -> set[int]:
        masks.append(operation)
        return set()

    monkeypatch.setattr(signal, "pthread_sigmask", mask, raising=False)
    monkeypatch.setitem(signal.__dict__, "SIG_BLOCK", 1)
    monkeypatch.setitem(signal.__dict__, "SIG_SETMASK", 2)
    with staged_output._defer_signals():  # ruff: ignore[private-member-access] - direct signal-shield contract.
        pass
    monkeypatch.delattr(signal, "SIGHUP", raising=False)
    with staged_output._defer_signals():  # ruff: ignore[private-member-access] - SIGHUP-absent contract.
        pass
    block = signal.__dict__["SIG_BLOCK"]
    restore = signal.__dict__["SIG_SETMASK"]
    assert masks == [block, restore] * 2
    bound = bind_destination(str(_destination(tmp_path)))
    missing_parent = replace(bound, parent=PathValue(None, "<none>", None))
    missing_state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct binding contract.
        missing_parent, b"", ""
    )
    with pytest.raises(AppError):
        staged_output._bind_parent(missing_state)  # ruff: ignore[private-member-access] - direct binding contract.
    changed = replace(bound, directory_identity=FileIdentity(0, 0, "directory", 0))
    changed_state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct binding contract.
        changed, b"", ""
    )
    with pytest.raises(AppError):
        staged_output._bind_parent(changed_state)  # ruff: ignore[private-member-access] - direct binding contract.
    assert changed_state.parent is None


def test_staging_helpers_cover_empty_short_and_collision(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = bind_destination(str(_destination(tmp_path)))
    empty = tmp_path / "empty"
    empty.write_bytes(b"")
    descriptor = os.open(empty, os.O_RDONLY)
    try:
        assert staged_output._read_all(descriptor) == b""  # ruff: ignore[private-member-access] - direct EOF contract.
    finally:
        os.close(descriptor)
    assert staged_output._close_descriptor(-1) is None  # ruff: ignore[private-member-access] - direct close contract.
    missing = staged_output._PublicationState(destination, b"x", "x")  # ruff: ignore[private-member-access] - direct staging contract.
    with pytest.raises(AppError):
        staged_output._verify_staged(missing)  # ruff: ignore[private-member-access] - missing stage contract.
    state = staged_output._PublicationState(destination, b"x", "x")  # ruff: ignore[private-member-access] - direct staging contract.
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - direct staging contract.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - direct staging contract.
    with monkeypatch.context() as context:
        context.setattr(os, "write", lambda *_args: 0)
        with pytest.raises(AppError):
            staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - short-write contract.
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.
    collision = staged_output._PublicationState(destination, b"", "")  # ruff: ignore[private-member-access] - collision contract.
    collision.parent = BoundDirectoryHandle(1, windows=False)
    with monkeypatch.context() as context:
        context.setattr(
            os,
            "open",
            lambda *_args, **_kwargs: (_ for _ in ()).throw(FileExistsError()),
        )
        with pytest.raises(AppError):
            staged_output._create_stage(collision)  # ruff: ignore[private-member-access] - bounded collision contract.


def test_final_receipt_detects_competitor_and_reconciles(
    tmp_path: Path,
) -> None:
    destination = bind_destination(str(_destination(tmp_path)))
    absent = staged_output._PublicationState(destination, b"x", "x")  # ruff: ignore[private-member-access] - direct receipt contract.
    with pytest.raises(AppError):
        staged_output._read_final_receipt(absent)  # ruff: ignore[private-member-access] - missing-stage receipt contract.
    candidate = b"candidate"
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct receipt contract.
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - direct receipt contract.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - direct receipt contract.
    staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - direct receipt contract.
    _destination(tmp_path).write_bytes(b"competitor")
    with pytest.raises(AppError):
        staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - competitor identity contract.
    receipt = staged_output._reconcile(state, "failed")  # ruff: ignore[private-member-access] - reconciliation contract.
    assert receipt.visibility == "not_proven"
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_link_fallback_and_finish_matrix(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = bind_destination(str(_destination(tmp_path)))
    empty = staged_output._PublicationState(destination, b"", "")  # ruff: ignore[private-member-access] - direct lifecycle contract.
    assert staged_output._cleanup(empty) == ("succeeded", None)  # ruff: ignore[private-member-access] - direct lifecycle contract.
    with pytest.raises(AppError):
        staged_output._publish_edge(empty)  # ruff: ignore[private-member-access] - absent stage contract.
    candidate = b"linked"
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - link fallback contract.
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - link fallback contract.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - link fallback contract.
    staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - link fallback contract.

    def link_edge(
        parent: BoundDirectoryHandle,
        descriptor: int,
        stage: bytes | str,
        final: bytes | str,
    ) -> bool:
        assert isinstance(stage, bytes)
        assert isinstance(final, bytes)
        assert descriptor >= 0
        os.link(
            stage, final, src_dir_fd=parent.descriptor, dst_dir_fd=parent.descriptor
        )
        return True

    monkeypatch.setattr(staged_output, "publish_stage_no_replace", link_edge)
    staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - portable link-fallback contract.
    assert state.receipt is not None
    assert state.receipt.address_verified
    assert staged_output._cleanup(state) == ("succeeded", None)  # ruff: ignore[private-member-access] - explicit test cleanup.
    failed = staged_output._PublicationState(destination, b"", "")  # ruff: ignore[private-member-access] - finish matrix.
    with pytest.raises(BaseExceptionGroup):
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - pre-edge dual-cause contract.
            failed, OSError("primary"), ("failed", RuntimeError("cleanup"))
        )


def test_deferred_interrupt_after_publication_is_not_created(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    destination = _destination(tmp_path)

    def mask(operation: int, _signals: set[int]) -> set[int]:
        if operation == signal.__dict__["SIG_SETMASK"]:
            raise KeyboardInterrupt
        return set()

    monkeypatch.setattr(signal, "pthread_sigmask", mask, raising=False)
    monkeypatch.setitem(signal.__dict__, "SIG_SETMASK", 2)
    with pytest.raises(PublishedWithError) as raised:
        publish(bind_destination(str(destination)), b"shielded")
    assert isinstance(raised.value.cause, KeyboardInterrupt)
    assert raised.value.receipt.visibility == "visible"
    assert destination.read_bytes() == b"shielded"
