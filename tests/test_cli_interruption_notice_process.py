"""A blocked interruption notice honors its seeded request and repeat escalation."""

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
import inspect, os, signal, sys, time, threading
from pathlib import Path
from eml_attachment_remover import cancellation, cli
from tests.trace_implementation_support import traced_implementation
marker = Path(sys.argv[1])
cancellation.GRACE_SECONDS = 0.3
class Blocked:
    encoding = 'ascii'
    def write(self, text):
        marker.write_text('blocked')
        if os.name == 'nt' and os.environ.get('TEST_REPEAT') == '1':
            threading.Timer(0.05, signal.raise_signal, (signal.SIGINT,)).start()
        time.sleep(60)
    def flush(self):
        pass
sys.stderr = Blocked()
function = traced_implementation(cli._json_error)
lines, first = inspect.getsourcelines(function)
target = next(
    first + index for index, line in enumerate(lines)
    if 'status = guard.result(int(error.code))' in line
)
sent = []
def trace(frame, event, arg):
    if (
        event == 'line' and frame.f_code is function.__code__
        and frame.f_lineno == target and not sent
    ):
        sent.append(1)
        signal.raise_signal(signal.SIGINT)
    return trace
sys.settrace(trace)
raise SystemExit(cli.main(['--output-format=json']))
"""


@pytest.mark.parametrize("repeat", [False, True])
def test_stalled_notice_has_a_deadline_and_repeat_escalation(
    tmp_path: Path, *, repeat: bool
) -> None:
    marker = tmp_path / "blocked"
    child = subprocess.Popen(
        [sys.executable, "-B", "-c", CHILD, str(marker)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={
            **os.environ,
            "PYTHONUNBUFFERED": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            "TEST_REPEAT": "1" if repeat else "0",
        },
    )
    try:
        deadline = time.monotonic() + 10
        while (
            not marker.exists() and child.poll() is None and time.monotonic() < deadline
        ):
            time.sleep(0.01)
        assert marker.exists()
        if repeat and os.name != "nt":
            child.send_signal(signal.SIGINT)
        output, errors = child.communicate(timeout=10)
        assert child.returncode == 130
        assert json.loads(output)["exit_code"] == 2
        assert not errors
    finally:
        if child.poll() is None:
            child.kill()
        child.communicate(timeout=10)
