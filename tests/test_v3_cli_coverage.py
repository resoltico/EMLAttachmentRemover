"""Terminal CLI precedence, error rendering, and channel-selection contracts."""

from __future__ import annotations

from typing import TYPE_CHECKING, TextIO

from eml_attachment_remover import cli
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    import pytest


def _ledger(status: ItemStatus, error: AppError | None = None) -> BatchLedger:
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(status, error)
    return ledger


def test_exit_code_prioritizes_publication_and_single_failure_facts() -> None:
    publication_error = AppError(ExitCode.INTERNAL_ERROR, "sync")
    published = _ledger(ItemStatus.PUBLISHED_WITH_ERROR, publication_error)
    assert cli.exit_code(published) == ExitCode.INTERNAL_ERROR

    incomplete = _ledger(
        ItemStatus.PUBLISHED_WITH_ERROR, AppError(ExitCode.WRITE_ERROR, "x")
    )
    assert cli.exit_code(incomplete) == ExitCode.PUBLICATION_INCOMPLETE

    failed_without_error = _ledger(ItemStatus.FAILED)
    assert cli.exit_code(failed_without_error) == ExitCode.INTERNAL_ERROR

    failed_parse = _ledger(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    assert cli.exit_code(failed_parse) == ExitCode.PARSE_ERROR

    failed_internal = _ledger(
        ItemStatus.FAILED,
        AppError(ExitCode.INTERNAL_ERROR, "unexpected"),
    )
    assert cli.exit_code(failed_internal) == ExitCode.INTERNAL_ERROR

    interrupted = _ledger(ItemStatus.CREATED)
    interrupted.record_interruption("SIGTERM", "report")
    assert cli.exit_code(interrupted) == ExitCode.INTERRUPTED


def test_exit_code_uses_batch_failure_for_multiple_terminal_failures() -> None:
    ledger = BatchLedger.from_requests([path_value("one.eml"), path_value("two.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    ledger.items[1].finish(ItemStatus.CREATED)
    assert cli.exit_code(ledger) == ExitCode.BATCH_FAILURE


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
    assert cli._cancelled([], empty, signal) == 130  # ruff: ignore[private-member-access] - cancellation state contract.

    ledger = _ledger(ItemStatus.CREATED)
    active = cli._RunState(ledger)  # ruff: ignore[private-member-access] - cancellation state contract.
    calls: list[str] = []
    monkeypatch.setattr(cli, "write_json", lambda _document: calls.append("json"))
    monkeypatch.setattr(cli, "write_human", lambda _document: calls.append("human"))
    assert cli._cancelled(["--output-format=json"], active, signal) == 130  # ruff: ignore[private-member-access] - cancellation state contract.
    assert cli._cancelled([], active, signal) == 130  # ruff: ignore[private-member-access] - cancellation state contract.
    assert calls == ["json", "human"]


def test_internal_error_and_selected_outputs_cover_all_channels(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    rendered: list[AppError] = []
    monkeypatch.setattr(cli, "_render_error", _record_error(rendered))
    assert cli._internal_error(Exception()) == 70  # ruff: ignore[private-member-access] - internal boundary contract.
    assert rendered[0].message == "Exception"

    ledger = _ledger(ItemStatus.CREATED)
    options = BatchOptions(
        dry_run=False,
        existing="error",
        fail_fast=False,
        output=None,
        output_dir=None,
    )
    channels: list[str] = []
    monkeypatch.setattr(cli, "report", lambda *_args: {"items": []})
    monkeypatch.setattr(cli, "write_json", lambda _document: channels.append("json"))
    monkeypatch.setattr(cli, "write_paths0", lambda _ledger: channels.append("paths0"))
    monkeypatch.setattr(cli, "write_human", lambda _document: channels.append("human"))
    for output_format in ("json", "paths0", "human"):
        cli._write_selected(output_format, ledger, options, 0)  # ruff: ignore[private-member-access] - selected-channel contract.
    assert channels == ["json", "paths0", "human"]


def _raise(error: BaseException) -> None:
    raise error


def _record_json_error(seen: list[str]) -> object:
    def record(_raw: list[str], _error: AppError) -> int:
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
