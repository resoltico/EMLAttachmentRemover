"""A verifier cannot trust the executor's changed-ancestor traversal."""

from __future__ import annotations

import pytest

from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_removals import RemovalIndex
from eml_attachment_remover.mime_verification import verify_candidate


def test_verifier_rejects_executor_omitting_affected_ancestor(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """One broken source of changed headers must not mislead the verifier."""
    raw = (
        b"Content-MD5: deliberately-stale\r\n"
        b"Content-Type: multipart/mixed; boundary=b\r\n\r\n"
        b"--b\r\nContent-Type: text/plain\r\n\r\nbody\r\n"
        b"--b\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nprivate\r\n--b--\r\n"
    )
    tree = parse_raw_mime(raw)
    removals = classify(tree.root).removals
    monkeypatch.setattr(
        RemovalIndex, "changed_ancestor_paths", lambda *_args: frozenset()
    )
    candidate = build_candidate(tree, removals)
    assert b"Content-MD5: deliberately-stale" in candidate.raw
    with pytest.raises(AppError) as failure:
        verify_candidate(tree, candidate, removals)
    assert failure.value.code is ExitCode.VERIFICATION_ERROR
