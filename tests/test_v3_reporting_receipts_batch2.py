"""Independent byte-channel and display-encoding report receipts."""

from __future__ import annotations

import io
import sys
from typing import TextIO, cast

import pytest

from eml_attachment_remover import reporting_v3
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    PathValue,
    PublicationReceipt,
)


class _RecordingStream:
    """A deliberately narrow text stream which records each rendered line."""

    def __init__(self, encoding: str | None) -> None:
        self.encoding = encoding
        self.writes: list[str] = []

    def write(self, value: str) -> int:
        self.writes.append(value)
        return len(value)


@pytest.mark.parametrize(
    ("encoding", "text", "expected"),
    [
        (None, "ordinary", "ordinary\n"),
        ("ascii", "bad-\udcff", "bad-\\udcff\n"),
    ],
)
def test_safe_writes_one_line_with_its_declared_or_fallback_encoding(
    encoding: str | None, text: str, expected: str
) -> None:
    """Human output has a deterministic, lossless fallback for display-only text."""
    stream = _RecordingStream(encoding)
    reporting_v3._safe(  # ruff: ignore[private-member-access] - rendering boundary receipt.
        cast("TextIO", stream), text
    )
    assert stream.writes == [expected]


def test_human_output_handles_nonmapping_items_warnings_and_errors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Malformed display documents never bypass the one-line/error channel contract."""
    standard = _RecordingStream("utf-8")
    errors = _RecordingStream("ascii")
    monkeypatch.setattr(sys, "stdout", standard)
    monkeypatch.setattr(sys, "stderr", errors)
    reporting_v3.write_human({
        "items": [
            "not-a-record",
            {
                "status": "failed",
                "source_request": {"display": "source-\udcff"},
                "warnings": [{"code": "W", "message": "warn-\udcff"}],
                "error": {"code": "PARSE_ERROR", "message": "bad-\udcff"},
            },
        ]
    })
    assert standard.writes == ["None: <unknown>\n", "failed: source-\\udcff\n"]
    assert errors.writes == [
        "source-\\udcff: W: warn-\\udcff\n",
        "source-\\udcff: PARSE_ERROR: bad-\\udcff\n",
    ]


def test_paths0_uses_text_fallback_only_for_accepted_publications(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A non-base64 POSIX name remains byte exact and failures remain diagnostic."""
    ledger = BatchLedger.from_requests([PathValue("input", "input", None)])
    item = ledger.items[0]
    item.publication = PublicationReceipt(
        visibility="visible",
        identity=None,
        digest=None,
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=PathValue("final-π.eml", "final display", None),
        temp_cleanup="succeeded",
    )
    item.finish(ItemStatus.CREATED)
    output = io.BytesIO()
    errors = _RecordingStream("utf-8")
    monkeypatch.setattr(
        sys,
        "stdout",
        cast("TextIO", type("Out", (), {"buffer": output})()),
    )
    monkeypatch.setattr(sys, "stderr", errors)
    reporting_v3.write_paths0(ledger)
    assert output.getvalue() == "final-π.eml".encode() + b"\0"
    assert errors.writes == []


def test_paths0_keeps_failed_item_out_of_binary_output_and_reports_it(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A rejected record never leaks a final name into the NUL-delimited stream."""
    ledger = BatchLedger.from_requests([PathValue("bad", "bad display", None)])
    item = ledger.items[0]
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "broken"))
    output = io.BytesIO()
    errors = _RecordingStream("utf-8")
    monkeypatch.setattr(
        sys,
        "stdout",
        cast("TextIO", type("Out", (), {"buffer": output})()),
    )
    monkeypatch.setattr(sys, "stderr", errors)
    reporting_v3.write_paths0(ledger)
    assert output.getvalue() == b""
    assert errors.writes == ["bad display: PARSE_ERROR: broken\n"]
