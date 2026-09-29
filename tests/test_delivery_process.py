"""A real process with a reader that stopped draining still honors cancellation."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
from typing import TYPE_CHECKING, Final

import pytest

if TYPE_CHECKING:
    from pathlib import Path

CHILD: Final = """
import sys
from eml_attachment_remover import cancellation, cli, report_delivery

marker = sys.argv[1]
cancellation.GRACE_SECONDS = 0.5
original = report_delivery.StagedChannels.deliver


def marked(self, output_format, guard):
    open(marker, "w").close()
    original(self, output_format, guard)


report_delivery.StagedChannels.deliver = marked
raise SystemExit(cli.main(sys.argv[2:]))
"""
DEADLINE_SECONDS: Final = 60.0


def _start(tmp_path: Path, count: int) -> tuple[subprocess.Popen[bytes], Path]:
    """Start a run whose JSON report far exceeds a pipe's capacity.

    Returns:
        The child process (stdout piped and never read) and its delivery marker.

    """
    marker = tmp_path / "delivering"
    missing = [str(tmp_path / f"missing-{index}.eml") for index in range(count)]
    child = subprocess.Popen(
        [
            *(sys.executable, "-c", CHILD, str(marker)),
            *("--output-format", "json", "--", *missing),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "PYTHONUNBUFFERED": "1"},
    )
    deadline = time.monotonic() + DEADLINE_SECONDS
    while not marker.exists():
        assert child.poll() is None, "the run ended before delivery began"
        assert time.monotonic() < deadline, "delivery never began"
        time.sleep(0.05)
    time.sleep(0.3)
    return child, marker


@pytest.mark.skipif(os.name == "nt", reason="POSIX signals and pipe semantics")
@pytest.mark.parametrize("delivered", [signal.SIGINT, signal.SIGTERM])
def test_a_stopped_reader_cannot_keep_a_signalled_process_alive(
    tmp_path: Path, delivered: signal.Signals
) -> None:
    """After the grace the process ends with 130 and a partial, unrepeated document."""
    child, _marker = _start(tmp_path, 400)
    assert child.stdout is not None
    started = time.monotonic()
    child.send_signal(delivered)
    assert child.wait(timeout=30) == 130
    assert time.monotonic() - started < 20
    partial = child.stdout.read()
    child.stdout.close()
    assert child.stderr is not None
    child.stderr.close()
    assert 0 < len(partial) < 400 * 400
    with pytest.raises(json.JSONDecodeError):
        json.loads(partial)


@pytest.mark.skipif(os.name == "nt", reason="POSIX signals and pipe semantics")
def test_a_repeated_signal_ends_a_stalled_delivery_immediately(
    tmp_path: Path,
) -> None:
    """The second interrupt does not wait for the grace."""
    child, _marker = _start(tmp_path, 400)
    child.send_signal(signal.SIGINT)
    time.sleep(0.1)
    child.send_signal(signal.SIGINT)
    assert child.wait(timeout=5) == 130
    for stream in (child.stdout, child.stderr):
        assert stream is not None
        stream.close()
