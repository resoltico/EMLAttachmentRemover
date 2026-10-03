"""Malformed wire data remains a typed failure across the full MIME pipeline."""

from __future__ import annotations

import hashlib
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

    from eml_attachment_remover.mime_raw import RawMimeTree

import pytest
from hypothesis import example, given
from hypothesis import strategies as st

from eml_attachment_remover import batch, batch_execution, report_stream
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    ExitCode,
    FileIdentity,
    ItemPhase,
    ItemStatus,
    LedgerItem,
    Removal,
    RemovalReason,
)
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_verification import verify_candidate
from eml_attachment_remover.native_values import path_value

SEEDS = (
    b"Subject: Public example\r\n\r\nBody\r\n",
    b"Content-Type: text/plain; charset=utf-8\r\n\r\nBody\r\n",
    b"Content-Type: text/plain; (escaped\\\\)\r\n\r\nBody\r\n",
    (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nKeep\r\n"
        b"--m\r\nContent-Disposition: attachment\r\n\r\nRemove\r\n--m--\r\n"
    ),
    (
        b"Content-Type: multipart/alternative; boundary=a\r\n\r\n"
        b"--a\r\nContent-Type: text/plain\r\n\r\nPlain\r\n"
        b"--a\r\nContent-Type: text/html\r\n\r\n<p>HTML</p>\r\n--a--\r\n"
    ),
    (
        b"Content-Type: multipart/related; boundary=r\r\n\r\n"
        b"--r\r\nContent-Type: text/html\r\n\r\n<p>HTML</p>\r\n"
        b"--r\r\nContent-Type: image/png\r\n\r\nBytes\r\n--r--\r\n"
    ),
    b"Content-Transfer-Encoding: base64\r\n\r\nQm9keQ==\r\n",
)


@st.composite
def mutated_messages(draw: st.DrawFn) -> bytes:
    """Splice arbitrary bytes and another message into a bounded wire sample.

    Returns:
        A synthetic malformed or retained-valid message.

    """
    seed = draw(st.sampled_from(SEEDS))
    start = draw(st.integers(0, len(seed)))
    end = draw(st.integers(start, len(seed)))
    inserted = draw(st.one_of(st.binary(max_size=256), st.sampled_from(SEEDS)))
    return seed[:start] + inserted + seed[end:]


@given(st.one_of(st.binary(max_size=8192), mutated_messages()))
@example(b"Content-Type: text/plain; (abc\\\\\r\n\r\nBody")
@example(b"Content-Type: text/plain; (abc\\)\r\n\r\nBody")
def test_arbitrary_wire_data_has_typed_admission_and_verifiable_accepted_plans(  # type: ignore[misc]
    raw: bytes,
) -> None:
    """Rejections are expected errors; admitted plans must actually verify."""
    try:
        source = parse_raw_mime(raw)
        plan = classify(source.root)
        candidate = build_candidate(source, plan.removals)
        receipt, fingerprints = verify_candidate(source, candidate, plan.removals)
    except AppError as error:
        if error.code not in {
            ExitCode.PARSE_ERROR,
            ExitCode.TRANSFORMATION_UNAVAILABLE,
        }:
            raise
        return
    assert receipt.retained_payloads_match
    assert receipt.structure_matches
    assert receipt.policy_is_idempotent
    assert receipt.digest_matches
    assert candidate.digest == hashlib.sha256(candidate.raw).hexdigest()
    assert fingerprints or not candidate.raw


@pytest.mark.parametrize("tail", [b"\\\\", b"\\)", b"\\x", b"\\\r\n \t"])
def test_unfinished_comments_after_complete_quoted_pairs_are_parse_errors(
    tail: bytes,
) -> None:
    raw = b"Content-Type: text/plain; (abc" + tail + b"\r\n\r\nBody"
    with pytest.raises(AppError, match="unterminated MIME comment") as rejected:
        parse_raw_mime(raw)
    assert rejected.value.code is ExitCode.PARSE_ERROR


@pytest.mark.parametrize(
    "field",
    [
        b"Content-Type: text/plain; charset=utf-8;",
        b"Content-Type: text/plain;",
        b"Content-Type: text/plain; charset=utf-8; \t",
        b"Content-Disposition: inline;",
    ],
)
def test_one_terminal_parameter_separator_preserves_wire_bytes(field: bytes) -> None:
    raw = field + b"\r\n\r\nBody"
    source = parse_raw_mime(raw)
    plan = classify(source.root)
    candidate = build_candidate(source, plan.removals)
    assert candidate.raw == raw
    if b"charset=" in field:
        assert source.root.content_type.parameters[b"charset"] == b"utf-8"
    verify_candidate(source, candidate, plan.removals)


