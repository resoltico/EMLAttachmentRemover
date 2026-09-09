"""Final narrow MIME parser and verification receipts."""

from __future__ import annotations

import hashlib

import pytest

from eml_attachment_remover import mime_execution, mime_verification
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_execution import Candidate
from eml_attachment_remover.mime_raw import parse_raw_mime


def test_raw_edit_plans_reject_empty_and_overlapping_spans_exactly() -> None:
    """Executor span normalization cannot accept an empty or overlapping deletion."""
    for edits in ([(2, 2)], [(2, 5), (4, 7)]):
        with pytest.raises(AppError) as rejected:
            mime_execution._nonoverlapping(edits)  # ruff: ignore[private-member-access] - direct raw edit-plan receipt.
        assert rejected.value == AppError(
            ExitCode.VERIFICATION_ERROR, "overlapping raw MIME edits"
        )


def test_verifier_rejects_a_digest_that_does_not_bind_candidate_bytes() -> None:
    """A separately parsed candidate must carry the digest of its exact raw bytes."""
    raw = b"Content-Type: text/plain\r\n\r\nbody\r\n"
    tree = parse_raw_mime(raw)
    candidate = Candidate(raw, hashlib.sha256(b"different").hexdigest(), ())
    with pytest.raises(AppError) as rejected:
        mime_verification.verify_candidate(tree, candidate, set())
    assert rejected.value == AppError(
        ExitCode.VERIFICATION_ERROR, "candidate MIME verification failed"
    )
