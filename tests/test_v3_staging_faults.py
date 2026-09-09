"""Direct edge-state contracts for v3 cross-platform publication staging."""

from __future__ import annotations

import hashlib
import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import staged_output
from eml_attachment_remover.domain import (
    AppError,
    ExitCode,
    FileIdentity,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import (
    BoundDirectoryHandle,
    bind_destination,
    descriptor_identity,
)
from eml_attachment_remover.staged_output import (
    PublishedWithError,
    _PublicationState,
)

if TYPE_CHECKING:
    from pathlib import Path


def _state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    destination = bind_destination(str(tmp_path / "output.eml"))
    return staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct state-machine fault contract.
        destination, candidate, hashlib.sha256(candidate).hexdigest()
    )


def _bound_state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    state = _state(tmp_path, candidate)
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - direct state-machine fault contract.
    staged_output._create_stage(state)  # ruff: ignore[private-member-access] - direct state-machine fault contract.
    staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - direct state-machine fault contract.
    return state


def test_stage_creation_owns_every_construction_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    unbound = _state(tmp_path)
    with pytest.raises(AppError) as raised:
        staged_output._create_stage(unbound)  # ruff: ignore[private-member-access] - unbound-stage contract.
    assert raised.value.code is ExitCode.INTERNAL_ERROR
    state = _state(tmp_path)
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - construction-fault setup.

    def interrupt_owner(_fd: int, _name: bytes | str) -> object:
        raise KeyboardInterrupt

    def vanished_stage(*_args: object) -> None:
        raise FileNotFoundError

    with monkeypatch.context() as context:
        context.setattr(staged_output, "_Stage", interrupt_owner)
        context.setattr(staged_output, "discard_private_stage", vanished_stage)
        with pytest.raises(KeyboardInterrupt):
            staged_output._create_stage(state)  # ruff: ignore[private-member-access] - constructor-interrupt contract.
    for residual in tmp_path.glob(".eml-remove-*.tmp"):
        residual.unlink()
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_close_and_verification_faults_are_not_silently_accepted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    with monkeypatch.context() as context:
        context.setattr(os, "close", lambda _fd: (_ for _ in ()).throw(SystemExit()))
        assert isinstance(staged_output._close_descriptor(1), SystemExit)  # ruff: ignore[private-member-access] - descriptor-close fault contract.
    with monkeypatch.context() as context:
        context.setattr(
            staged_output,
            "close_bound_directory",
            lambda _directory: (_ for _ in ()).throw(KeyboardInterrupt()),
        )
        assert isinstance(
            staged_output._close_directory(BoundDirectoryHandle(1, windows=False)),  # ruff: ignore[private-member-access] - directory-close fault contract.
            KeyboardInterrupt,
        )
    state = _bound_state(tmp_path)
    with monkeypatch.context() as context:
        context.setattr(staged_output, "_read_all", lambda _fd: b"different")
        with pytest.raises(AppError) as raised:
            staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - staged-reread mismatch contract.
        assert raised.value.code is ExitCode.VERIFICATION_ERROR
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_final_receipt_detects_handle_digest_and_entry_replacement(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    state = _bound_state(tmp_path)
    stage = state.stage
    parent = state.parent
    assert stage is not None
    assert parent is not None
    expected = descriptor_identity(stage.descriptor)
    for first, second, digest in [
        (FileIdentity(0, 0, "-", 0), expected, state.digest),
        (expected, expected, "wrong"),
    ]:
        with monkeypatch.context() as context:
            context.setattr(
                staged_output, "child_lstat", lambda *_args, first=first: first
            )
            context.setattr(
                staged_output,
                "open_child_nofollow",
                lambda *_args: os.dup(stage.descriptor),
            )
            context.setattr(
                staged_output, "descriptor_identity", lambda _fd, second=second: second
            )
            state.digest = digest
            os.lseek(stage.descriptor, 0, os.SEEK_SET)
            with pytest.raises(AppError):
                staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - final-address mismatch contract.
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_windows_cleanup_link_and_finish_edge_states(tmp_path: Path) -> None:
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(1, windows=True)
    state.stage = staged_output._Stage(2, "private")  # ruff: ignore[private-member-access] - Windows delete-on-close contract.
    state.kernel_published = True
    assert staged_output._remove_stage_entry(state) is None  # ruff: ignore[private-member-access] - renamed Windows stage contract.
    assert state.stage.name is None
    receipt = PublicationReceipt(
        visibility="visible",
        identity=None,
        digest=None,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=None,
        temp_cleanup="pending",
    )
    state.receipt = receipt
    with pytest.raises(PublishedWithError):
        staged_output._finish_or_raise(state, None, ("failed", OSError("close")))  # ruff: ignore[private-member-access] - post-edge cleanup contract.
    pre_edge = _state(tmp_path)
    with pytest.raises(AppError):
        staged_output._finish_or_raise(pre_edge, None, ("succeeded", None))  # ruff: ignore[private-member-access] - impossible-pre-edge contract.
    with pytest.raises(KeyboardInterrupt):
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - primary-cause contract.
            pre_edge, KeyboardInterrupt(), ("succeeded", None)
        )


