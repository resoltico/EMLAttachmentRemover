"""The platform launcher observes owner loss without consuming framed input."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
from contextlib import suppress
from pathlib import Path

import pytest

from tests.test_processing_launcher import RUNNER

PROCESSOR = """
import os, time
from pathlib import Path
def exact(count):
    data = b""
    while len(data) < count:
        chunk = os.read(0, count-len(data))
        if not chunk: raise SystemExit(2)
        data += chunk
    return data
exact(int.from_bytes(exact(4), "big"))
marker = Path(os.environ["ROOT_READY"])
pending = marker.with_suffix(".pending")
pending.write_text(str(os.getpid()))
pending.replace(marker)
while True: time.sleep(0.01)
"""


@pytest.mark.skipif(os.name == "nt", reason="POSIX process suspension")
def test_owner_eof_stops_a_suspended_processor_without_stealing_request_bytes(
    tmp_path: Path,
) -> None:
    """The independent supervisor covers a processor whose Python monitor cannot run."""
    processor = tmp_path / "processor.pyz"
    processor.write_text(PROCESSOR)
    marker = tmp_path / "root-ready"
    environment = {
        **os.environ,
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_ZIPAPP": str(processor),
        "EML_REMOVER_UI_OWNER_PIPE": "1",
        "EML_REMOVER_UI_REQUEST_PIPE": "1",
        "ROOT_READY": str(marker),
    }
    child: int | None = None
    with subprocess.Popen(
        ["/bin/sh", str(RUNNER), "--request-stdin"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    ) as process:
        assert process.stdin is not None
        try:
            payload = json.dumps({"request": "opaque test packet"}).encode()
            process.stdin.write(len(payload).to_bytes(4, "big") + payload)
            process.stdin.flush()
            deadline = time.monotonic() + 10
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            assert marker.exists(), (
                "Supervisor consumed request bytes or processor did not start"
            )
            child = int(marker.read_text())
            stopped = getattr(signal, "SIGSTOP", None)
            assert isinstance(stopped, int)
            os.kill(child, stopped)
            _confirm_stopped(child)
            process.stdin.close()
            process.stdin = None
            output, errors = process.communicate(timeout=17)
            assert process.returncode == 143, errors
            assert b"invalid processor report" in output
            with pytest.raises(ProcessLookupError):
                os.kill(child, 0)
        finally:
            _retire(process, child)


def _retire(process: subprocess.Popen[bytes], child: int | None) -> None:
    if process.poll() is not None:
        return
    if child is not None:
        killed = getattr(signal, "SIGKILL", None)
        assert isinstance(killed, int)
        with suppress(ProcessLookupError):
            os.kill(child, killed)
    process.kill()
    process.wait(timeout=5)


def _confirm_stopped(child: int) -> None:
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        if sys.platform == "darwin":
            state = subprocess.check_output(
                ["/bin/ps", "-o", "state=", "-p", str(child)],
                timeout=1,
            ).strip()
        else:
            status = Path(f"/proc/{child}/status").read_text(encoding="utf-8")
            state_line = next(
                line for line in status.splitlines() if line.startswith("State:")
            )
            state = state_line.partition(":")[2].strip().encode()
        if state.startswith(b"T"):
            return
        time.sleep(0.01)
    pytest.fail("processor suspension was not observed before owner EOF")


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher signal traps")
def test_signal_after_child_assignment_is_forwarded_only_once(tmp_path: Path) -> None:
    """Force the child-startup race rather than relying on probabilistic timing."""
    runner = tmp_path / "processing-launcher.sh"
    source = RUNNER.read_text()
    injection = (
        'CHILD=$!\nwhile [ ! -e "$ROOT_READY" ]; do sleep 0.01; done\nkill -TERM $$\n'
    )
    assert source.count("CHILD=$!\n") == 1
    runner.write_text(source.replace("CHILD=$!\n", injection))
    ready = tmp_path / "ready"
    count = tmp_path / "signal-count"
    processor = tmp_path / "processor.pyz"
    processor.write_text("""
import os, signal, time
from pathlib import Path
count = Path(os.environ["SIGNAL_COUNT"])
received = 0
def stop(_signal, _frame):
    global received
    received += 1
    count.write_text(str(received))
    time.sleep(0.2)
    print("{}")
    raise SystemExit(0)
signal.signal(signal.SIGTERM, stop)
Path(os.environ["ROOT_READY"]).write_text("ready")
while True: time.sleep(0.01)
""")
    with subprocess.Popen(
        ["/bin/sh", str(runner), "example.eml"],
        env={
            **os.environ,
            "EML_REMOVER_PYTHON": sys.executable,
            "EML_REMOVER_ZIPAPP": str(processor),
            "ROOT_READY": str(ready),
            "SIGNAL_COUNT": str(count),
        },
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    ) as process:
        try:
            _output, errors = process.communicate(timeout=5)
            assert process.returncode == 143, errors
        finally:
            kill_group = getattr(os, "killpg", None)
            killed = getattr(signal, "SIGKILL", None)
            assert callable(kill_group)
            assert isinstance(killed, int)
            with suppress(ProcessLookupError):
                kill_group(process.pid, killed)
    assert count.read_text() == "1"
