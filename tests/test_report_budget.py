"""Report evidence is reserved before publication and every input keeps its place."""

from __future__ import annotations

import json
from dataclasses import replace

import pytest
from hypothesis import assume, given
from hypothesis import strategies as st

from eml_attachment_remover import (
    native_values,
    report_budget,
    report_document,
    report_spool,
    report_stream,
)
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    FileIdentity,
    ItemStatus,
    LedgerItem,
    PathValue,
    PublicationReceipt,
    TransformationPlan,
)
from eml_attachment_remover.native_paths import path_value


def _canonical(value: object) -> int:
    """Measure a value's canonical JSON independently of the code under test.

    Returns:
        The number of ASCII bytes of the serialization.

    """
    return len(json.dumps(value, ensure_ascii=True, sort_keys=True, allow_nan=False))


_PLAN = TransformationPlan((), (), (), "a" * 64, 1, b"x")


def _detail(item: LedgerItem) -> tuple[object, list[dict[str, object]]]:
    return item.transformation, item.warnings


def _prepared(ledger: BatchLedger, index: int = 0) -> LedgerItem:
    item = ledger.items[index]
    item.transformation = _PLAN
    item.warnings.append({"code": "W"})
    return item


def _spooled(count: int = 1) -> tuple[BatchLedger, report_spool.ReportSpool]:
    ledger = BatchLedger.from_requests([
        path_value(f"source{index}.eml") for index in range(count)
    ])
    report_stream.start(ledger)
    budget = report_budget.plan(ledger)
    assert budget is not None
    ledger.report_budget = budget
    spool = ledger.report_spool
    assert isinstance(spool, report_spool.ReportSpool)
    return ledger, spool


def _receipt(address: PathValue | None) -> PublicationReceipt:
    return PublicationReceipt(
        "visible",
        FileIdentity(1, 2, "-rw-------", 3),
        "b" * 64,
        "succeeded",
        "succeeded",
        address_verified=True,
        final_address=address,
        temp_cleanup="pending",
    )


def test_the_worst_address_is_exactly_the_longest_reportable_path() -> None:
    """The reservation is built from the same limit the destination binder enforces."""
    address = report_budget.worst_address()
    assert native_values.address_units(address) == native_values.MAX_ADDRESS_UNITS


@given(
    text=st.text(
        alphabet=st.characters(codec="utf-8", exclude_characters="\0"),
        min_size=1,
        max_size=native_values.MAX_ADDRESS_UNITS,
    ),
    failed=st.booleans(),
)
def test_no_real_terminal_record_exceeds_its_reservation(  # type: ignore[misc]
    text: str, *, failed: bool
) -> None:
    """Whatever address a publication reports, the prepared reservation held it."""
    address = path_value(text)
    assume(native_values.address_units(address) <= native_values.MAX_ADDRESS_UNITS)
    ledger = BatchLedger.from_requests([path_value("s.eml")])
    item = _prepared(ledger)
    reserved = report_budget.terminal_size(item)
    error = (
        AppError(ExitCode.WRITE_ERROR, "m" * 5000, (1, 2, 3), "publish")
        if failed
        else None
    )
    terminal = replace(
        item,
        status=ItemStatus.PUBLISHED_WITH_ERROR,
        terminalized=True,
        publication=_receipt(address),
        error=error,
    )
    assert _canonical(report_document.item_json(terminal)) <= reserved


def test_the_reservation_is_a_small_fraction_of_the_item_limit() -> None:
    """Reserving the worst address must leave nearly the whole record for evidence."""
    ledger = BatchLedger.from_requests([path_value("s.eml")])
    reserved = report_budget.terminal_size(_prepared(ledger))
    assert reserved < report_spool.MAX_RECORD_BYTES // 8


def test_error_messages_are_capped_in_the_serialized_record() -> None:
    """A long exception text cannot outgrow the reserved error."""
    cap = report_document.MAX_ERROR_MESSAGE
    long = report_document.error_json(AppError(ExitCode.PARSE_ERROR, "e" * (cap + 50)))
    exact = report_document.error_json(AppError(ExitCode.PARSE_ERROR, "e" * cap))
    assert long is not None
    assert exact is not None
    assert long["message"] == "e" * (cap - 1) + "…"
    assert exact["message"] == "e" * cap


