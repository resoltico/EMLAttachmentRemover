"""Exact receipts for the worst-case reservation and the minimal record."""

from __future__ import annotations

import base64
from types import SimpleNamespace
from typing import cast

import pytest

from eml_attachment_remover import native_values, report_budget, report_spool
from eml_attachment_remover.domain import (
    AppError,
    ExitCode,
    FileIdentity,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
)

WORD = "w" * 32
SOURCE = PathValue("s.eml", "s.eml", "cy5lbWw=")


def _fake_path(text: str) -> PathValue:
    """Describe a path identically on every host (one display byte per unit).

    Returns:
        A path value whose serialized size depends only on the text length.

    """
    return PathValue(text, "d" * len(text), base64.b64encode(text.encode()).decode())


@pytest.fixture(autouse=True)  # ruff: ignore[pytest-fixture-autouse] - every test needs it.
def host_independent_paths(monkeypatch: pytest.MonkeyPatch) -> None:
    """Make the reserved address's serialized size the same on every platform."""
    monkeypatch.setattr(report_budget, "path_value", _fake_path)


def _spool(written: int) -> report_spool.ReportSpool:
    return cast("report_spool.ReportSpool", SimpleNamespace(bytes_written=written))


def test_the_worst_words_are_the_longest_enumerated_values() -> None:
    """Statuses, phases, and exit codes are reserved at their longest spelling."""
    assert report_budget.LONGEST_STATUS == ItemStatus.PUBLISHED_WITH_ERROR
    assert report_budget.LONGEST_PHASE == "inventoried"
    assert report_budget.LONGEST_CODE is ExitCode.TRANSFORMATION_UNAVAILABLE
    assert (report_budget.WORST_WORD, report_budget.WORST_NUMBER) == (WORD, 10**30)
    assert report_budget.NEWLINE_BYTES == 1


def test_the_worst_receipt_fills_every_field_with_its_largest_value() -> None:
    """Nothing in the reserved receipt is null, false, or short."""
    address = _fake_path("\x01" * native_values.MAX_ADDRESS_UNITS)
    assert report_budget.worst_address() == address
    assert report_budget._worst_receipt() == PublicationReceipt(  # ruff: ignore[private-member-access] - reservation receipt.
        WORD,
        FileIdentity(10**30, 10**30, WORD, 10**30),
        "f" * 64,
        WORD,
        WORD,
        address_verified=True,
        final_address=address,
        temp_cleanup=WORD,
    )


def test_the_worst_error_has_the_longest_code_message_and_phase() -> None:
    """Errors after admission carry no MIME path."""
    assert report_budget._worst_error() == AppError(  # ruff: ignore[private-member-access] - reservation error.
        ExitCode.TRANSFORMATION_UNAVAILABLE, "\x01" * 2048, None, WORD
    )


def test_the_reserved_terminal_record_has_this_exact_size() -> None:
    """One pinned number covers every field of the worst-case terminal record."""
    assert report_budget.terminal_size(LedgerItem(0, SOURCE)) == 47507


def test_the_minimal_worst_record_has_this_exact_size_and_outcome() -> None:
    """A later input's reserve is its identity plus the largest outcome."""
    item = LedgerItem(0, SOURCE)
    record = report_budget.minimal_record(item, worst=True)
    assert (record["status"], record["phase"]) == (
        "published_with_error",
        "inventoried",
    )
    assert record["error"] == {
        "code": "TRANSFORMATION_UNAVAILABLE",
        "message": "\x01" * 2048,
        "mime_path": None,
        "phase": WORD,
    }
    assert report_budget._size(record) == 12775  # ruff: ignore[private-member-access] - exact reserve size.


def test_the_minimal_record_keeps_only_identity_outcome_and_a_pathless_error() -> None:
    """Everything else is dropped, and the item's own status and phase are kept."""
    item = LedgerItem(3, SOURCE)
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad", (1, 2), "p"))
    assert report_budget.minimal_record(item) == {
        "index": 3,
        "phase": "requested",
        "status": "failed",
        "terminalized": True,
        "source_request": {
            "text": "s.eml",
            "display": "s.eml",
            "native_base64": "cy5lbWw=",
            "native_utf16le_base64": None,
        },
        "destination_request": None,
        "source": None,
        "destination": None,
        "transformation": None,
        "verification": None,
        "publication": None,
        "warnings": [],
        "error": {
            "code": "PARSE_ERROR",
            "message": "bad",
            "mime_path": None,
            "phase": "p",
        },
    }


def test_a_minimal_record_drops_detail_but_keeps_a_receipt_it_already_has() -> None:
    """A published item's receipt survives the reduction."""
    item = LedgerItem(0, SOURCE)
    item.publication = report_budget._worst_receipt()  # ruff: ignore[private-member-access] - kept receipt.
    item.warnings.append({"code": "W"})
    record = report_budget.minimal_record(item)
    assert record["publication"] is not None
    assert record["warnings"] == []
    assert item.warnings == [{"code": "W"}]


@pytest.mark.parametrize(
    ("written", "size", "index", "expected"),
    [
        (0, 1000, 0, True),
        (0, 1001, 0, False),
        (1, 1000, 0, False),
        (0, 1000, 1, True),
        (51, 1000, 1, False),
        (50, 1000, 1, True),
    ],
)
def test_a_record_fits_only_within_its_limit_and_the_later_reserve(
    monkeypatch: pytest.MonkeyPatch,
    written: int,
    size: int,
    index: int,
    *,
    expected: bool,
) -> None:
    """Both bounds are inclusive; the later inputs' tail counts, never subtracts."""
    monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", 1000)
    monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", 1000 + 1 + 100)
    budget = report_budget.ReportBudget((100, 50, 0))
    assert budget.fits(_spool(written), index, size) is expected


def test_capacity_counts_what_is_already_written_and_the_newline() -> None:
    """The spool's used bytes add, and one byte is reserved for the delimiter."""
    budget = report_budget.ReportBudget((0,))
    limit = report_spool.MAX_SPOOL_BYTES
    assert budget.fits(_spool(limit - 11), 0, 10)
    assert not budget.fits(_spool(limit - 10), 0, 10)
    assert not budget.fits(_spool(limit), 0, 0)
    assert budget.fits(_spool(limit - 1), 0, 0)
