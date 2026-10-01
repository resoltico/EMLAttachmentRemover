"""Character and encoded-byte limits agree with cumulative report admission."""

from __future__ import annotations

import json

import pytest
from hypothesis import given
from hypothesis import strategies as st

from eml_attachment_remover import report_budget, report_diagnostics, reporting_v3
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value


@pytest.mark.parametrize("character", ["\x01", "a", "é", "😀", '"', "\\"])
@pytest.mark.parametrize("length", [0, 1023, 1024, 1025, 2048, 2049, 4096])
def test_diagnostic_limits_include_the_truncation_marker(
    character: str, length: int
) -> None:
    message = character * length
    bounded = report_diagnostics.bounded_message(message)
    assert len(bounded) <= report_diagnostics.MAX_ERROR_MESSAGE
    assert (
        len(json.dumps(bounded, ensure_ascii=True)) - 2
        <= report_diagnostics.MAX_ERROR_BYTES
    )
    if bounded != message:
        assert bounded.endswith("…")
        assert message.startswith(bounded[:-1])


@given(st.text(max_size=5000))
def test_every_accepted_diagnostic_fits_its_minimal_reservation(message: str) -> None:  # type: ignore[misc]
    ledger = BatchLedger.from_requests([path_value("s.eml")])
    item = ledger.items[0]
    reserved = len(
        json.dumps(report_budget.minimal_record(item, worst=True), ensure_ascii=True)
    )
    item.finish(ItemStatus.FAILED, AppError(ExitCode.WRITE_ERROR, message))
    actual = len(json.dumps(report_budget.minimal_record(item), ensure_ascii=True))
    assert actual <= reserved


def test_non_bmp_failures_cannot_consume_later_inputs_reservations() -> None:
    ledger = BatchLedger.from_requests([path_value(f"s{i}.eml") for i in range(4)])
    budget = report_budget.plan(ledger)
    assert budget is not None
    for item in ledger.items:
        item.finish(ItemStatus.FAILED, AppError(ExitCode.WRITE_ERROR, "😀" * 2048))
    actual = [
        len(json.dumps(report_budget.minimal_record(item), ensure_ascii=True)) + 1
        for item in ledger.items
    ]
    for index in range(len(actual)):
        assert sum(actual[index + 1 :]) <= budget.tail[index]
    assert reporting_v3.MAX_ERROR_BYTES == report_diagnostics.MAX_ERROR_BYTES


def test_byte_truncation_keeps_the_longest_prefix_at_an_exact_boundary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(report_diagnostics, "MAX_ERROR_BYTES", 19)
    assert report_diagnostics.bounded_message("😀aa😀") == "😀a…"
