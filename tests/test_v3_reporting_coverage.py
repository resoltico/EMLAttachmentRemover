"""Schema-3 reporting failure and human/native-output edge contracts."""

from __future__ import annotations

from base64 import b64encode
from typing import cast

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
from eml_attachment_remover.native_paths import path_value


def _ledger(status: ItemStatus) -> BatchLedger:
    ledger = BatchLedger.from_requests([path_value("input.eml")])
    ledger.items[0].finish(status)
    return ledger


def test_report_rejects_nonterminal_items_and_serializes_windows_basename() -> None:
    pending = BatchLedger.from_requests([path_value("pending.eml")])
    with pytest.raises(RuntimeError):
        reporting_v3.report(pending, "apply", 9)

    assert reporting_v3._basename("name") == {  # ruff: ignore[private-member-access] - native report encoding contract.
        "basename_base64": None,
        "basename_utf16le_base64": b64encode("name".encode("utf-16-le")).decode(),
    }
    assert reporting_v3._basename(b"name")["basename_base64"] == "bmFtZQ=="  # ruff: ignore[private-member-access] - POSIX native report encoding contract.
    interrupted = _ledger(ItemStatus.CREATED)
    interrupted.record_interruption("SIGTERM", "report")
    document = reporting_v3.report(interrupted, "apply", ExitCode.INTERRUPTED)
    assert document["interruption"] == {
        "signal": "SIGTERM",
        "reason": "interrupted by SIGTERM",
        "phase": "report",
    }


def test_human_reporting_handles_malformed_and_structured_entries(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with pytest.raises(TypeError):
        reporting_v3.write_human({"items": "not-a-list"})

    document: dict[str, object] = {
        "items": [
            "not-a-mapping",
            {
                "status": "failed",
                "source_request": {"display": "request"},
                "warnings": ["bad", {"code": "WARN", "message": "detail"}],
                "error": {"code": "PARSE_ERROR", "message": "bad message"},
            },
        ]
    }
    reporting_v3.write_human(document)
    captured = capsys.readouterr()
    assert "None: <unknown>" in captured.out
    assert "failed: request" in captured.out
    assert "request: WARN: detail" in captured.err
    assert "request: PARSE_ERROR: bad message" in captured.err


def test_paths0_uses_native_bytes_fallback_and_reports_errors(
    capfd: pytest.CaptureFixture[str],
) -> None:
    created = _ledger(ItemStatus.CREATED)
    created.items[0].publication = PublicationReceipt(
        visibility="published",
        identity=None,
        digest="digest",
        file_sync="succeeded",
        directory_sync="succeeded",
        address_verified=True,
        final_address=PathValue(
            "native-path",
            "native-path",
            b64encode(b"raw-native").decode(),
        ),
        temp_cleanup="not_needed",
    )
    fallback = _ledger(ItemStatus.EXISTING_VERIFIED)
    fallback.items[0].publication = PublicationReceipt(
        visibility="existing_verified",
        identity=None,
        digest="digest",
        file_sync="not_attempted",
        directory_sync="not_attempted",
        address_verified=True,
        final_address=PathValue("fallback", "fallback", None, "dABlAHgAdAA="),
        temp_cleanup="not_needed",
    )
    failed = _ledger(ItemStatus.FAILED)
    failed.items[0].error = AppError(ExitCode.PARSE_ERROR, "bad")
    failed.items[0].warnings = cast(
        "list[dict[str, object]]",
        ["ignored", {"code": "WARN", "message": "detail"}],
    )
    joined = BatchLedger(created.items + fallback.items + failed.items)
    reporting_v3.write_paths0(joined)
    output, errors = capfd.readouterr()
    assert output.encode() == b"raw-native\0fallback\0"
    assert "PARSE_ERROR: bad" in errors
    assert "WARN: detail" in errors
