"""The report lifecycle: sealed staging, recovery, bounded delivery."""

from __future__ import annotations

import json
import os
import signal
import sys
import threading
from contextlib import ExitStack
from io import BytesIO, TextIOWrapper
from typing import TYPE_CHECKING, override

import pytest

from eml_attachment_remover import (
    cancellation,
    report_delivery,
    report_session,
    report_stream,
)
from eml_attachment_remover.cancellation import DeliveryGuard
from eml_attachment_remover.domain import AppError, BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value
from eml_attachment_remover.report_delivery import StagedChannels
from eml_attachment_remover.report_session import ReportSession
from tests.report_session_support import open_session

if TYPE_CHECKING:
    from typing import TextIO


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


class _Faulty:
    """A staging file that fails one named operation on one numbered call."""

    def __init__(self, real: TextIO, operation: str, call: int) -> None:
        self.real = real
        self.operation = operation
        self.remaining = call
        self.buffer = real.buffer

    def __getattr__(self, name: str) -> object:
        target = getattr(self.real, name)
        if name != self.operation:
            return target

        def call(*args: object) -> object:
            self.remaining -= 1
            if self.remaining == 0:
                message = f"synthetic {name} failure"
                raise OSError(message)
            return target(*args)

        return call


@pytest.mark.parametrize(
    ("operation", "call"),
    [("flush", 1), ("seek", 2), ("read", 1), ("truncate", 1)],
)
def test_every_step_before_the_first_byte_can_recover(
    operation: str, call: int, capsys: pytest.CaptureFixture[str]
) -> None:
    """Flush, rewind, first read, and reset failures all fall back to recovery."""
    ledger = _spooled()
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json")
            channels = StagedChannels(
                _Faulty(session.channels.out, operation, call),  # type: ignore[arg-type]
                session.channels.err,
            )
            faulty = ReportSession(channels, "json", "apply")
            status = faulty.publish(ledger, 5)
        document = json.loads(capsys.readouterr().out)
    finally:
        report_stream.close(ledger)
    assert status == document["exit_code"] == int(ExitCode.WRITE_ERROR)
    assert document["batch_error"]["message"] == "terminal report spool failed"
    assert faulty.delivery_started


