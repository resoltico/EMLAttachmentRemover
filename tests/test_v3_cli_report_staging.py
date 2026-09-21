"""CLI terminal-report staging and cleanup contracts."""

from __future__ import annotations

import sys
import tempfile
from io import BytesIO, StringIO
from pathlib import Path
from typing import cast

import pytest

from eml_attachment_remover import cli, report_spool, report_stream, staged_output
from eml_attachment_remover.batch import BatchOptions
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value


def _failed() -> BatchLedger:
    """Build one terminal ledger eligible for a private report spool.

    Returns:
        One terminal failed ledger.

    """
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    return ledger


def _options() -> BatchOptions:
    """Build the canonical ordinary report options.

    Returns:
        Stable non-dry-run options.

    """
    return BatchOptions(
        dry_run=False, existing="error", fail_fast=False, output=None, output_dir=None
    )


def test_staged_files_are_explicitly_private_utf8_text_files(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The cleanup barrier never inherits encoding, newline, or temp-root policy."""
    calls: list[dict[str, object]] = []

    class StagedText(StringIO):
        """Text staging double exposing the binary replay channel."""

        buffer = BytesIO()

    def temporary_file(**kwargs: object) -> StagedText:
        calls.append(kwargs)
        return StagedText()

    ledger = _failed()
    ledger.report_spool = object()
    private_root = Path("/private")
    monkeypatch.setattr(tempfile, "TemporaryFile", temporary_file)
    monkeypatch.setattr(cli, "private_temp_root", lambda: private_root)
    monkeypatch.setattr(cli, "_write_selected", lambda *_args: None)
    monkeypatch.setattr(report_stream, "close", lambda _ledger: None)
    monkeypatch.setattr(cli, "_copy_text", lambda *_args: None)
    monkeypatch.setattr(cli, "_copy_bytes", lambda *_args: None)
    cli._write_then_close("json", ledger, _options(), 0)  # ruff: ignore[private-member-access] - exact temporary-file policy.
    assert calls == [
        {"mode": "w+", "encoding": "utf-8", "newline": "", "dir": private_root},
        {"mode": "w+", "encoding": "utf-8", "newline": "", "dir": private_root},
    ]


def test_copy_chunks_and_paths0_binary_replay_are_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Large staged reports remain bounded and NUL-delimited output stays binary."""
    reads: list[int | None] = []

    class Source:
        """A two-read source that records the bounded read request."""

        @staticmethod
        def read(size: int | None) -> str:
            reads.append(size)
            return "x" if len(reads) == 1 else ""

    destination = StringIO()
    cli._copy_text(Source(), destination)  # ruff: ignore[private-member-access] - bounded replay contract.
    assert reads == [1024 * 1024, 1024 * 1024]
    assert destination.getvalue() == "x"

    byte_reads: list[int | None] = []

    class ByteSource:
        """Binary counterpart of the bounded report source."""

        @staticmethod
        def read(size: int | None) -> bytes:
            byte_reads.append(size)
            return b"x" if len(byte_reads) == 1 else b""

    byte_destination = BytesIO()
    cli._copy_bytes(ByteSource(), byte_destination)  # ruff: ignore[private-member-access] - bounded binary replay contract.
    assert byte_reads == [1024 * 1024, 1024 * 1024]
    assert byte_destination.getvalue() == b"x"

    ledger = _failed()
    ledger.report_spool = object()
    captured = BytesIO()

    class Output:
        """Minimal real-output boundary for paths0 replay."""

        buffer = captured

        @staticmethod
        def write(_text: str) -> int:
            return 0

    monkeypatch.setattr(sys, "stdout", Output())
    monkeypatch.setattr(
        cli, "_write_selected", lambda *_args: sys.stdout.buffer.write(b"one\0")
    )
    monkeypatch.setattr(report_stream, "close", lambda _ledger: None)
    cli._write_then_close("paths0", ledger, _options(), 0)  # ruff: ignore[private-member-access] - binary paths0 replay.
    assert captured.getvalue() == b"one\0"

    captured.seek(0)
    captured.truncate(0)
    monkeypatch.setattr(
        cli, "_write_selected", lambda *_args: sys.stdout.buffer.write(b'{"ok":true}\n')
    )
    cli._write_then_close("json", ledger, _options(), 0)  # ruff: ignore[private-member-access] - binary JSON replay.
    assert captured.getvalue() == b'{"ok":true}\n'

    human = StringIO()
    monkeypatch.setattr(sys, "stdout", human)
    monkeypatch.setattr(
        cli, "_write_selected", lambda *_args: sys.stdout.write("human\n")
    )
    cli._write_then_close("human", ledger, _options(), 0)  # ruff: ignore[private-member-access] - text human replay.
    assert human.getvalue() == "human\n"


def test_cancelled_constructs_the_canonical_terminal_options(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Signal reporting must retain the public human mode and stable option facts."""
    ledger = _failed()
    captured: list[tuple[object, ...]] = []
    monkeypatch.setattr(cli, "_write_then_close", lambda *args: captured.append(args))
    status = cli._cancelled([], cli._RunState(ledger), CancellationSignal(2, "SIGINT"))  # ruff: ignore[private-member-access] - signal terminal contract.
    assert status == 130
    output_format, received_ledger, options, received_status = captured[0]
    assert (output_format, received_ledger, received_status) == ("human", ledger, 130)
    assert options == _options()


def test_stage_construction_reraises_one_failure_without_a_wrapper() -> None:
    """A sole staging-construction failure preserves its original identity."""
    failure = OSError("one construction failure")
    with pytest.raises(OSError, match="one construction failure") as raised:
        staged_output._raise_stage_construction_failures([failure])  # ruff: ignore[private-member-access] - sole-error construction contract.
    assert raised.value is failure


@pytest.mark.parametrize("failure_point", ["writer", "second-temporary-file"])
def test_spooled_failures_always_release_both_private_report_owners(
    failure_point: str,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    """No rendering or staging failure may leak a private terminal report spool."""
    ledger = _failed()
    report_stream.start(ledger)
    report_stream.archive_all(ledger)
    primary = cast("report_spool.ReportSpool", ledger.report_spool)
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)
    owned_paths = (primary.path, emergency.path)
    if failure_point == "writer":
        message = "synthetic render failure"

        def fail_writer(*_args: object) -> None:
            raise OSError(message)

        monkeypatch.setattr(cli, "_write_selected", fail_writer)
    else:
        original = tempfile.TemporaryFile
        allocations = 0

        def temporary_file(
            *,
            mode: str,
            encoding: str | None,
            newline: str | None,
            dir: Path | None,  # ruff: ignore[builtin-argument-shadowing] - mirrors TemporaryFile's public keyword.
        ) -> object:
            nonlocal allocations
            allocations += 1
            if allocations == 2:
                message = "synthetic staging failure"
                raise OSError(message)
            return original(mode=mode, encoding=encoding, newline=newline, dir=dir)

        monkeypatch.setattr(tempfile, "TemporaryFile", temporary_file)
    with pytest.raises(OSError, match=r"synthetic (render|staging) failure"):
        cli._write_then_close(  # ruff: ignore[private-member-access] - unconditional private cleanup contract.
            "json", ledger, _options(), 1
        )
    assert ledger.report_spool is None
    assert ledger.emergency_report_spool is None
    assert all(not path.exists() for path in owned_paths)
    assert not capsys.readouterr().out
