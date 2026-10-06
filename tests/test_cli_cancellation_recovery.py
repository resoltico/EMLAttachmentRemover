"""Interrupted error responses remain complete and are delivered only once."""

from __future__ import annotations

import json
from contextlib import ExitStack
from typing import TYPE_CHECKING

from eml_attachment_remover import cli, report_emergency
from eml_attachment_remover.cancellation import CancellationSignal, DeliveryGuard
from eml_attachment_remover.cli_intent import OutputIntent
from eml_attachment_remover.domain import AppError, ExitCode
from tests.report_session_support import open_session

if TYPE_CHECKING:
    import pytest

    from eml_attachment_remover.domain import BatchLedger
    from eml_attachment_remover.report_delivery import StagedChannels


def test_repeated_startup_cancellation_never_delivers_a_second_json_document(
    capsys: pytest.CaptureFixture[str],
) -> None:
    state = cli._RunState(  # ruff: ignore[private-member-access] - retained error response owner.
        intent=OutputIntent("json", "dry-run", ("source.eml",)),
        error_delivery=DeliveryGuard(),
    )
    for _ in range(2):
        assert cli._cancelled(state, CancellationSignal(2, "SIGINT")) == 130  # ruff: ignore[private-member-access] - repeated response recovery.
    captured = capsys.readouterr()
    document = json.loads(captured.out)
    assert document["exit_code"] == 130
    assert document["mode"] == "dry-run"
    assert len(document["items"]) == 1
    assert not captured.err


def test_cancellation_after_reported_interruption_is_quiet_and_successfully_returns(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with ExitStack() as resources:
        session = open_session(resources, "json", "apply")
        session.delivery_complete = True
        session.interruption_reported = True
        state = cli._RunState(session=session)  # ruff: ignore[private-member-access] - completed response owner.
        assert cli._cancelled(state, CancellationSignal(2, "SIGINT")) == 130  # ruff: ignore[private-member-access] - completed response recovery.
    captured = capsys.readouterr()
    assert not captured.out
    assert not captured.err


def test_interrupted_error_staging_recovers_without_reusing_a_processing_report(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    original_stage = report_emergency.stage
    interrupted = False

    def stage(
        resources: ExitStack,
        ledger: BatchLedger,
        output_format: str,
        mode: str,
        status: int,
    ) -> StagedChannels:
        nonlocal interrupted
        if not interrupted:
            interrupted = True
            raise KeyboardInterrupt
        return original_stage(resources, ledger, output_format, mode, status)

    def failed_processing(*_args: object, **_kwargs: object) -> None:
        raise AppError(ExitCode.WRITE_ERROR, "processing owner unavailable")

    monkeypatch.setattr(cli, "execute", failed_processing)
    monkeypatch.setattr(report_emergency, "stage", stage)
    assert cli.main(["--output-format=json", "source.eml"]) == 130
    captured = capsys.readouterr()
    document = json.loads(captured.out)
    assert document["exit_code"] == 130
    assert document["batch_error"]["code"] == "INTERRUPTED"
    assert len(document["items"]) == 1
    assert not captured.err
