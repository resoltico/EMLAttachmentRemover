"""CLI terminal-report staging, delivery, and cleanup contracts."""

from __future__ import annotations

import json
import sys
import tempfile
from contextlib import ExitStack
from io import BytesIO, StringIO, TextIOWrapper
from pathlib import Path
from types import SimpleNamespace
from typing import cast, override

import pytest

from eml_attachment_remover import (
    cli,
    report_delivery,
    report_session,
    report_spool,
    report_stream,
    staged_output,
)
from eml_attachment_remover.cancellation import CancellationSignal, DeliveryGuard
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value
from tests.report_session_support import complete_owned, open_session


def _failed() -> BatchLedger:
    """Build one terminal ledger eligible for a private report spool.

    Returns:
        One terminal failed ledger.

    """
    ledger = BatchLedger.from_requests([path_value("source.eml")])
    ledger.items[0].finish(ItemStatus.FAILED, AppError(ExitCode.PARSE_ERROR, "bad"))
    return ledger


def _spooled() -> BatchLedger:
    """Build a terminal ledger whose record already lives in a private spool.

    Returns:
        The archived ledger; callers release it.

    """
    ledger = _failed()
    report_stream.start(ledger)
    report_stream.archive_all(ledger)
    return ledger


def test_staging_targets_each_final_channel_encoding(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Human text escapes for its real channel; byte channels are staged unchanged."""
    calls: list[dict[str, object]] = []

    def temporary_file(**kwargs: object) -> StringIO:
        calls.append(kwargs)
        return StringIO()

    monkeypatch.setattr(tempfile, "TemporaryFile", temporary_file)
    monkeypatch.setattr(report_delivery, "private_temp_root", lambda: Path("/p"))
    monkeypatch.setattr(sys, "stdout", SimpleNamespace(encoding="ascii"))
    monkeypatch.setattr(sys, "stderr", SimpleNamespace(encoding="latin-1"))
    with ExitStack() as resources:
        for output_format in ("human", "json", "paths0"):
            report_delivery.StagedChannels.open(resources, output_format)
    expected = [
        (encoding, "latin-1")
        for encoding in ("ascii", report_delivery.BYTE_CHANNEL, "utf-8")
    ]
    assert [
        (calls[index]["encoding"], calls[index + 1]["encoding"])
        for index in range(0, 6, 2)
    ] == expected
    assert all(
        (call["mode"], call["newline"], call["dir"]) == ("w+", "", str(Path("/p")))
        for call in calls
    )


def test_unknown_channel_encoding_stages_utf8() -> None:
    """An in-memory stand-in without an encoding keeps the historic UTF-8 staging."""
    assert report_delivery._encoding(StringIO()) == "utf-8"  # ruff: ignore[private-member-access] - fallback codec.
    assert report_delivery._encoding(object()) == "utf-8"  # ruff: ignore[private-member-access] - no declared codec.
    assert report_delivery._encoding(SimpleNamespace(encoding="cp1252")) == "cp1252"  # ruff: ignore[private-member-access] - real codec.


def test_copy_is_chunked_flushed_and_stamped() -> None:
    """Large staged reports are replayed in bounded, flushed, progress-stamped reads."""
    reads: list[int | None] = []

    class Source(BytesIO):
        """A two-read source that records the bounded read request."""

        @override
        def read(self, size: int | None = -1) -> bytes:
            reads.append(size)
            return b"x" if len(reads) == 1 else b""

    class Sink(BytesIO):
        """A byte sink that records each flush."""

        flushes = 0

        @override
        def flush(self) -> None:
            type(self).flushes += 1

    guard = DeliveryGuard()
    guard.progress = 0.0
    destination = Sink()
    report_delivery._copy_chunks(Source(), destination, guard)  # ruff: ignore[private-member-access] - bounded replay contract.
    assert reads == [report_delivery.CHUNK_SIZE] * 2
    assert (destination.getvalue(), Sink.flushes) == (b"x", 1)
    assert guard.progress > 0.0


@pytest.mark.parametrize(
    ("output_format", "written", "expected", "binary"),
    [
        ("paths0", b"one\0", b"one\0", True),
        ("json", b'{"ok":true}\n', b'{"ok":true}\n', True),
        ("human", "human\n", b"human\n", False),
    ],
)
def test_delivery_replays_one_staged_report_on_its_channel(
    output_format: str,
    written: bytes | str,
    expected: bytes,
    monkeypatch: pytest.MonkeyPatch,
    *,
    binary: bool,
) -> None:
    """A partial earlier render is discarded; only the final render is delivered."""
    captured = BytesIO()
    text = StringIO()

    class Output:
        """Real-output boundary exposing text and binary channels."""

        buffer = captured
        encoding = "utf-8"

        @staticmethod
        def write(value: str) -> int:
            return text.write(value)

        @staticmethod
        def flush() -> None:
            return None

    def render(*_args: object) -> None:
        if isinstance(written, bytes):
            sys.stdout.buffer.write(written)
        else:
            sys.stdout.write(written)

    ledger = _spooled()
    monkeypatch.setattr(sys, "stdout", Output())
    monkeypatch.setattr(report_session, "_write_selected", render)
    with ExitStack() as resources:
        session = open_session(resources, output_format)
        session.channels.out.write("stale partial render")
        assert session.publish(ledger, 4) == 4
    # Byte channels reach the binary buffer only; human text reaches the text stream.
    received = (captured.getvalue(), text.getvalue().encode())
    assert received == ((expected, b"") if binary else (b"", expected))
    assert session.delivery_started
    assert ledger.report_spool is None


@pytest.mark.parametrize("mode", ["apply", "dry-run"])
def test_staging_failure_recovers_the_reserved_status_report(
    mode: str, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """A full disk during staging still delivers the outcome of published items."""
    ledger = _spooled()
    original = report_session._write_selected  # ruff: ignore[private-member-access] - real renderer.
    attempts: list[bool] = []

    def fail_once(*args: object) -> None:
        attempts.append(ledger.report_spool_failed)
        if not attempts[1:]:
            message = "synthetic no space"
            raise OSError(message)
        original(*args)  # type: ignore[arg-type]

    monkeypatch.setattr(report_session, "_write_selected", fail_once)
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json", mode)
            status = session.publish(ledger, 5)
        document = json.loads(capsys.readouterr().out)
    finally:
        report_stream.close(ledger)
    assert attempts == [False]
    assert status == int(ExitCode.WRITE_ERROR) == document["exit_code"]
    assert document["mode"] == mode
    assert document["batch_error"]["message"] == "terminal report spool failed"
    assert [item["status"] for item in document["items"]] == ["failed"]
    assert session.delivery_started


def test_released_diagnoses_cleanup_failure_without_rewriting_status(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """Spool cleanup after rendering reports its failure but keeps the run's status."""
    ledger = _failed()

    def fail_close(_ledger: BatchLedger) -> None:
        message = "synthetic cleanup failure"
        raise report_spool.ReportSpoolError(message)

    monkeypatch.setattr(report_stream, "close", fail_close)
    state = cli._RunState(ledger)  # ruff: ignore[private-member-access] - cleanup state.
    assert cli._released(state, 5) == 5  # ruff: ignore[private-member-access] - cleanup contract.
    assert cli._released(cli._RunState(), 3) == 3  # ruff: ignore[private-member-access] - no ledger to release.
    assert capsys.readouterr().err.endswith(
        "error[INTERNAL_ERROR:70]: report cleanup failed: synthetic cleanup failure\n"
    )


def test_dispatch_releases_spools_after_every_rendering_attempt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Failed or cancelled runs never leak a private terminal report spool."""
    ledger = _spooled()
    primary = cast("report_spool.ReportSpool", ledger.report_spool)
    emergency = cast("report_spool.ReportSpool", ledger.emergency_report_spool)

    def fail(_raw: list[str], state: cli._RunState, _resources: ExitStack) -> int:
        state.ledger = ledger
        message = "synthetic late failure"
        raise RuntimeError(message)

    monkeypatch.setattr(cli, "_run", fail)
    monkeypatch.setattr(cli, "_render_error", lambda *_args: 70)
    assert cli._dispatch(["source.eml"]) == 70  # ruff: ignore[private-member-access] - release contract.
    assert ledger.report_spool is None
    assert primary.file.closed
    assert emergency.file.closed


def test_stage_construction_reraises_one_failure_without_a_wrapper() -> None:
    """A sole staging-construction failure preserves its original identity."""
    failure = OSError("one construction failure")
    with pytest.raises(OSError, match="one construction failure") as raised:
        staged_output._raise_stage_construction_failures([failure])  # ruff: ignore[private-member-access] - sole-error construction contract.
    assert raised.value is failure


def test_cancellation_keeps_an_interruption_recorded_during_processing(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A later report-phase signal never rewrites where the batch was interrupted."""
    ledger = _failed()
    ledger.record_interruption("SIGTERM", "processing")
    with ExitStack() as resources:
        state = cli._RunState(ledger, open_session(resources, "json"))  # ruff: ignore[private-member-access] - earlier interruption.
        assert cli._cancelled(state, CancellationSignal(2, "SIGINT")) == 130  # ruff: ignore[private-member-access] - report-phase signal.
    assert json.loads(capsys.readouterr().out)["interruption"] == {
        "signal": "SIGTERM",
        "reason": "interrupted by SIGTERM",
        "phase": "processing",
    }


@pytest.mark.parametrize(
    ("arguments", "mode", "staged_encoding"),
    [
        (["--output-format=human"], "apply", "ascii"),
        (["--dry-run", "--output-format=human"], "dry-run", "ascii"),
        (["--output-format=json"], "apply", report_delivery.BYTE_CHANNEL),
        (["--output-format=paths0"], "apply", report_delivery.BYTE_CHANNEL),
    ],
)
def test_run_retains_the_request_and_stages_for_its_channel(
    arguments: list[str],
    mode: str,
    staged_encoding: str,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The retained mode and format decide staging before any item can publish."""
    for name in ("stdout", "stderr"):
        monkeypatch.setattr(sys, name, TextIOWrapper(BytesIO(), encoding="ascii"))
    monkeypatch.setattr(
        cli,
        "execute",
        lambda *_args, ledger, **_kwargs: complete_owned(ledger, _failed()),
    )
    monkeypatch.setattr(report_session, "_write_selected", lambda *_args: None)
    state = cli._RunState()  # ruff: ignore[private-member-access] - retained request.
    with ExitStack() as resources:
        cli._run([*arguments, "source.eml"], state, resources)  # ruff: ignore[private-member-access] - request retention.
        assert state.session is not None
        assert state.session.channels.out.encoding == staged_encoding
    assert state.session.mode == mode