@pytest.mark.parametrize(
    "parameters", [b";;", b"; ;", b"; ; charset=utf-8", b"; charset=utf-8;;"]
)
def test_empty_nonterminal_parameters_remain_refused(parameters: bytes) -> None:
    with pytest.raises(AppError):
        parse_raw_mime(b"Content-Type: text/plain" + parameters + b"\r\n\r\nBody")


def test_source_authorization_refuses_forged_body_deletion() -> None:
    source = parse_raw_mime(SEEDS[4])
    forged = (Removal((1,), "text/html", RemovalReason.EXPLICIT_ATTACHMENT),)
    candidate = build_candidate(source, forged)
    assert parse_raw_mime(candidate.raw).root.children
    with pytest.raises(AppError, match="authorization mismatch"):
        verify_candidate(source, candidate, forged)


@pytest.mark.parametrize(
    "problem", [ValueError("synthetic parser fault"), ValueError()]
)
def test_preparation_exception_is_item_local_and_later_files_still_publish(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    problem: ValueError,
) -> None:
    """A parser fault cannot abort later inputs or turn failed bytes into a copy."""
    sources = [tmp_path / name for name in ["fault.eml", "second.eml", "third.eml"]]
    for source in sources:
        source.write_bytes(SEEDS[0])
    original = parse_raw_mime
    calls = 0

    def fail_first(raw: bytes) -> RawMimeTree:
        nonlocal calls
        calls += 1
        if calls == 1:
            raise problem
        return original(raw)

    monkeypatch.setattr(batch, "parse_raw_mime", fail_first)
    ledger = batch.execute(
        [str(path) for path in sources],
        batch.BatchOptions(
            dry_run=False,
            existing="error",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
        retain_evidence=False,
    )
    try:
        assert [item.status for item in ledger.items] == [
            ItemStatus.FAILED,
            ItemStatus.CREATED,
            ItemStatus.CREATED,
        ]
        assert ledger.items[0].error is not None
        assert ledger.items[0].error.code is ExitCode.INTERNAL_ERROR
        assert ledger.items[0].error.message == (str(problem) or "ValueError")
        assert ledger.items[0].error.phase == "bound"
        assert ledger.batch_error is None
        assert not (tmp_path / "fault.mime-pruned.eml").exists()
        assert (tmp_path / "second.mime-pruned.eml").read_bytes() == SEEDS[0]
        assert (tmp_path / "third.mime-pruned.eml").read_bytes() == SEEDS[0]
        assert all(path.read_bytes() == SEEDS[0] for path in sources)
    finally:
        report_stream.close(ledger)


def test_nested_structured_field_failure_has_its_actual_mime_location() -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain; charset\r\n\r\nBody\r\n--m--\r\n"
    )
    with pytest.raises(AppError, match="Content-Type:") as rejected:
        parse_raw_mime(raw)
    assert rejected.value.mime_path == (0,)


@pytest.mark.parametrize(
    "phase",
    [ItemPhase.INVENTORIED, ItemPhase.CANDIDATE, ItemPhase.STAGED, ItemPhase.PUBLISHED],
)
def test_unexpected_fault_outside_local_mime_preparation_is_not_contained(
    phase: ItemPhase,
) -> None:
    item = LedgerItem(0, path_value("example.eml"), phase=phase)
    problem = ValueError("shared state fault")

    def fail(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise problem

    with pytest.raises(ValueError, match="shared state fault") as rejected:
        batch_execution.prepare_candidate(item, FileIdentity(1, 2, "regular", 3), fail)
    assert rejected.value is problem
    assert item.status is None


@pytest.mark.parametrize(
    "problem",
    [
        AppError(ExitCode.INTERNAL_ERROR, "invariant"),
        MemoryError(),
        SystemExit(),
        KeyboardInterrupt(),
        CancellationSignal(2, "SIGINT"),
    ],
)
def test_preparation_does_not_contain_invariants_or_control_flow(
    problem: BaseException,
) -> None:
    item = LedgerItem(0, path_value("example.eml"), phase=ItemPhase.BOUND)

    def fail(_item: LedgerItem, _identity: FileIdentity) -> None:
        raise problem

    with pytest.raises(type(problem)) as rejected:
        batch_execution.prepare_candidate(item, FileIdentity(1, 2, "regular", 3), fail)
    assert rejected.value is problem
    assert item.status is None
