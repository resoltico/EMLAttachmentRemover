"""Terminal CLI precedence, error rendering, and channel-selection contracts."""

from __future__ import annotations

from contextlib import ExitStack
from typing import TextIO

import pytest

from eml_attachment_remover import cli, exit_status, report_session, report_stream
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value
from tests.report_session_support import open_session


def _ledger(status: ItemStatus, error: AppError | None = None) -> BatchLedger:
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(status, error)
    return ledger


def test_exit_code_prioritizes_publication_and_single_failure_facts() -> None:
    publication_error = AppError(ExitCode.INTERNAL_ERROR, "sync")
    published = _ledger(ItemStatus.PUBLISHED_WITH_ERROR, publication_error)
    assert exit_status.exit_code(published) == ExitCode.INTERNAL_ERROR

    incomplete = _ledger(
        ItemStatus.PUBLISHED_WITH_ERROR, AppError(ExitCode.WRITE_ERROR, "x")
    )
    assert exit_status.exit_code(incomplete) == ExitCode.PUBLICATION_INCOMPLETE

    failed_without_error = _ledger(ItemStatus.FAILED)
    assert exit_status.exit_code(failed_without_error) == ExitCode.INTERNAL_ERROR

    failed_parse = _ledger(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    assert exit_status.exit_code(failed_parse) == ExitCode.PARSE_ERROR

    failed_internal = _ledger(
        ItemStatus.FAILED,
        AppError(ExitCode.INTERNAL_ERROR, "unexpected"),
    )
    assert exit_status.exit_code(failed_internal) == ExitCode.INTERNAL_ERROR

    interrupted = _ledger(ItemStatus.CREATED)
    interrupted.record_interruption("SIGTERM", "report")
    assert exit_status.exit_code(interrupted) == ExitCode.INTERRUPTED


def test_exit_code_uses_batch_failure_for_multiple_terminal_failures() -> None:
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    ledger.items[1].finish(ItemStatus.CREATED)
    assert exit_status.exit_code(ledger) == ExitCode.BATCH_FAILURE


def test_exit_code_uses_batch_failure_for_multiple_incomplete_publications() -> None:
    """Multiple visible-but-incomplete publications are a batch failure."""
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    for item in ledger.items:
        item.finish(
            ItemStatus.PUBLISHED_WITH_ERROR,
            AppError(ExitCode.WRITE_ERROR, "post-edge evidence failed"),
        )
    assert exit_status.exit_code(ledger) == ExitCode.BATCH_FAILURE


def test_internal_item_error_outranks_an_additional_batch_write_error() -> None:
    """A later report failure cannot mask a retained programming failure."""
    ledger = _ledger(ItemStatus.FAILED, AppError(ExitCode.INTERNAL_ERROR, "invariant"))
    ledger.batch_error = AppError(ExitCode.WRITE_ERROR, "report spool failed")
    assert exit_status.exit_code(ledger) == ExitCode.INTERNAL_ERROR


def test_ordinary_batch_write_error_keeps_its_write_exit_code() -> None:
    """The precedence repair does not promote a report write failure to internal."""
    ledger = _ledger(ItemStatus.CREATED)
    ledger.batch_error = AppError(ExitCode.WRITE_ERROR, "report spool failed")
    assert exit_status.exit_code(ledger) == ExitCode.WRITE_ERROR


def test_render_and_application_errors_preserve_the_selected_channel(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    class Parser:
        @staticmethod
        def print_usage(stream: TextIO) -> None:
            stream.write("usage: remove-eml-attachments\n")

    usage = AppError(ExitCode.USAGE, "bad input")
    assert cli._render_error(usage, Parser()) == ExitCode.USAGE  # ruff: ignore[private-member-access] - output boundary contract.
    assert cli._render_error(usage, None) == ExitCode.USAGE  # ruff: ignore[private-member-access] - output boundary contract.
    assert "usage: remove-eml-attachments" in capsys.readouterr().err

    seen: list[str] = []
    monkeypatch.setattr(cli, "_json_error", _record_json_error(seen))
    monkeypatch.setattr(cli, "_render_error", _record_human_error(seen))
    assert cli._application_error(["--output-format=json"], usage) == 2  # ruff: ignore[private-member-access] - output boundary contract.
    assert cli._application_error([], usage) == 2  # ruff: ignore[private-member-access] - output boundary contract.
    assert seen == ["json", "human"]


def test_dispatch_preserves_every_expected_exception_class(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    state = cli._RunState()  # ruff: ignore[private-member-access] - dispatch state contract.
    cancellation = CancellationSignal(2, "SIGINT")
    cases: tuple[BaseException, ...] = (
        cancellation,
        AppError(ExitCode.PARSE_ERROR, "bad"),
        KeyboardInterrupt(),
        BrokenPipeError(),
        OSError("unexpected"),
    )
    results: list[int] = []
    monkeypatch.setattr(cli, "_cancelled", lambda *_args: 130)
    monkeypatch.setattr(cli, "_application_error", lambda *_args: 5)
    monkeypatch.setattr(cli, "_render_error", lambda *_args: 130)
    monkeypatch.setattr(cli, "_internal_error", lambda _error: 70)
    for error in cases:
        monkeypatch.setattr(cli, "_run", lambda *_args, error=error: _raise(error))
        results.append(cli._dispatch(["source.eml"]))  # ruff: ignore[private-member-access] - exception precedence contract.
    assert results == [130, 5, 130, 1, 70]
    assert state.ledger is None


def test_cancelled_renders_preledger_json_and_human_terminal_ledgers(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    signal = CancellationSignal(15, "SIGTERM")
    empty = cli._RunState()  # ruff: ignore[private-member-access] - cancellation state contract.
    monkeypatch.setattr(cli, "_render_error", lambda *_args: 130)
    assert cli._cancelled(empty, signal) == 130  # ruff: ignore[private-member-access] - cancellation state contract.

    calls: list[str] = []
    monkeypatch.setattr(
        report_stream, "write_json", lambda *_args: calls.append("json")
    )
    monkeypatch.setattr(
        report_stream, "write_human", lambda _ledger: calls.append("human")
    )
    monkeypatch.setattr(
        report_stream, "write_paths0", lambda _ledger: calls.append("paths0")
    )
    for output_format in ("json", "human", "paths0"):
        with ExitStack() as resources:
            session = open_session(resources, output_format)
            active = cli._RunState(_ledger(ItemStatus.CREATED), session)  # ruff: ignore[private-member-access] - cancellation state contract.
            assert cli._cancelled(active, signal) == 130  # ruff: ignore[private-member-access] - cancellation state contract.
    # The retained request, never a guess from raw argv, selects the channel.
    assert calls == ["json", "human", "paths0"]


def test_internal_error_and_selected_outputs_cover_all_channels(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rendered: list[AppError] = []
    monkeypatch.setattr(cli, "_render_error", _record_error(rendered))
    assert cli._internal_error(Exception()) == 70  # ruff: ignore[private-member-access] - internal boundary contract.
    assert rendered[0].message == "Exception"

    ledger = _ledger(ItemStatus.CREATED)
    channels: list[str] = []
    monkeypatch.setattr(report_session, "report", lambda *_args: {"items": []})
    monkeypatch.setattr(
        report_session, "write_json", lambda _document: channels.append("json")
    )
    monkeypatch.setattr(
        report_session, "write_paths0", lambda _ledger: channels.append("paths0")
    )
    monkeypatch.setattr(
        report_session, "write_human", lambda _document: channels.append("human")
    )
    for output_format in ("json", "paths0", "human"):
        report_session._write_selected(output_format, ledger, "apply", 0)  # ruff: ignore[private-member-access] - selected-channel contract.
    assert channels == ["json", "paths0", "human"]


def _raise(error: BaseException) -> None:
    raise error


def _record_json_error(seen: list[str]) -> object:
    def record(
        _raw: list[str], _error: AppError, _intent: object = None, **_kwargs: object
    ) -> int:
        seen.append("json")
        return 2

    return record


def _record_human_error(seen: list[str]) -> object:
    def record(_error: AppError, _parser: object) -> int:
        seen.append("human")
        return 2

    return record


def _record_error(rendered: list[AppError]) -> object:
    def record(error: AppError, _parser: object) -> int:
        rendered.append(error)
        return 70

    return record


@pytest.mark.parametrize(
    "case",
    [
        (False, True, False, False),
        (True, False, False, False),
        (True, True, True, False),
        (True, True, False, True),
    ],
)
def test_cancelled_publishes_only_a_ledger_with_an_undelivered_session(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    case: tuple[bool, bool, bool, bool],
) -> None:
    """Any missing report owner, or a delivery already begun, means diagnostic only."""
    has_ledger, has_session, started, publishes = case
    published: list[int] = []

    def publish(
        _self: report_session.ReportSession, _ledger: BatchLedger, status: int
    ) -> int:
        published.append(status)
        return 0

    monkeypatch.setattr(report_session.ReportSession, "publish", publish)
    with ExitStack() as resources:
        session = open_session(resources) if has_session else None
        if session is not None:
            session.delivery_started = started
        ledger = _ledger(ItemStatus.CREATED) if has_ledger else None
        state = cli._RunState(ledger, session)  # ruff: ignore[private-member-access] - cancellation state contract.
        status = cli._cancelled(state, CancellationSignal(2, "SIGINT"))  # ruff: ignore[private-member-access] - cancellation state contract.
    if publishes:
        assert (status, published) == (0, [130])
        assert not capsys.readouterr().err
    else:
        assert (status, published) == (130, [])
        assert (
            capsys.readouterr().err
            == "remove-eml-attachments: error[INTERRUPTED:130]: interrupted by SIGINT\n"
        )