def test_admission_is_skipped_without_a_spool_or_budget() -> None:
    """Without a private spool or a plan there is no limit to protect."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    item = _prepared(ledger)
    report_budget.admit(ledger, item)
    assert _detail(item) == (_PLAN, [{"code": "W"}])
    spool = report_spool.ReportSpool.create()
    ledger.report_spool = spool
    try:
        report_budget.admit(ledger, item)
    finally:
        spool.close()
    assert _detail(item) == (_PLAN, [{"code": "W"}])


def test_record_exactly_at_the_limit_is_admitted_and_one_over_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The record bound is inclusive and includes the terminal fields."""
    ledger, _spool = _spooled()
    try:
        item = _prepared(ledger)
        size = report_budget.terminal_size(item)
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", size)
        report_budget.admit(ledger, item)
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", size - 1)
        with pytest.raises(AppError) as raised:
            report_budget.admit(ledger, item)
    finally:
        report_stream.close(ledger)
    assert raised.value == AppError(
        ExitCode.PARSE_ERROR, report_budget.RECORD_LIMIT_MESSAGE
    )
    assert _detail(item) == (None, [])


def test_capacity_counts_the_record_its_newline_and_every_later_input(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A record is admitted only if every later input can still be terminalized."""
    ledger, spool = _spooled(3)
    try:
        budget = ledger.report_budget
        assert isinstance(budget, report_budget.ReportBudget)
        item = _prepared(ledger)
        exact = spool.bytes_written + report_budget.terminal_size(item) + 1
        exact += budget.tail[0]
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", exact)
        report_budget.admit(ledger, item)
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", exact - 1)
        with pytest.raises(AppError) as raised:
            report_budget.admit(ledger, item)
    finally:
        report_stream.close(ledger)
    assert raised.value.message == report_budget.CAPACITY_LIMIT_MESSAGE


def test_the_record_limit_is_reported_before_the_capacity_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """When both would be exceeded, the record itself is what is too large."""
    ledger, _spool = _spooled()
    try:
        item = _prepared(ledger)
        size = report_budget.terminal_size(item)
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", size - 1)
        monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", size - 1)
        with pytest.raises(AppError) as raised:
            report_budget.admit(ledger, item)
    finally:
        report_stream.close(ledger)
    assert raised.value.message == report_budget.RECORD_LIMIT_MESSAGE


def test_tail_reserves_each_later_input_and_none_after_the_last() -> None:
    """``tail[i]`` is exactly the minimal records of inputs after ``i``."""
    ledger = BatchLedger.from_requests([path_value(f"s{n}.eml") for n in range(4)])
    budget = report_budget.plan(ledger)
    assert budget is not None
    needs = [
        _canonical(report_budget.minimal_record(item, worst=True)) + 1
        for item in ledger.items
    ]
    assert budget.tail == (
        sum(needs[1:]),
        sum(needs[2:]),
        needs[3],
        0,
    )


def test_a_batch_whose_minimal_records_cannot_fit_gets_no_plan(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Even the smallest honest report must fit before any input runs."""
    ledger = BatchLedger.from_requests([path_value("a.eml"), path_value("b.eml")])
    total = sum(
        _canonical(report_budget.minimal_record(item, worst=True)) + 1
        for item in ledger.items
    )
    monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", total)
    assert report_budget.plan(ledger) is not None
    monkeypatch.setattr(report_spool, "MAX_SPOOL_BYTES", total - 1)
    assert report_budget.plan(ledger) is None


def test_minimal_record_keeps_identity_and_outcome_only() -> None:
    """The reduced form drops bulky evidence and an error's MIME path."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    item = _prepared(ledger)
    item.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad", (1, 2)))
    record = report_budget.minimal_record(item)
    assert (record["transformation"], record["warnings"], record["source"]) == (
        None,
        [],
        None,
    )
    assert record["status"] == "failed"
    assert record["error"] == {
        "code": "PARSE_ERROR",
        "message": "bad",
        "mime_path": None,
        "phase": None,
    }
    assert report_budget.minimal_record(ledger.items[0], worst=True)["error"]


def test_minimal_record_of_an_unfinished_item_has_no_error() -> None:
    """An item without an error keeps a null error in its reduced form."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    assert report_budget.minimal_record(ledger.items[0])["error"] is None


def test_an_actual_record_that_does_not_fit_is_spooled_in_its_minimal_form(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Failing after preparation must never cost the report its later inputs."""
    ledger, spool = _spooled(2)
    try:
        first = ledger.items[0]
        first.warnings.append({"code": "W", "message": "m" * 5000})
        first.finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
        monkeypatch.setattr(report_spool, "MAX_RECORD_BYTES", 4000)
        report_stream.archive(ledger, first)
        record = json.loads(next(spool.records()))
    finally:
        report_stream.close(ledger)
    assert (record["status"], record["warnings"], record["source_request"]) == (
        "failed",
        [],
        report_document.path_json(first.source_request),
    )
