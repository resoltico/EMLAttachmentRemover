"""Exact receipts for sealing, the watched delivery thread, and safe lines."""

from __future__ import annotations

import sys
import threading
import time
from io import BytesIO, TextIOWrapper
from typing import cast, override

import pytest

from eml_attachment_remover import report_delivery, reporting_v3
from eml_attachment_remover.cancellation import DeliveryGuard
from eml_attachment_remover.report_delivery import StagedChannels


class _Recorder:
    """A file that records every call made on it."""

    def __init__(self, calls: list[tuple[str, object]], name: str) -> None:
        self.calls = calls
        self.name = name

    def flush(self) -> None:
        self.calls.append((self.name, "flush"))

    def seek(self, offset: int) -> None:
        self.calls.append((self.name, f"seek{offset}"))

    def read(self, size: int) -> str:
        self.calls.append((self.name, f"read{size}"))
        return ""


def test_seal_flushes_rewinds_primes_one_character_and_rewinds_again() -> None:
    """Both files are prepared identically, and nothing is left half-read."""
    calls: list[tuple[str, object]] = []
    channels = StagedChannels(
        cast("TextIOWrapper", _Recorder(calls, "out")),
        cast("TextIOWrapper", _Recorder(calls, "err")),
    )
    channels.seal()
    steps = ["flush", "seek0", "read1", "seek0"]
    assert calls == [("out", step) for step in steps] + [
        ("err", step) for step in steps
    ]


def test_the_watched_output_runs_on_a_daemon_thread() -> None:
    """A stalled channel write can never keep the interpreter from exiting."""
    seen: list[bool] = []

    def work() -> None:
        seen.append(threading.current_thread().daemon)

    report_delivery._bounded(work, DeliveryGuard())  # ruff: ignore[private-member-access] - watched thread contract.
    assert seen == [True]


def test_a_failure_on_the_watched_thread_is_raised_on_the_caller() -> None:
    """The first failure wins and keeps its identity."""
    failure = BrokenPipeError()

    def work() -> None:
        raise failure

    with pytest.raises(BrokenPipeError) as raised:
        report_delivery._bounded(work, DeliveryGuard())  # ruff: ignore[private-member-access] - watched thread contract.
    assert raised.value is failure


def test_safe_lines_are_escaped_for_the_channel_codec_and_never_fail() -> None:
    """A character the terminal cannot show becomes an escape, not an exception."""
    raw = BytesIO()
    stream = TextIOWrapper(raw, encoding="ascii", newline="")
    reporting_v3._safe(stream, "caf\u00e9 and \u001b[0m")  # ruff: ignore[private-member-access] - display contract.
    stream.flush()
    assert raw.getvalue() == b"caf\\xe9 and \\u001b[0m\n"


def test_a_late_note_is_watched_like_the_report(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The delivering thread polls the guard while the note is being written."""
    checks: list[int] = []

    class Watched(DeliveryGuard):
        """A guard that counts how often it is consulted."""

        @override
        def check(self) -> None:
            checks.append(1)

    class SlowStderr:
        """A standard error that is slow to accept its output."""

        @staticmethod
        def write(_text: str) -> int:
            time.sleep(0.25)
            return 0

        @staticmethod
        def flush() -> None:
            return None

    monkeypatch.setattr(report_delivery, "POLL_SECONDS", 0.02)
    monkeypatch.setattr(sys, "stderr", SlowStderr())
    report_delivery.write_note("note\n", Watched())
    assert checks
