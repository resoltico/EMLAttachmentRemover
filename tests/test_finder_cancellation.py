"""Signals to the launcher still finalize validated completed processor results."""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

from tests.test_processing_launcher import RUNNER, _complete

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher signals")
@pytest.mark.parametrize("target", ["launcher", "processor"])
@pytest.mark.parametrize(
    "number", [signal.SIGINT, signal.SIGTERM, getattr(signal, "SIGHUP", signal.SIGTERM)]
)
def test_cancellation_displays_completed_results_before_cleanup(
    tmp_path: Path,
    target: str,
    number: int,
) -> None:
    result, revealed = _cancel(tmp_path, target, number)
    expected = 128 + number if target == "launcher" else 130
    assert result.returncode == expected
    assert json.loads(result.stdout)["report"]["summary"]["created"] == 1
    assert "Interrupted:" in result.stdout
    assert "invalid processor report" not in result.stdout
    assert not revealed
    assert not list(tmp_path.glob("eml-remover-report.*"))
    assert not list(tmp_path.glob("eml-remover-errors.*"))


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher signals")
@pytest.mark.parametrize("repeat", [False, True])
def test_an_uncooperative_child_cannot_prevent_finalization(
    tmp_path: Path, *, repeat: bool
) -> None:
    result, revealed = _cancel(
        tmp_path, "launcher", signal.SIGTERM, stubborn=True, repeat=repeat
    )
    assert result.returncode == 143
    assert json.loads(result.stdout)["report"]["summary"]["created"] == 1
    assert "launcher cancelled" in result.stdout
    assert not revealed


@pytest.mark.skipif(os.name == "nt", reason="POSIX launcher signals")
def test_launcher_cancellation_does_not_accept_incomplete_results(
    tmp_path: Path,
) -> None:
    result, revealed = _cancel(tmp_path, "incomplete", signal.SIGTERM)
    assert result.returncode == 143
    assert "invalid processor report" in result.stdout
    assert "created 1" not in result.stdout
    assert not revealed


@pytest.mark.skipif(os.name == "nt", reason="POSIX owner lifetime pipe")
@pytest.mark.parametrize("stubborn", [False, True])
def test_gui_owner_loss_cancels_the_owned_processor(
    tmp_path: Path, *, stubborn: bool
) -> None:
    """Closing the GUI lifetime pipe preserves receipts and bounds shutdown."""
    result, revealed = _cancel(tmp_path, "owner", signal.SIGTERM, stubborn=stubborn)
    assert result.returncode == 143
    assert json.loads(result.stdout)["report"]["summary"]["created"] == 1
    assert "launcher cancelled" in result.stdout
    assert not revealed
    assert not list(tmp_path.glob("eml-remover-report.*"))


def _cancel(
    tmp_path: Path,
    target: str,
    number: int,
    *,
    stubborn: bool = False,
    repeat: bool = False,
) -> tuple[subprocess.CompletedProcess[str], bool]:
    """Run a processor fixture to readiness, signal it, and collect final output.

    Returns:
        The launcher result and whether it requested a reveal.

    """
    marker = tmp_path / "ready"
    reveal = tmp_path / "revealed"
    opener = tmp_path / "open"
    opener.write_text(f"#!/bin/sh\ntouch '{reveal}'\n", encoding="utf-8")
    opener.chmod(0o700)
    processor = tmp_path / "processor.pyz"
    processor.write_text(
        "import os, signal, sys, time\nfrom pathlib import Path\n"
        "def stop(*args): raise SystemExit(130)\n"
        "for number in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):\n"
        "    handler = signal.SIG_IGN if os.environ['STUBBORN'] == '1' else stop\n"
        "    signal.signal(number, handler)\n"
        "print(os.environ['REPORT'], flush=True)\n"
        "Path(os.environ['MARKER']).write_text(str(os.getpid()))\n"
        "while True: time.sleep(0.05)\n",
        encoding="utf-8",
    )
    environment = {
        # These synthetic children import no product Python and can be SIGKILLed;
        # instrumenting them would leave incomplete coverage database files.
        **{
            key: value
            for key, value in os.environ.items()
            if not key.startswith("COVERAGE_")
        },
        "PATH": f"{tmp_path}{os.pathsep}{os.environ['PATH']}",
        "TMPDIR": str(tmp_path),
        "EML_REMOVER_PYTHON": sys.executable,
        "EML_REMOVER_ZIPAPP": str(processor),
        "EML_REMOVER_UI_OWNER_PIPE": "1" if target == "owner" else "0",
        "STUBBORN": "1" if stubborn else "0",
        "MARKER": str(marker),
        "REPORT": json.dumps(_complete(0))
        if target != "incomplete"
        else '{"schema_version":',
    }
    with subprocess.Popen(
        ["/bin/sh", str(RUNNER), "source.eml"],
        env=environment,
        stdin=subprocess.PIPE if target == "owner" else None,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    ) as launcher:
        try:
            deadline = time.monotonic() + 5
            while not marker.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            assert marker.exists(), "processor never reached readiness"
            pid = int(marker.read_text()) if target == "processor" else launcher.pid
            if target == "owner":
                time.sleep(0.2)
                assert launcher.poll() is None
                os.kill(int(marker.read_text()), 0)
                assert launcher.stdin is not None
                launcher.stdin.close()
                launcher.stdin = None
            else:
                os.kill(pid, number)
            if repeat:
                time.sleep(0.1)
                os.kill(pid, number)
            out, err = launcher.communicate(timeout=16)
        finally:
            if launcher.poll() is None:
                _kill(launcher)
                launcher.communicate(timeout=5)
        return subprocess.CompletedProcess(
            launcher.args, launcher.returncode, out, err
        ), reveal.exists()


def _kill(launcher: subprocess.Popen[str]) -> None:
    """Stop the fixture's process group after a test failure."""
    if sys.platform == "win32":
        launcher.kill()
    else:
        os.killpg(launcher.pid, signal.SIGKILL)
