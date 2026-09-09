"""Additional exact contracts for the v3 staged-publication state machine."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

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
    _PublicationState,  # ruff: ignore[import-private-name] - direct state-machine contract.
)

if TYPE_CHECKING:
    from pathlib import Path


def _state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    """Build an unbound lifecycle state with stable candidate evidence.

    Returns:
        A state suitable for direct publication-transition tests.

    """
    return _PublicationState(
        bind_destination(str(tmp_path / "output.eml")),
        candidate,
        hashlib.sha256(candidate).hexdigest(),
    )


def _receipt(
    state: _PublicationState, *, directory_sync: str = "succeeded"
) -> PublicationReceipt:
    """Build the one complete visible receipt for a direct state test.

    Returns:
        A receipt whose address and digest are bound to ``state``.

    """
    return PublicationReceipt(
        visibility="visible",
        identity=FileIdentity(1, 2, "regular", 3),
        digest=state.digest,
        file_sync="succeeded",
        directory_sync=directory_sync,
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="pending",
    )


def test_final_receipt_rejects_each_missing_live_owner(tmp_path: Path) -> None:
    """Final-address verification needs both the stage handle and parent binding."""
    absent_stage = _state(tmp_path)
    absent_stage.parent = BoundDirectoryHandle(1, windows=False)
    absent_parent = _state(tmp_path)
    absent_parent.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct owner state.
    for state in (absent_stage, absent_parent):
        with pytest.raises(AppError) as raised:
            staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - direct receipt-owner contract.
        assert raised.value == AppError(
            ExitCode.INTERNAL_ERROR, "staging file was not created"
        )


def test_final_receipt_rejects_a_nonregular_final_entry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An identity match alone cannot substitute for an ordinary final file."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(1, windows=False)
    state.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct receipt state.
    nonregular = FileIdentity(1, 2, "directory", 3)
    closed: list[int] = []

    def close(descriptor: int) -> None:
        closed.append(descriptor)

    monkeypatch.setattr(staged_output, "descriptor_identity", lambda _fd: nonregular)
    monkeypatch.setattr(staged_output, "child_lstat", lambda *_args: nonregular)
    monkeypatch.setattr(staged_output, "_close_descriptor", close)
    with pytest.raises(AppError) as raised:
        staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - final regular-file invariant.
    assert raised.value == AppError(
        ExitCode.WRITE_ERROR, "published destination identity mismatch"
    )
    assert closed == [-1]


def test_final_receipt_requires_every_identity_and_digest_observation(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Each address, handle, reread, and post-reread observation is independent."""
    expected = FileIdentity(1, 2, "regular", 3)
    changed = FileIdentity(4, 5, "regular", 6)
    cases = (
        (
            "initial-entry",
            (changed,),
            (expected,),
            b"candidate",
            ExitCode.WRITE_ERROR,
            "published destination identity mismatch",
            [-1],
        ),
        (
            "final-handle",
            (expected,),
            (expected, changed),
            b"candidate",
            ExitCode.WRITE_ERROR,
            "final destination handle changed",
            [13],
        ),
        (
            "post-read-handle",
            (expected,),
            (expected, expected, changed),
            b"candidate",
            ExitCode.VERIFICATION_ERROR,
            "final destination changed",
            [13],
        ),
        (
            "digest",
            (expected,),
            (expected, expected, expected),
            b"changed",
            ExitCode.VERIFICATION_ERROR,
            "final destination changed",
            [13],
        ),
        (
            "post-read-entry",
            (expected, changed),
            (expected, expected, expected),
            b"candidate",
            ExitCode.WRITE_ERROR,
            "final destination entry changed",
            [13],
        ),
    )
    for (
        _label,
        lstat_values,
        identity_values,
        body,
        code,
        message,
        expected_close,
    ) in cases:
        state = _state(tmp_path)
        state.parent = BoundDirectoryHandle(1, windows=False)
        state.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct receipt state.
        lstat_results = iter(lstat_values)
        identity_results = iter(identity_values)
        closed: list[int] = []
        with monkeypatch.context() as context:
            context.setattr(
                staged_output,
                "child_lstat",
                lambda *_args, results=lstat_results: next(results),
            )
            context.setattr(
                staged_output,
                "descriptor_identity",
                lambda _fd, results=identity_results: next(results),
            )
            context.setattr(staged_output, "open_child_nofollow", lambda *_args: 13)
            context.setattr(staged_output, "_read_all", lambda _fd, body=body: body)
            context.setattr(
                staged_output,
                "_close_descriptor",
                lambda fd, closed=closed: closed.append(fd),
            )
            with pytest.raises(AppError) as raised:
                staged_output._read_final_receipt(state)  # ruff: ignore[private-member-access] - independent final-address evidence.
        assert raised.value == AppError(code, message)
        assert closed == expected_close


def test_publish_edge_requires_each_exact_staging_owner(tmp_path: Path) -> None:
    """The visibility edge must reject every incomplete ownership configuration."""
    absent_stage = _state(tmp_path)
    absent_stage.parent = BoundDirectoryHandle(1, windows=False)
    absent_parent = _state(tmp_path)
    absent_parent.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct edge state.
    absent_name = _state(tmp_path)
    absent_name.parent = BoundDirectoryHandle(1, windows=False)
    absent_name.stage = staged_output._Stage(2, None)  # ruff: ignore[private-member-access] - direct edge state.
    for state, expected in (
        (
            absent_stage,
            AppError(ExitCode.INTERNAL_ERROR, "staging file was not created"),
        ),
        (
            absent_parent,
            AppError(ExitCode.INTERNAL_ERROR, "staging file was not created"),
        ),
        (
            absent_name,
            AppError(ExitCode.INTERNAL_ERROR, "staging name is unavailable"),
        ),
    ):
        with pytest.raises(AppError) as raised:
            staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - exact visibility-owner contract.
        assert raised.value == expected


