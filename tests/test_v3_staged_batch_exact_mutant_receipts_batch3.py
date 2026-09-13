"""Exact mutation receipts for staging edges and batch candidate control."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch, staged_output
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    PublicationReceipt,
    Removal,
)
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.native_paths import (
    bind_destination,
    path_value,
    read_source,
)
from eml_attachment_remover.staged_output import PublishedWithError

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.mime_execution import Candidate
    from eml_attachment_remover.mime_policy import PolicyResult
    from eml_attachment_remover.mime_raw import RawMimeTree, RawNode
    from eml_attachment_remover.staged_output import _PublicationState


def _options() -> BatchOptions:
    """Build stable direct-execution options.

    Returns:
        The standard nonpublishing batch option bundle.

    """
    return BatchOptions(
        dry_run=True,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )


def _state(tmp_path: Path, candidate: bytes = b"candidate") -> _PublicationState:
    """Build direct publication state with a concrete bound destination.

    Returns:
        An unstarted staging lifecycle state.

    """
    return staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct lifecycle receipt.
        bind_destination(str(tmp_path / "output.eml")),
        candidate,
        hashlib.sha256(candidate).hexdigest(),
    )


def _source(path: Path) -> Path:
    """Create one MIME body whose candidate phases can be observed.

    Returns:
        The written source path.

    """
    path.write_bytes(
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nkeep\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremove\r\n--m--\r\n"
    )
    return path


def test_staging_preconditions_and_terminal_edge_have_exact_public_errors(
    tmp_path: Path,
) -> None:
    """Private-stage and terminal-edge failures preserve their exact public receipts."""
    state = _state(tmp_path)
    with pytest.raises(AppError) as unbound:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - unbound parent boundary.
    assert unbound.value == AppError(
        ExitCode.INTERNAL_ERROR, "destination directory was not bound"
    )

    with pytest.raises(AppError) as missing_stage:
        staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - missing stage boundary.
    assert missing_stage.value == AppError(
        ExitCode.INTERNAL_ERROR, "staging file was not created"
    )

    with pytest.raises(AppError) as before_edge:
        staged_output._finish_or_raise(  # ruff: ignore[private-member-access] - terminal lifecycle receipt.
            state, None, ("succeeded", None)
        )
    assert before_edge.value == AppError(
        ExitCode.INTERNAL_ERROR, "publication ended before its visibility edge"
    )


def test_staged_reread_mismatch_and_unproven_publication_are_exact(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The stage verification and final re-address messages are stable receipts."""
    state = _state(tmp_path, b"candidate")
    state.parent = BoundDirectory(41, windows=False)
    state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - direct private-stage owner.
    monkeypatch.setattr(staged_output.__dict__["os"], "write", lambda *_args: 9)
    monkeypatch.setattr(staged_output.__dict__["os"], "fsync", lambda _fd: None)
    monkeypatch.setattr(staged_output.__dict__["os"], "fchmod", lambda *_args: None)
    monkeypatch.setattr(staged_output.__dict__["os"], "lseek", lambda *_args: 0)
    monkeypatch.setattr(staged_output, "_read_all", lambda _fd: b"changed")

    with pytest.raises(AppError) as mismatch:
        staged_output._verify_staged(state)  # ruff: ignore[private-member-access] - staged-byte reread receipt.
    assert mismatch.value == AppError(
        ExitCode.VERIFICATION_ERROR, "staged candidate bytes do not match"
    )

    state = _state(tmp_path)
    state.parent = BoundDirectory(41, windows=False)
    state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - direct visibility edge owner.
    unproven = PublicationReceipt(
        visibility="not_proven",
        identity=None,
        digest=state.digest,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=False,
        final_address=None,
        temp_cleanup="pending",
    )
    monkeypatch.setattr(staged_output, "publish_stage_no_replace", lambda *_args: False)
    monkeypatch.setattr(staged_output, "_read_final_receipt", lambda _state: unproven)
    monkeypatch.setattr(
        staged_output, "sync_bound_directory", lambda _parent: "succeeded"
    )
    monkeypatch.setattr(staged_output, "_reconcile", lambda *_args: unproven)

    with pytest.raises(AppError) as unpublished:
        staged_output._publish_edge(state)  # ruff: ignore[private-member-access] - unproven visibility receipt.
    assert unpublished.value == AppError(
        ExitCode.WRITE_ERROR, "could not re-address published candidate"
    )
    assert state.kernel_published
    assert state.receipt is unproven


