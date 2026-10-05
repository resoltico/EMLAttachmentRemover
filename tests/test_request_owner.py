"""Real EOF routing and resource retirement for owned frontend input."""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
import threading
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover.cancellation import install_cancellation_handlers
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.request_owner import sources

if TYPE_CHECKING:
    from pathlib import Path


def _frame(path: str) -> bytes:
    native = path.encode("utf-16-le") if os.name == "nt" else os.fsencode(path)
    payload = json.dumps({
        "encoding": "windows-utf16le" if os.name == "nt" else "posix-bytes",
        "paths": [base64.b64encode(native).decode("ascii")],
    }).encode()
    return len(payload).to_bytes(4, "big") + payload


def test_owner_restores_borrowed_pipe_flags_and_retires_its_thread() -> None:
    """Finishing a run releases its monitor without closing the caller's input."""
    reader, writer = os.pipe()
    with os.fdopen(reader, "r") as stream:
        try:
            os.write(writer, _frame("example.eml"))
            before = os.get_blocking(reader)
            with install_cancellation_handlers(), sources(stream) as selection:
                assert selection == ["example.eml"]
                assert not os.get_blocking(reader)
            assert os.get_blocking(reader) is before
            assert os.fstat(reader)
            assert not any(
                thread.name == "eml-request-owner" for thread in threading.enumerate()
            )
        finally:
            os.close(writer)


PROBE = """
import json, os, sys, time
from eml_attachment_remover.cancellation import (
    CancellationSignal, checkpoint, delivery_guard, install_cancellation_handlers,
)
from eml_attachment_remover.cancellation_state import CURRENT
from eml_attachment_remover import request_owner
mode = sys.argv[1]
if mode == "stalled": request_owner.GRACE_SECONDS = 0.2
try:
    with install_cancellation_handlers(), request_owner.sources(sys.stdin):
        if mode == "delivery":
            with delivery_guard() as guard:
                print("ready", flush=True)
                while not guard.signals:
                    time.sleep(0.005)
                    guard.check()
            state = CURRENT.get()
            status = guard.result(0)
            counts = {"guard":len(guard.signals),"owner":len(state.signals)}
            print(json.dumps(counts), flush=True)
        else:
            print("ready", flush=True)
            while True:
                if mode != "stalled": checkpoint()
                time.sleep(10 if mode == "stalled" else 0.005)
    raise SystemExit(status)
except CancellationSignal as interrupted:
    print(interrupted.name, flush=True)
    raise SystemExit(130)
"""


@pytest.mark.parametrize("mode", ["processing", "delivery", "stalled"])
def test_actual_owner_eof_reaches_the_current_handler_or_bounded_exit(
    mode: str,
) -> None:
    """Actual EOF routes one request; a Python-injected stall stays bounded."""
    with subprocess.Popen(
        [sys.executable, "-B", "-c", PROBE, mode],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as process:
        assert process.stdin is not None
        assert process.stdout is not None
        try:
            process.stdin.write(_frame("example.eml"))
            process.stdin.flush()
            assert process.stdout.readline() == b"ready\n"
            process.stdin.close()
            process.stdin = None
            output, errors = process.communicate(timeout=5)
            assert process.returncode == 130, errors
            if mode == "delivery":
                assert json.loads(output) == {"guard": 1, "owner": 1}
            elif mode == "processing":
                assert output == b"SIGINT\n"
            else:
                assert output == b""
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def test_regular_files_are_refused_as_owned_lifetime_inputs(tmp_path: Path) -> None:
    """A finite file is not a live frontend lifetime channel."""
    path = tmp_path / "request.bin"
    path.write_bytes(_frame("example.eml"))
    with path.open() as stream, pytest.raises(AppError) as failure:
        with sources(stream):
            pytest.fail("regular file accepted as an owner pipe")
    assert failure.value.code is ExitCode.USAGE


def test_trailing_data_cancels_without_waiting_for_owner_eof() -> None:
    """Extra bytes cannot conceal owner loss behind a buffered backlog."""
    with subprocess.Popen(
        [sys.executable, "-B", "-c", PROBE, "processing"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as process:
        assert process.stdin is not None
        assert process.stdout is not None
        try:
            process.stdin.write(_frame("example.eml"))
            process.stdin.flush()
            assert process.stdout.readline() == b"ready\n"
            process.stdin.write(b"unexpected trailing input")
            process.stdin.flush()
            assert process.wait(timeout=5) == 130
            output, errors = process.communicate(timeout=2)
            assert output == b"SIGINT\n", errors
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