def test_publish_edge_records_visibility_before_a_receipt_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Any post-call failure still records that the candidate became visible."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(1, windows=False)
    state.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct edge state.
    calls: list[tuple[str, object]] = []

    def publish(*arguments: object) -> bool:
        calls.append(("publish", arguments))
        return False

    message = "readdress"

    def fail_receipt(observed: _PublicationState) -> PublicationReceipt:
        calls.append(("receipt", observed.kernel_published))
        raise OSError(message)

    monkeypatch.setattr(staged_output, "publish_stage_no_replace", publish)
    monkeypatch.setattr(staged_output, "_read_final_receipt", fail_receipt)
    monkeypatch.setattr(
        staged_output,
        "sync_bound_directory",
        lambda *_args: (_ for _ in ()).throw(AssertionError("sync after failure")),
    )
    with pytest.raises(OSError, match="readdress"):
        staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - nonterminal visibility boundary.
    assert state.kernel_published is True
    assert calls == [
        (
            "publish",
            (state.parent, 2, b"private", state.destination.basename),
        ),
        ("receipt", True),
    ]


def test_publish_edge_uses_one_sync_when_the_native_rename_consumes_stage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A rename-style publication has no residual private link to delete or resync."""
    state = _state(tmp_path)
    state.parent = BoundDirectoryHandle(1, windows=False)
    state.stage = staged_output._Stage(2, b"private")  # ruff: ignore[private-member-access] - direct edge state.
    final = _receipt(state, directory_sync="unsupported")
    calls: list[tuple[str, object]] = []

    def publish(*_arguments: object) -> bool:
        calls.append(("publish", state.kernel_published))
        return False

    def read_receipt(observed: _PublicationState) -> PublicationReceipt:
        calls.append(("receipt", observed.kernel_published))
        return final

    def sync(parent: BoundDirectoryHandle) -> str:
        calls.append(("sync", parent))
        return "unsupported"

    def reconcile(
        observed: _PublicationState, directory_sync: str
    ) -> PublicationReceipt:
        calls.append(("reconcile", (observed, directory_sync)))
        return final

    monkeypatch.setattr(staged_output, "publish_stage_no_replace", publish)
    monkeypatch.setattr(staged_output, "_read_final_receipt", read_receipt)
    monkeypatch.setattr(staged_output, "sync_bound_directory", sync)
    monkeypatch.setattr(staged_output, "_reconcile", reconcile)
    monkeypatch.setattr(
        staged_output,
        "_remove_stage_entry",
        lambda *_args: (_ for _ in ()).throw(AssertionError("no residual link")),
    )
    staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - direct rename publication contract.
    assert state.receipt is final
    assert calls == [
        ("publish", False),
        ("receipt", True),
        ("sync", state.parent),
        ("reconcile", (state, "unsupported")),
    ]


def test_finish_returns_a_cleanup_updated_copy_of_a_complete_receipt(
    tmp_path: Path,
) -> None:
    """Accepted post-edge publication updates only the returned cleanup fact."""
    state = _state(tmp_path)
    state.kernel_published = True
    state.receipt = _receipt(state)
    result = staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - terminal receipt contract.
        state, None, ("succeeded", None)
    )
    assert result == PublicationReceipt(
        visibility="visible",
        identity=state.receipt.identity,
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="succeeded",
    )
    assert state.receipt.temp_cleanup == "pending"


def test_finish_preserves_primary_and_cleanup_causes_after_visibility(
    tmp_path: Path,
) -> None:
    """A visible candidate carries both post-edge causes in one truthful error."""
    state = _state(tmp_path)
    state.kernel_published = True
    state.receipt = _receipt(state)
    primary = AppError(ExitCode.WRITE_ERROR, "sync")
    cleanup = OSError("close")
    with pytest.raises(PublishedWithError) as raised:
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - terminal dual-cause contract.
            state, primary, ("failed", cleanup)
        )
    assert raised.value.cause is primary
    assert raised.value.cleanup_cause is cleanup
    assert raised.value.receipt.temp_cleanup == "failed"
    assert state.receipt.temp_cleanup == "pending"


def test_finish_reconciles_a_missing_post_edge_receipt_with_failed_sync(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A terminal edge without a cached receipt must re-address it as failed sync."""
    state = _state(tmp_path)
    state.kernel_published = True
    reconciled = _receipt(state, directory_sync="failed")
    calls: list[tuple[_PublicationState, str]] = []

    def reconcile(
        observed: _PublicationState, directory_sync: str
    ) -> PublicationReceipt:
        calls.append((observed, directory_sync))
        return reconciled

    monkeypatch.setattr(staged_output, "_reconcile", reconcile)
    result = staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - missing terminal receipt contract.
        state, None, ("succeeded", None)
    )
    assert calls == [(state, "failed")]
    assert result == PublicationReceipt(
        visibility="visible",
        identity=reconciled.identity,
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="failed",
        address_verified=True,
        final_address=state.destination.request,
        temp_cleanup="succeeded",
    )


def test_finish_preserves_both_pre_edge_causes_in_the_original_order(
    tmp_path: Path,
) -> None:
    """Before visibility, primary failure remains first in the combined group."""
    state = _state(tmp_path)
    primary = AppError(ExitCode.WRITE_ERROR, "write")
    cleanup = OSError("cleanup")
    with pytest.raises(BaseExceptionGroup) as raised:
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - pre-edge dual-cause contract.
            state, primary, ("failed", cleanup)
        )
    assert raised.value.message == "multiple staging cleanup failures"
    assert raised.value.exceptions == (primary, cleanup)
