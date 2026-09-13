"""Verifier fault-injection regression for stale-header policy independence."""

from __future__ import annotations

import pytest

from eml_attachment_remover.domain import AppError
from eml_attachment_remover.mime_execution import build_candidate
from eml_attachment_remover.mime_policy import classify
from eml_attachment_remover.mime_raw import parse_raw_mime
from eml_attachment_remover.mime_verification import verify_candidate


def test_verifier_detects_executor_stale_header_policy_fault(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    raw = (
        b"Content-Type: multipart/mixed; boundary=m\r\n"
        b"Content-MD5: stale\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )
    source = parse_raw_mime(raw)
    plan = classify(source.root)
    monkeypatch.setattr(
        "eml_attachment_remover.mime_execution.CHANGED_HEADERS", frozenset()
    )
    candidate = build_candidate(source, plan.removals)
    with pytest.raises(AppError):
        verify_candidate(source, candidate, {removal.path for removal in plan.removals})
