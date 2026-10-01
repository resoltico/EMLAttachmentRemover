"""CLI interruption after an error handler retires stays short and preserves bytes."""

from __future__ import annotations

import inspect
import json
import signal
import sys
import tempfile
import threading
from contextlib import contextmanager
from io import BytesIO, StringIO
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    batch,
    cancellation,
    cli,
    report_delivery,
    report_stream,
)
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.cancellation_state import CURRENT
from tests.live_report_support import MESSAGE, inputs
from tests.trace_implementation_support import traced_implementation


def test_interruption_notice_uses_utf8_for_an_inmemory_diagnostic_channel(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    diagnostics = StringIO()

    def requested(*_args: object) -> int:
        raise CancellationSignal(2, "SIGINT")

    monkeypatch.setattr(sys, "stderr", diagnostics)
    monkeypatch.setattr(cli, "_run", requested)
    assert cli.main([]) == 130
    assert diagnostics.getvalue() == (
        "remove-eml-attachments: error[INTERRUPTED:130]: interrupted by SIGINT\n"
    )


def test_startup_interruption_is_not_diagnosed_again_after_cleanup_signal(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def cancelled(*_args: object) -> object:
        raise KeyboardInterrupt

    original = report_stream.close
    calls: list[int] = []

    def closed(*args: object) -> None:
        original(*args)  # type: ignore[arg-type]
        calls.append(1)
        if len(calls) == 1:
            raise KeyboardInterrupt

    monkeypatch.setattr(report_delivery.StagedChannels, "open", cancelled)
    monkeypatch.setattr(report_stream, "close", closed)
    assert cli.main(["--output-format=json", "source.eml"]) == 130
    captured = capsys.readouterr()
    assert json.loads(captured.out)["interrupted"]
    assert not captured.err


def test_a_late_error_notice_is_not_repeated_if_its_write_is_interrupted(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    class Signalled(BytesIO):
        def write(self, payload: object) -> int:
            if not self.tell():
                signal.raise_signal(signal.SIGINT)
            return super().write(payload)  # type: ignore[arg-type]

    class Output:
        encoding = "utf-8"
        buffer = Signalled()

    original = report_delivery.write_note

    def written(*args: object) -> None:
        original(*args)  # type: ignore[arg-type]
        raise KeyboardInterrupt

    monkeypatch.setattr(sys, "stdout", Output())
    monkeypatch.setattr(cli, "write_note", written)
    assert cli.main(["--output-format=json"]) == 130
    assert json.loads(Output.buffer.getvalue().decode("ascii"))["exit_code"] == 2
    assert capsys.readouterr().err.count("interrupted by SIGINT") == 1


if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path
    from types import FrameType

    from _typeshed import TraceFunction

    from eml_attachment_remover.batch import BatchOptions
    from eml_attachment_remover.cancellation import DeliveryGuard
    from eml_attachment_remover.domain import BatchLedger


def test_a_reported_processing_interruption_is_not_diagnosed_again_at_cleanup(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    sources = inputs(tmp_path)
    original_execute, original_close = batch.execute, report_stream.close
    closes: list[int] = []

    def completed(
        sources: list[str], options: BatchOptions, *, ledger: BatchLedger
    ) -> BatchLedger:
        result = original_execute(sources, options, ledger=ledger)
        result.record_interruption("SIGINT", "report")
        return result

    def closed(ledger: BatchLedger) -> None:
        original_close(ledger)
        closes.append(1)
        if len(closes) == 2:
            raise KeyboardInterrupt

    monkeypatch.setattr(cli, "execute", completed)
    monkeypatch.setattr(report_stream, "close", closed)
    assert cli.main(["--output-format=json", *sources]) == 130
    captured = capsys.readouterr()
    assert json.loads(captured.out)["interrupted"]
    assert not captured.err


@contextmanager
def _interrupt(statement: str) -> Iterator[list[int]]:
    function = cli._json_error  # ruff: ignore[private-member-access] - JSON error response finalization.
    function = traced_implementation(function)
    lines, first = inspect.getsourcelines(function)
    target = next(
        first + index for index, line in enumerate(lines) if statement in line
    )
    sent: list[int] = []

    def trace(frame: FrameType, event: str, _argument: object) -> TraceFunction:
        if (
            event == "line"
            and frame.f_code is function.__code__
            and frame.f_lineno == target
            and not sent
        ):
            assert signal.getsignal(signal.SIGINT) == signal.default_int_handler
            sent.append(1)
            signal.raise_signal(signal.SIGINT)
        return trace

    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        yield sent
    finally:
        sys.settrace(previous)


@pytest.mark.parametrize("scenario", ["usage", "temporary_root"])
def test_real_post_retirement_interrupt_preserves_the_original_json_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    scenario: str,
) -> None:
    sources = inputs(tmp_path)
    arguments = ["--output-format=json", *sources]
    expected = 2
    if scenario == "usage":
        arguments.insert(0, "--unsupported-option")
    else:
        unsafe_root = tmp_path / "temporary-root-is-a-file"
        unsafe_root.write_text("public invalid temporary-root fixture")
        monkeypatch.setattr(tempfile, "tempdir", str(unsafe_root))
        expected = 7
    assert cli.main(arguments) == expected
    baseline = capsys.readouterr().out
    with _interrupt("status = guard.result(int(error.code))") as sent:
        assert cli.main(arguments) == 130
    captured = capsys.readouterr()
    assert sent == [1]
    assert captured.out == baseline
    assert json.loads(captured.out)["exit_code"] == expected
    assert (
        captured.err
        == "remove-eml-attachments: error[INTERRUPTED:130]: interrupted by SIGINT\n"
    )
    assert not list(tmp_path.glob("*.mime-pruned.eml"))
    assert all(
        (tmp_path / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
        for source in sources
    )
    assert CURRENT.get() is None
    assert signal.getsignal(signal.SIGINT) == signal.default_int_handler
    assert not any(
        thread.name == "eml-cancellation-monitor" for thread in threading.enumerate()
    )


def test_pre_delivery_interrupt_can_emit_one_startup_interruption_report(
    capsys: pytest.CaptureFixture[str],
) -> None:
    with _interrupt("with delivery_guard() as guard:") as sent:
        assert cli.main(["--output-format=json"]) == 130
    captured = capsys.readouterr()
    assert sent == [1]
    assert json.loads(captured.out)["interrupted"]
    assert not captured.err


def test_partial_error_document_is_never_restarted(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    prefix: list[bytes] = []

    def partial(
        channels: report_delivery.StagedChannels, _fmt: str, guard: DeliveryGuard
    ) -> None:
        chunk = channels.out.buffer.read(16)
        prefix.append(chunk)
        report_delivery._write_all(sys.stdout.buffer, chunk, guard, report=True)  # ruff: ignore[private-member-access] - real accepted output units.
        raise KeyboardInterrupt

    monkeypatch.setattr(report_delivery.StagedChannels, "deliver", partial)
    assert cli.main(["--output-format=json"]) == 130
    captured = capsys.readouterr()
    assert len(prefix) == 1
    assert captured.out.encode() == prefix[0]
    assert captured.err.count("interrupted by SIGINT") == 1


@pytest.mark.parametrize(
    "failure", [KeyboardInterrupt(), OSError("public closed diagnostic")]
)
def test_failed_notice_returns_interrupted_without_recursion(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    failure: BaseException,
) -> None:
    def failed(*_args: object) -> None:
        raise failure

    monkeypatch.setattr(cli, "write_note", failed)
    with _interrupt("status = guard.result(int(error.code))"):
        assert cli.main(["--output-format=json"]) == 130
    captured = capsys.readouterr()
    assert json.loads(captured.out)["exit_code"] == 2
    assert not captured.err


def test_failed_notice_formatting_also_preserves_interruption(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def failed(*_args: object) -> str:
        message = "public diagnostic formatting failure"
        raise ValueError(message)

    monkeypatch.setattr(cli, "error_line", failed)
    with _interrupt("status = guard.result(int(error.code))"):
        assert cli.main(["--output-format=json"]) == 130
    assert json.loads(capsys.readouterr().out)["exit_code"] == 2


def test_outer_cooperative_interruption_is_contained(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def interrupted(*_args: object) -> int:
        raise CancellationSignal(2, "SIGINT")

    monkeypatch.setattr(cli, "_guarded", interrupted)
    assert cli.main([]) == 130
    assert capsys.readouterr().err.count("interrupted by SIGINT") == 1


@pytest.mark.parametrize("fmt", ["json", "human"])
def test_an_enclosing_owner_request_is_not_counted_as_a_second_signal(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    fmt: str,
) -> None:
    exits: list[int] = []

    def requested(*_args: object) -> int:
        cancellation.checkpoint()
        return 0

    monkeypatch.setattr(cli, "_run", requested)
    monkeypatch.setattr(cancellation, "hard_exit", exits.append)
    with cancellation.install_cancellation_handlers():
        signal.raise_signal(signal.SIGINT)
        assert cli.main(["--output-format", fmt]) == 130
    captured = capsys.readouterr()
    assert not exits
    if fmt == "json":
        assert json.loads(captured.out)["interrupted"]
        assert not captured.err
    else:
        assert captured.err.count("interrupted by SIGINT") == 1


def test_interruption_during_release_does_not_repeat_an_existing_notice(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    def interrupted(*_args: object) -> int:
        raise CancellationSignal(2, "SIGINT")

    def release(_state: object, _status: int) -> int:
        signal.raise_signal(signal.SIGINT)
        return 0

    monkeypatch.setattr(cli, "_run", interrupted)
    monkeypatch.setattr(cli, "_released", release)
    assert cli.main([]) == 130
    captured = capsys.readouterr()
    assert not captured.out
    assert captured.err.count("interrupted by SIGINT") == 1