def test_publish_reconciles_only_a_post_edge_missing_receipt_with_failed_sync(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A publication-edge exception triggers exactly one failed-sync reconciliation."""
    destination = bind_destination(str(tmp_path / "output.eml"))
    candidate = b"candidate"
    failure = RuntimeError("edge receipt failed")
    calls: list[str] = []
    receipt = PublicationReceipt(
        visibility="not_proven",
        identity=None,
        digest=hashlib.sha256(candidate).hexdigest(),
        file_sync="succeeded",
        directory_sync="failed",
        address_verified=False,
        final_address=None,
        temp_cleanup="pending",
    )

    def bind(state: _PublicationState) -> None:
        state.parent = BoundDirectory(41, windows=False)

    def create(state: _PublicationState) -> None:
        state.stage = staged_output._Stage(51, b"stage")  # ruff: ignore[private-member-access] - artificial post-edge owner.

    def edge(state: _PublicationState) -> None:
        state.kernel_published = True
        raise failure

    def reconcile(_state: _PublicationState, directory_sync: str) -> PublicationReceipt:
        calls.append(directory_sync)
        return receipt

    monkeypatch.setattr(staged_output, "_bind_parent", bind)
    monkeypatch.setattr(staged_output, "_create_stage", create)
    monkeypatch.setattr(staged_output, "_verify_staged", lambda _state: None)
    monkeypatch.setattr(staged_output, "_publish_edge", edge)
    monkeypatch.setattr(staged_output, "_reconcile", reconcile)
    monkeypatch.setattr(staged_output, "_cleanup", lambda _state: ("succeeded", None))

    with pytest.raises(PublishedWithError) as raised:
        staged_output.publish(destination, candidate)
    assert raised.value.cause is failure
    assert raised.value.receipt.directory_sync == "failed"
    assert calls == ["failed"]


def test_publish_does_not_reconcile_a_preedge_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An exception before publication cannot be treated as a visible receipt."""
    destination = bind_destination(str(tmp_path / "output.eml"))
    failure = AppError(ExitCode.WRITE_ERROR, "stage unavailable")
    calls: list[str] = []

    monkeypatch.setattr(
        staged_output,
        "_bind_parent",
        lambda state: setattr(state, "parent", BoundDirectory(41, windows=False)),
    )
    monkeypatch.setattr(
        staged_output,
        "_create_stage",
        lambda _state: (_ for _ in ()).throw(failure),
    )
    monkeypatch.setattr(
        staged_output,
        "_reconcile",
        lambda *_args: calls.append("reconcile"),
    )
    monkeypatch.setattr(staged_output, "_cleanup", lambda _state: ("succeeded", None))

    with pytest.raises(AppError) as raised:
        staged_output.publish(destination, b"candidate")
    assert raised.value is failure
    assert calls == []


def test_candidate_preconditions_and_fingerprint_mismatch_have_exact_messages(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Source address, identity, and fingerprints retain distinct receipts."""
    missing = BatchLedger.from_requests([path_value("source.eml")]).items[0]
    missing.source_request = path_value("source.eml")
    missing.source_request = type(missing.source_request)(None, "<none>", None)
    identity = FileIdentity(1, 2, "regular", 3)
    with pytest.raises(AppError) as no_address:
        batch._candidate(missing, identity)  # ruff: ignore[private-member-access] - native-address prerequisite.
    assert no_address.value == AppError(
        ExitCode.INPUT_ERROR, "source has no native address"
    )

    source = _source(tmp_path / "message.eml")
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    with pytest.raises(AppError) as changed:
        batch._candidate(  # ruff: ignore[private-member-access] - inventory identity prerequisite.
            item, FileIdentity(0, 0, "regular", 0)
        )
    assert changed.value == AppError(
        ExitCode.INPUT_ERROR, "source changed after inventory"
    )

    snapshot = read_source(str(source))
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    monkeypatch.setattr(batch, "fingerprint_retained", lambda *_args: ())
    with pytest.raises(AppError) as mismatch:
        batch._candidate(item, snapshot.identity)  # ruff: ignore[private-member-access] - independent fingerprint receipt.
    assert mismatch.value == AppError(
        ExitCode.VERIFICATION_ERROR, "source fingerprint recomputation mismatch"
    )


def test_candidate_records_the_exact_phase_before_each_next_pipeline_step(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Binding, parsing, and classification phase receipts precede their consumers."""
    source = _source(tmp_path / "message.eml")
    item = BatchLedger.from_requests([path_value(str(source))]).items[0]
    original_parse = parse_raw_mime
    original_classify = classify
    original_build = build_candidate

    def parse(raw: bytes) -> RawMimeTree:
        assert item.phase is ItemPhase.BOUND
        return original_parse(raw)

    def classify_policy(root: RawNode) -> PolicyResult:
        assert item.phase is ItemPhase.PARSED
        return original_classify(root)

    def build(tree: RawMimeTree, removals: tuple[Removal, ...]) -> Candidate:
        assert item.phase is ItemPhase.CLASSIFIED
        return original_build(tree, removals)

    monkeypatch.setitem(batch.__dict__, "parse_raw_mime", parse)
    monkeypatch.setitem(batch.__dict__, "classify", classify_policy)
    monkeypatch.setitem(batch.__dict__, "build_candidate", build)
    batch._candidate(  # ruff: ignore[private-member-access] - ordered candidate pipeline phases.
        item, read_source(str(source)).identity
    )
    assert item.phase is ItemPhase.CANDIDATE


def test_execute_enforces_the_strict_native_argument_budget_with_exact_reasons(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The exact byte limit is permitted; only a larger request is declined."""
    calls: list[list[str]] = []

    def run(ledger: BatchLedger, sources: list[str], _options: BatchOptions) -> None:
        calls.append(sources)
        assert len(ledger.items) == 1

    monkeypatch.setattr(batch, "_run_inventory_and_items", run)
    monkeypatch.setattr(batch, "MAX_CUMULATIVE_NATIVE_ARGUMENT_BYTES", 1)
    accepted = batch.execute(["x"], _options())
    assert calls == [["x"]]
    assert accepted.items[0].error == AppError(
        ExitCode.BATCH_FAILURE, "not run", phase="batch"
    )

    monkeypatch.setattr(batch, "MAX_CUMULATIVE_NATIVE_ARGUMENT_BYTES", 0)
    rejected = batch.execute(["x"], _options())
    assert calls == [["x"]]
    assert rejected.items[0].error == AppError(
        ExitCode.BATCH_FAILURE,
        "batch exceeds native argument resource limit",
        phase="batch",
    )


def test_execute_records_outer_cancellation_with_the_exact_signal_and_phase(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Outer cancellation retains the inventoried signal receipt before finalization."""

    def cancel(
        _ledger: BatchLedger, _sources: list[str], _options: BatchOptions
    ) -> None:
        raise CancellationSignal(1, "SIGTERM")

    monkeypatch.setattr(batch, "_run_inventory_and_items", cancel)
    ledger = batch.execute(["one.eml"], _options())
    assert ledger.interruption is not None
    assert (ledger.interruption.signal, ledger.interruption.phase) == (
        "SIGTERM",
        "inventoried",
    )
    assert ledger.items[0].error == AppError(
        ExitCode.INTERRUPTED, "interrupted by SIGTERM", phase="inventoried"
    )


def test_execute_records_keyboard_interrupt_with_the_exact_inventoried_phase(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Keyboard interruption has a stable SIGINT receipt at the outer boundary."""

    def cancel(
        _ledger: BatchLedger, _sources: list[str], _options: BatchOptions
    ) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr(batch, "_run_inventory_and_items", cancel)
    ledger = batch.execute(["one.eml"], _options())
    assert ledger.interruption is not None
    assert (ledger.interruption.signal, ledger.interruption.phase) == (
        "SIGINT",
        "inventoried",
    )
    assert ledger.items[0].status is ItemStatus.NOT_RUN
    assert ledger.items[0].error == AppError(
        ExitCode.INTERRUPTED, "interrupted by SIGINT", phase="inventoried"
    )
