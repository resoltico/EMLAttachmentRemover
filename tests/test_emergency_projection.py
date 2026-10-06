"""Recovery preserves receipts without MIME detail, spool reads, or codec drift."""

from __future__ import annotations

import json
import sys
from contextlib import ExitStack
from io import BytesIO, TextIOWrapper
from types import SimpleNamespace

import pytest

from eml_attachment_remover import report_emergency
from eml_attachment_remover.cancellation import delivery_guard
from eml_attachment_remover.domain import AppError, ExitCode
from tests.test_reporting_complete_receipts import _complete_ledger


@pytest.mark.parametrize("encoding", ["ascii", "latin-1", "utf-16"])
@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
def test_recovery_projects_receipts_and_obeys_each_output_channel(
    monkeypatch: pytest.MonkeyPatch, encoding: str, fmt: str
) -> None:
    ledger = _complete_ledger()
    ledger.batch_error = AppError(ExitCode.BATCH_FAILURE, "batch π message")
    ledger.report_spool = object()
    ledger.emergency_report_spool = object()
    ledger.report_spool_failed = True
    out, err = BytesIO(), BytesIO()
    with (
        TextIOWrapper(out, encoding=encoding) as stdout,
        TextIOWrapper(err, encoding=encoding) as stderr,
    ):
        monkeypatch.setattr(sys, "stdout", stdout)
        monkeypatch.setattr(sys, "stderr", stderr)
        with ExitStack() as resources:
            channels = report_emergency.stage(resources, ledger, fmt, "apply", 9)
            with delivery_guard() as guard:
                channels.deliver(fmt, guard)
        if fmt == "json":
            document = json.loads(out.getvalue().decode("ascii"))
            assert document["exit_code"] == 9
            assert document["mode"] == "apply"
            assert document["summary"]["created"] == 1
            assert document["summary"]["failed"] == 1
            assert all(
                item[field] is None
                for item in document["items"]
                for field in ("source", "destination", "transformation", "verification")
            )
            assert all(item["warnings"] == [] for item in document["items"])
            assert document["items"][0]["publication"]["sha256"] == "published-sha256"
            assert document["items"][1]["error"] == {
                "code": "PARSE_ERROR",
                "message": "failed message",
                "mime_path": None,
                "phase": "parsed",
            }
            assert not err.getvalue()
        elif fmt == "paths0":
            assert out.getvalue() == b"published.bin\0"
            batch = (
                "batch π message" if encoding == "utf-16" else "batch \\u03c0 message"
            )
            assert f"Batch: BATCH_FAILURE: {batch}\n" in err.getvalue().decode(
                encoding
            ).replace("\r\n", "\n")
        else:
            text = out.getvalue().decode(encoding).replace("\r\n", "\n")
            display = (
                "source \\u03c0 display" if encoding != "utf-16" else "source π display"
            )
            assert f"created: {display} -> published display\n" in text
            assert "CREATED_WARN" not in text + err.getvalue().decode(encoding)
            batch = (
                "batch π message" if encoding == "utf-16" else "batch \\u03c0 message"
            )
            assert f"Batch: BATCH_FAILURE: {batch}\n" in err.getvalue().decode(
                encoding
            ).replace("\r\n", "\n")
    assert ledger.items[0].source is not None
    assert ledger.items[0].transformation is not None
    assert ledger.items[0].warnings


@pytest.mark.parametrize("stream", [SimpleNamespace(encoding=None), object()])
def test_recovery_uses_utf8_when_a_stream_has_no_encoding(
    monkeypatch: pytest.MonkeyPatch, stream: object
) -> None:
    monkeypatch.setattr(sys, "stdout", stream)
    monkeypatch.setattr(sys, "stderr", stream)
    ledger = _complete_ledger()
    ledger.batch_error = AppError(ExitCode.BATCH_FAILURE, "batch π message")
    with ExitStack() as resources:
        channels = report_emergency.stage(resources, ledger, "human", "apply", 9)
        assert "source π display" in channels.out.read()
        assert channels.err.buffer.read().endswith(
            "Batch: BATCH_FAILURE: batch π message\n".encode()
        )