def test_recovery_json_stays_ascii_bytes_on_a_utf16_terminal(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Recovery never lets the terminal codec choose the machine-readable bytes."""
    raw = BytesIO()
    terminal = TextIOWrapper(raw, encoding="utf-16", newline="")
    monkeypatch.setattr(sys, "stdout", terminal)
    ledger = _spooled()
    original = report_session._write_selected  # ruff: ignore[private-member-access] - real renderer.
    attempts: list[int] = []

    def fail_once(*args: object) -> None:
        attempts.append(1)
        if len(attempts) == 1:
            message = "synthetic no space"
            raise OSError(message)
        original(*args)  # type: ignore[arg-type]

    monkeypatch.setattr(report_session, "_write_selected", fail_once)
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json")
            assert session.publish(ledger, 5) == int(ExitCode.WRITE_ERROR)
    finally:
        report_stream.close(ledger)
    document = json.loads(raw.getvalue().decode("ascii"))
    assert document["exit_code"] == int(ExitCode.WRITE_ERROR)
    assert not raw.getvalue().startswith((b"\xff\xfe", b"\xfe\xff"))


@pytest.mark.parametrize("failed_already", [False, True])
def test_a_render_failure_without_usable_reserved_evidence_is_not_masked(
    capsys: pytest.CaptureFixture[str], *, failed_already: bool
) -> None:
    """Only a run with unused reserved records may fall back; otherwise it raises."""
    ledger = _failed()
    if failed_already:
        report_stream.start(ledger)
        ledger.report_spool_failed = True
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json")
            channels = StagedChannels(
                _Faulty(session.channels.out, "flush", 1),  # type: ignore[arg-type]
                session.channels.err,
            )
            with pytest.raises(OSError, match="synthetic flush failure"):
                ReportSession(channels, "json", "apply").publish(ledger, 5)
    finally:
        report_stream.close(ledger)
    assert not capsys.readouterr().out


def test_unspooled_ledgers_are_delivered_through_staging(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Without a spool the in-memory report still reaches its channel via staging."""
    with ExitStack() as resources:
        session = open_session(resources)
        assert session.publish(_failed(), 5) == 5
    captured = capsys.readouterr()
    assert captured.out == "failed: source.eml\n"
    assert captured.err == "source.eml: PARSE_ERROR: bad\n"
    assert session.delivery_started


def test_a_signal_during_delivery_is_reported_after_the_complete_document(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The document is whole and unchanged; the process says it was interrupted."""
    ledger = _spooled()
    sent: list[int] = []

    class Output:
        """A stdout whose first accepted chunk delivers SIGINT."""

        encoding = "utf-8"
        buffer = BytesIO()

        @classmethod
        def write(cls, value: str) -> int:
            return len(value)

        @classmethod
        def flush(cls) -> None:
            return None

    original_write = Output.buffer.write

    def write(chunk: bytes) -> int:
        if not sent:
            sent.append(1)
            signal.raise_signal(signal.SIGINT)
        return original_write(chunk)

    monkeypatch.setattr(Output.buffer, "write", write, raising=False)
    monkeypatch.setattr(sys, "stdout", Output())
    with ExitStack() as resources:
        session = open_session(resources, "json")
        status = session.publish(ledger, 0)
    document = json.loads(Output.buffer.getvalue())
    assert (status, document["exit_code"], document["interrupted"]) == (130, 0, False)
    assert capsys.readouterr().err.endswith(
        "error[INTERRUPTED:130]: interrupted by SIGINT after the report was delivered\n"
    )


def test_a_stalled_consumer_is_abandoned_once_the_grace_is_spent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A reader that stops draining cannot keep a cancelled process alive."""
    release = threading.Event()
    exits: list[int] = []

    class Stalled(BytesIO):
        """A byte sink whose write never returns until the process is ended."""

        @override
        def write(self, _chunk: object) -> int:
            release.wait(30)
            return 0

    class Output:
        """Standard output over the stalled sink."""

        encoding = "utf-8"
        buffer = Stalled()

    def hard_exit(status: int) -> None:
        exits.append(status)
        release.set()

    monkeypatch.setattr(cancellation, "GRACE_SECONDS", 0.2)
    monkeypatch.setattr(report_delivery, "POLL_SECONDS", 0.02)
    monkeypatch.setattr(cancellation, "hard_exit", hard_exit)
    monkeypatch.setattr(sys, "stdout", Output())
    ledger = _spooled()
    timer = threading.Timer(0.1, os.kill, (os.getpid(), signal.SIGTERM))
    timer.start()
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json")
            session.publish(ledger, 0)
    finally:
        timer.cancel()
    assert exits == [130]


def test_a_closed_pipe_during_delivery_reaches_the_caller(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A consumer that went away is a delivery failure, not a silent success."""

    class Closed(BytesIO):
        """A byte sink whose reader has gone."""

        @override
        def write(self, _chunk: object) -> int:
            raise BrokenPipeError

    class Output:
        """Standard output over the closed sink."""

        encoding = "utf-8"
        buffer = Closed()

    monkeypatch.setattr(sys, "stdout", Output())
    ledger = _spooled()
    with ExitStack() as resources:
        session = open_session(resources, "json")
        with pytest.raises(BrokenPipeError):
            session.publish(ledger, 0)


def test_write_note_is_flushed_stamped_and_bounded(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A late diagnostic goes through the same watched thread as the report."""
    guard = DeliveryGuard()
    before = guard.progress
    report_delivery.write_note("note\n", guard)
    assert capsys.readouterr().err == "note\n"
    assert guard.progress >= before
