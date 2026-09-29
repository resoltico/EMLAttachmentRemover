"""Exact public CLI mutation receipts for cancellation and removed options."""

from __future__ import annotations

import json
from contextlib import ExitStack

import pytest

from eml_attachment_remover import cli, report_session
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value
from tests.report_session_support import open_session


def test_preledger_cancellation_retains_its_exact_interruption_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The no-ledger path has one full interruption error and no report attempt."""
    captured: list[tuple[AppError, object | None]] = []

    def render(error: AppError, parser: object | None) -> int:
        captured.append((error, parser))
        return int(error.code)

    monkeypatch.setattr(cli, "_render_error", render)
    signal = CancellationSignal(15, "SIGTERM")
    assert cli._cancelled(cli._RunState(), signal) == 130  # ruff: ignore[private-member-access] - preledger cancellation receipt.
    assert captured == [
        (AppError(ExitCode.INTERRUPTED, "interrupted by SIGTERM"), None)
    ]


@pytest.mark.parametrize("mode", ["dry-run", "apply"])
@pytest.mark.parametrize("output_format", ["json", "human"])
def test_active_cancellation_reports_its_retained_mode_on_its_selected_channel(
    output_format: str, mode: str, capsys: pytest.CaptureFixture[str]
) -> None:
    """An interrupted report keeps the requested channel and mode, never defaults."""
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(ItemStatus.CREATED)
    with ExitStack() as resources:
        state = cli._RunState(  # ruff: ignore[private-member-access] - active cancellation state.
            ledger, open_session(resources, output_format, mode)
        )
        status = cli._cancelled(  # ruff: ignore[private-member-access] - active cancellation receipt.
            state, CancellationSignal(2, "SIGHUP")
        )
    assert status == 130
    captured = capsys.readouterr()
    if output_format == "json":
        document = json.loads(captured.out)
        assert (
            document["mode"],
            document["exit_code"],
            document["interruption"],
            document["items"][0]["status"],
        ) == (
            mode,
            130,
            {
                "signal": "SIGHUP",
                "reason": "interrupted by SIGHUP",
                "phase": "report",
            },
            "created",
        )
        assert not captured.err
    else:
        assert captured.out == "created: source.eml\n"
        assert not captured.err


def test_json_error_terminalizes_every_source_with_the_configuration_reason(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Raw-argv JSON validation failures retain their exact batch reason and status."""
    error = AppError(ExitCode.USAGE, "invalid selected option")
    assert cli._json_error(["first.eml", "second.eml"], error) == 2  # ruff: ignore[private-member-access] - JSON failure receipt.
    captured = capsys.readouterr()
    assert not captured.err
    document = json.loads(captured.out)
    assert (
        document["mode"],
        document["exit_code"],
        document["batch_error"],
        [(item["status"], item["error"]["message"]) for item in document["items"]],
    ) == (
        "apply",
        2,
        {
            "code": "USAGE",
            "message": "invalid selected option",
            "mime_path": None,
            "phase": None,
        },
        [
            ("not_run", "batch configuration error"),
            ("not_run", "batch configuration error"),
        ],
    )


def test_run_keeps_every_validated_option_and_the_completed_ledger(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The parser boundary forwards all five batch controls without substitution."""
    expected_ledger = BatchLedger.from_requests([path_value("source.eml")])
    expected_ledger.items[0].finish(ItemStatus.WOULD_CREATE)
    captured: list[BatchOptions] = []

    def execute(_sources: list[str], options: BatchOptions) -> BatchLedger:
        captured.append(options)
        return expected_ledger

    monkeypatch.setattr(
        cli,
        "execute",
        execute,
    )
    monkeypatch.setattr(report_session, "_write_selected", lambda *_arguments: None)
    state = cli._RunState()  # ruff: ignore[private-member-access] - run-state receipt.
    with ExitStack() as resources:
        assert (
            cli._run(  # ruff: ignore[private-member-access] - parsed options receipt.
                [
                    "--dry-run",
                    "--existing=verify",
                    "--fail-fast",
                    "--output-dir",
                    "destinations",
                    "--output-format=json",
                    "source.eml",
                ],
                state,
                resources,
            )
            == 0
        )
        assert state.session is not None
        assert (state.session.output_format, state.session.mode) == (
            "json",
            "dry-run",
        )
    assert captured == [
        BatchOptions(
            dry_run=True,
            existing="verify",
            fail_fast=True,
            output=None,
            output_dir="destinations",
        )
    ]
    assert state.ledger is expected_ledger
