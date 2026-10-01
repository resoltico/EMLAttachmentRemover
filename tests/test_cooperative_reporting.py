"""Pending requests select one complete report and one invocation diagnostic."""

from __future__ import annotations

import json
import signal
from contextlib import ExitStack
from typing import TYPE_CHECKING

from eml_attachment_remover import batch_execution, cancellation, report_session
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_values import path_value
from eml_attachment_remover.report_session import ReportSession
from tests.report_session_support import open_session

if TYPE_CHECKING:
    import pytest


def test_a_new_report_interruption_is_rendered_even_if_status_was_already_130(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    ledger = BatchLedger.from_requests([path_value("public.eml")])
    ledger.items[0].finish(
        ItemStatus.FAILED, AppError(ExitCode.INTERRUPTED, "earlier item failure")
    )
    calls: list[int] = []

    def requested() -> None:
        calls.append(1)
        if len(calls) == 2:
            raise cancellation.CancellationSignal(signal.SIGINT, "SIGINT")

    monkeypatch.setattr(report_session, "checkpoint", requested)
    with ExitStack() as resources:
        assert open_session(resources, "json").publish(ledger, 130) == 130
    document = json.loads(capsys.readouterr().out)
    assert document["interrupted"]
    assert document["interruption"]["signal"] == "SIGINT"
    assert document["interruption"]["phase"] == "report"
    assert document["summary"]["failed"] == 1


def test_interruption_during_staging_restages_before_output(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    ledger = BatchLedger.from_requests([path_value("public.eml")])
    ledger.items[0].finish(ItemStatus.NOT_RUN)
    original = ReportSession._render  # ruff: ignore[private-member-access] - real staging seam.
    calls: list[int] = []

    def render(session: ReportSession, value: BatchLedger, status: int) -> None:
        original(session, value, status)
        calls.append(status)
        if len(calls) == 1:
            signal.raise_signal(signal.SIGINT)

    monkeypatch.setattr(ReportSession, "_render", render)
    with cancellation.install_cancellation_handlers(), ExitStack() as resources:
        session = open_session(resources, "json")
        assert session.publish(ledger, 0) == 130
    report = json.loads(capsys.readouterr().out)
    assert report["interrupted"]
    assert report["exit_code"] == 130
    assert calls == [0]


def test_second_checkpoint_does_not_overwrite_an_existing_interruption(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ledger = BatchLedger.from_requests([path_value("public.eml")])
    ledger.record_interruption("SIGHUP", "inventory")

    def requested() -> None:
        raise cancellation.CancellationSignal(signal.SIGINT, "SIGINT")

    monkeypatch.setattr(report_session, "checkpoint", requested)
    assert report_session._acknowledge(ledger, 0) == 130  # ruff: ignore[private-member-access] - checkpoint contract.
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGHUP"


def test_batch_boundary_retains_the_first_interruption(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ledger = BatchLedger.from_requests([path_value("public.eml")])
    ledger.record_interruption("SIGHUP", "inventory")

    def requested(*_args: object) -> None:
        raise cancellation.CancellationSignal(signal.SIGINT, "SIGINT")

    monkeypatch.setattr(batch_execution, "_execute", requested)
    options = BatchOptions(
        dry_run=False, existing="error", fail_fast=False, output=None, output_dir=None
    )
    result = batch_execution.run(ledger, [], options, requested, limits=(1, 1))
    assert result is ledger
    assert ledger.interruption is not None
    assert ledger.interruption.signal == "SIGHUP"
