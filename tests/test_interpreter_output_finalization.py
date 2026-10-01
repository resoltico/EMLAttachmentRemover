"""Observe application selection separately from real CPython stream-shutdown exit."""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Final, cast

import pytest

from tests.live_report_support import MESSAGE, inputs

CHILD: Final = """
import inspect, json, runpy, signal, sys, threading
from pathlib import Path
from eml_attachment_remover import cli
from tests.trace_implementation_support import traced_implementation
from eml_attachment_remover.cancellation_state import CURRENT
from eml_attachment_remover.report_session import ReportSession
channel, marker, *sources = sys.argv[1:]
function = cli._json_error
statement = (
    'with delivery_guard() as guard:' if channel == 'stdout'
    else 'status = guard.result(int(error.code))'
)
if channel == 'created_stderr':
    function = ReportSession._deliver
    statement = 'status = guard.result(status)'
function = traced_implementation(function)
lines, first = inspect.getsourcelines(function)
target = next(first + index for index, line in enumerate(lines) if statement in line)
sent = []
def trace(frame, event, arg):
    if (
        event == 'line' and frame.f_code is function.__code__
        and frame.f_lineno == target and not sent
    ):
        assert callable(signal.getsignal(signal.SIGINT))
        sent.append(1)
        signal.raise_signal(signal.SIGINT)
    return trace
sys.settrace(trace)
sys.argv = ['remove-eml-attachments', '--output-format=json', *sources]
if channel != 'created_stderr':
    sys.argv.insert(2, '--unsupported')
try:
    runpy.run_module('eml_attachment_remover', run_name='__main__')
except SystemExit as selected:
    Path(marker).write_text(json.dumps({
        'selected': selected.code, 'sent': sent, 'context_clear': CURRENT.get() is None,
        'handler_restored': (
            signal.getsignal(signal.SIGINT) == signal.default_int_handler
        ),
        'monitors': [t.name for t in threading.enumerate()
                     if t.name == 'eml-cancellation-monitor'],
    }))
    raise
"""


@pytest.mark.skipif(
    sys.platform != "linux", reason="real Linux ENOSPC device endpoints"
)
@pytest.mark.parametrize("channel", ["healthy", "stdout", "stderr", "created_stderr"])
@pytest.mark.parametrize("unbuffered", [False, True])
def test_selected_interruption_and_observed_interpreter_exit_are_distinct(
    tmp_path: Path, channel: str, *, unbuffered: bool
) -> None:
    sources = inputs(tmp_path)
    marker = tmp_path / "selected.json"
    command = [sys.executable, "-B"]
    if unbuffered:
        command.append("-u")
    command.extend(["-c", CHILD, channel, str(marker), *sources])
    environment = dict(os.environ)
    environment.pop("PYTHONUNBUFFERED", None)
    with Path("/dev/full").open("wb") as failed:
        result = subprocess.run(
            command,
            stdout=failed if channel == "stdout" else subprocess.PIPE,
            stderr=failed if channel.endswith("stderr") else subprocess.PIPE,
            env=environment,
            timeout=20,
            check=False,
        )
    receipt = json.loads(marker.read_text())
    assert receipt == {
        "selected": 130,
        "sent": [1],
        "context_clear": True,
        "handler_restored": True,
        "monitors": [],
    }
    assert result.returncode == (
        120 if channel != "healthy" and not unbuffered else 130
    )
    if channel == "stdout":
        assert cast("bytes | None", result.stdout) is None
    else:
        document = json.loads(result.stdout)
        assert document["exit_code"] == (0 if channel == "created_stderr" else 2)
    assert all(
        (tmp_path / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
        for source in sources
    )
    copies = sorted(tmp_path.glob("*.mime-pruned.eml"))
    assert len(copies) == (2 if channel == "created_stderr" else 0)
    for item, copy in zip(document["items"] if copies else [], copies, strict=True):
        assert item["terminalized"]
        assert item["status"] == "created"
        assert item["publication"]["address_verified"]
        assert (
            item["publication"]["sha256"]
            == hashlib.sha256(copy.read_bytes()).hexdigest()
        )
        assert b"public body" in copy.read_bytes()
        assert b"PUBLIC-ATTACHMENT" not in copy.read_bytes()