def test_missing_private_stage_is_recorded_as_already_removed(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    state = _bound_state(tmp_path)
    assert state.parent is not None
    state.stage = staged_output._Stage(2, "private")  # ruff: ignore[private-member-access] - exact stage-entry cleanup contract.

    def absent(*_arguments: object) -> None:
        raise FileNotFoundError

    monkeypatch.setattr(staged_output, "discard_private_stage", absent)
    assert staged_output._remove_stage_entry(state) is None  # ruff: ignore[private-member-access] - exact stage-entry cleanup contract.
    assert state.stage.name is None


def test_constructor_and_receipt_dual_failures_are_preserved(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    state = _state(tmp_path)
    staged_output._bind_parent(state)  # ruff: ignore[private-member-access] - constructor-fault setup.

    def fail_owner(_fd: int, _name: bytes | str) -> object:
        raise KeyboardInterrupt

    discard_message = "discard"

    def fail_discard(*_args: object) -> None:
        raise OSError(discard_message)

    original_close = staged_output._close_descriptor  # ruff: ignore[private-member-access] - direct close fault injection.

    def fail_close(descriptor: int) -> BaseException | None:
        actual = original_close(descriptor)
        assert actual is None
        return OSError("close")

    with monkeypatch.context() as context:
        context.setattr(staged_output, "_Stage", fail_owner)
        context.setattr(staged_output, "discard_private_stage", fail_discard)
        context.setattr(staged_output, "_close_descriptor", fail_close)
        with pytest.raises(BaseExceptionGroup):
            staged_output._create_stage(state)  # ruff: ignore[private-member-access] - owner/cleanup dual-failure contract.
    for residual in tmp_path.glob(".eml-remove-*.tmp"):
        residual.unlink()
    staged_output._cleanup(state)  # ruff: ignore[private-member-access] - explicit test cleanup.
    receipt_state = _bound_state(tmp_path)
    stage = receipt_state.stage
    parent = receipt_state.parent
    assert stage is not None
    assert parent is not None
    expected = descriptor_identity(stage.descriptor)
    receipt_state.digest = hashlib.sha256(b"candidate").hexdigest()

    def expected_entry(*_args: object) -> FileIdentity:
        return expected

    with monkeypatch.context() as context:
        context.setattr(staged_output, "child_lstat", expected_entry)
        context.setattr(
            staged_output,
            "open_child_nofollow",
            lambda *_args: os.dup(stage.descriptor),
        )
        calls = 0

        def changing_identity(_fd: int) -> FileIdentity:
            nonlocal calls
            calls += 1
            return expected if calls == 1 else FileIdentity(0, 0, "-", 0)

        context.setattr(staged_output, "descriptor_identity", changing_identity)
        with pytest.raises(AppError):
            staged_output._read_final_receipt(receipt_state)  # ruff: ignore[private-member-access] - final-handle identity contract.
    staged_output._cleanup(receipt_state)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_remaining_stage_cleanup_and_terminal_branches(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(1, windows=False)
    state.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct discard fault contract.
    with monkeypatch.context() as context:
        context.setattr(
            staged_output,
            "discard_private_stage",
            lambda *_args: (_ for _ in ()).throw(OSError("discard")),
        )
        error = staged_output._remove_stage_entry(state)  # ruff: ignore[private-member-access] - discard fault contract.
    assert isinstance(error, OSError)
    state.stage.name = None
    with pytest.raises(AppError):
        staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - unavailable stage-name contract.
    receipt = PublicationReceipt(
        visibility="visible",
        identity=None,
        digest=None,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=None,
        temp_cleanup="pending",
    )
    terminal = _state(tmp_path)
    terminal.kernel_published = True
    with monkeypatch.context() as context:
        context.setattr(staged_output, "_reconcile", lambda *_args: receipt)
        result = staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - missing-receipt reconciliation contract.
            terminal, None, ("succeeded", None)
        )
    assert result.visibility == "visible"
    assert result.temp_cleanup == "succeeded"
    pre_edge = _state(tmp_path)
    with pytest.raises(OSError, match="cleanup"):
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - cleanup-only pre-edge contract.
            pre_edge, None, ("failed", OSError("cleanup"))
        )


def test_receipt_close_and_publish_edge_faults_are_truthful(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    mode_case = tmp_path / "mode"
    mode_case.mkdir()
    mode_state = _state(mode_case)
    staged_output._bind_parent(mode_state)  # ruff: ignore[private-member-access] - Windows mode setup.
    staged_output._create_stage(mode_state)  # ruff: ignore[private-member-access] - Windows mode setup.
    mode_parent = mode_state.parent
    assert mode_parent is not None
    mode_parent.windows = True
    staged_output._verify_staged(mode_state)  # ruff: ignore[private-member-access] - Windows ACL-mode branch contract.
    mode_parent.windows = False
    staged_output._cleanup(mode_state)  # ruff: ignore[private-member-access] - explicit test cleanup.
    mismatch_case = tmp_path / "mismatch"
    mismatch_case.mkdir()
    mismatch = _bound_state(mismatch_case)
    stage = mismatch.stage
    parent = mismatch.parent
    assert stage is not None
    assert parent is not None
    assert isinstance(stage.name, bytes)
    assert isinstance(mismatch.destination.basename, bytes)
    os.link(
        stage.name,
        mismatch.destination.basename,
        src_dir_fd=parent.descriptor,
        dst_dir_fd=parent.descriptor,
    )
    expected = descriptor_identity(stage.descriptor)
    changed = FileIdentity(0, 0, "-", 0)
    calls = 0

    def changed_final_entry(*_args: object) -> FileIdentity:
        nonlocal calls
        calls += 1
        return expected if calls == 1 else changed

    with monkeypatch.context() as context:
        context.setattr(staged_output, "child_lstat", changed_final_entry)
        with pytest.raises(AppError):
            staged_output._read_final_receipt(mismatch)  # ruff: ignore[private-member-access] - final-entry replacement contract.
    staged_output._cleanup(mismatch)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_link_cleanup_failure_after_visible_edge(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    edge_case = tmp_path / "edge"
    edge_case.mkdir()
    edge = _bound_state(edge_case)
    edge_stage = edge.stage
    edge_parent = edge.parent
    assert edge_stage is not None
    assert edge_parent is not None
    assert isinstance(edge_stage.name, bytes)
    assert isinstance(edge.destination.basename, bytes)

    def portable_link(
        directory: BoundDirectoryHandle,
        _descriptor: int,
        stage_name: bytes | str,
        destination_name: bytes | str,
    ) -> bool:
        assert isinstance(stage_name, bytes)
        assert isinstance(destination_name, bytes)
        os.link(
            stage_name,
            destination_name,
            src_dir_fd=directory.descriptor,
            dst_dir_fd=directory.descriptor,
        )
        return True

    with monkeypatch.context() as context:
        context.setattr(staged_output, "publish_stage_no_replace", portable_link)
        context.setattr(
            staged_output, "_remove_stage_entry", lambda _state: OSError("unlink")
        )
        with pytest.raises(OSError, match="unlink"):
            staged_output._publish_edge(edge)  # ruff: ignore[private-member-access] - link-cleanup failure contract.
    staged_output._cleanup(edge)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_final_receipt_and_unproven_publish_errors_are_preserved(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    dual_case = tmp_path / "dual"
    dual_case.mkdir()
    dual = _bound_state(dual_case)
    with monkeypatch.context() as context:
        context.setattr(
            staged_output, "_close_descriptor", lambda _fd: OSError("close")
        )
        with pytest.raises(BaseExceptionGroup):
            staged_output._read_final_receipt(dual)  # ruff: ignore[private-member-access] - primary plus final-close failure contract.
    staged_output._cleanup(dual)  # ruff: ignore[private-member-access] - explicit test cleanup.
    clean_case = tmp_path / "clean"
    clean_case.mkdir()
    clean = _bound_state(clean_case)
    stage = clean.stage
    parent = clean.parent
    assert stage is not None
    assert parent is not None
    assert isinstance(stage.name, bytes)
    assert isinstance(clean.destination.basename, bytes)
    os.link(
        stage.name,
        clean.destination.basename,
        src_dir_fd=parent.descriptor,
        dst_dir_fd=parent.descriptor,
    )
    original_close = staged_output._close_descriptor  # ruff: ignore[private-member-access] - final-close fault injection.

    def close_after_actual(descriptor: int) -> BaseException | None:
        actual = original_close(descriptor)
        assert actual is None
        return OSError("close")

    with monkeypatch.context() as context:
        context.setattr(staged_output, "_close_descriptor", close_after_actual)
        with pytest.raises(OSError, match="close"):
            staged_output._read_final_receipt(clean)  # ruff: ignore[private-member-access] - final-close-only failure contract.
    staged_output._cleanup(clean)  # ruff: ignore[private-member-access] - explicit test cleanup.


def test_unproven_post_edge_receipt_is_an_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    unproven_case = tmp_path / "unproven"
    unproven_case.mkdir()
    unproven = _bound_state(unproven_case)
    stage = unproven.stage
    parent = unproven.parent
    assert stage is not None
    assert parent is not None
    assert isinstance(stage.name, bytes)
    assert isinstance(unproven.destination.basename, bytes)

    def link_without_replacement(
        directory: BoundDirectoryHandle,
        _descriptor: int,
        stage_name: bytes | str,
        destination_name: bytes | str,
    ) -> bool:
        assert isinstance(stage_name, bytes)
        assert isinstance(destination_name, bytes)
        os.link(
            stage_name,
            destination_name,
            src_dir_fd=directory.descriptor,
            dst_dir_fd=directory.descriptor,
        )
        return False

    receipt = PublicationReceipt(
        visibility="not_proven",
        identity=None,
        digest=None,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=False,
        final_address=None,
        temp_cleanup="pending",
    )
    with monkeypatch.context() as context:
        context.setattr(
            staged_output, "publish_stage_no_replace", link_without_replacement
        )
        context.setattr(staged_output, "_reconcile", lambda *_args: receipt)
        with pytest.raises(AppError):
            staged_output._publish_edge(unproven)  # ruff: ignore[private-member-access] - unproven post-edge receipt contract.
    staged_output._cleanup(unproven)  # ruff: ignore[private-member-access] - explicit test cleanup.
