"""Hard stops preserve truthful uncertainty at both publication boundaries."""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path

PROBE = """
import sys, time
from pathlib import Path
from eml_attachment_remover import cancellation, cli, staged_output
edge, marker, source = sys.argv[1:]
cancellation.GRACE_SECONDS = 0.2
def stall(*arguments):
    Path(marker).write_text('stalled')
    time.sleep(60)
if edge == 'before':
    staged_output.os.fsync = stall
else:
    staged_output.sync_bound_directory = stall
raise SystemExit(cli.main(['--output-format=json', source]))
"""


@pytest.mark.skipif(os.name == "nt", reason="POSIX process-signal boundary")
@pytest.mark.parametrize("edge", ["before", "after"])
@pytest.mark.parametrize("stop_kind", ["kill", "interrupt"])
def test_forced_stop_discloses_no_receipt_and_never_deletes_visible_output(
    tmp_path: Path,
    edge: str,
    stop_kind: str,
) -> None:
    source = tmp_path / "sample.eml"
    raw = b"Subject: Public synthetic sample\r\n\r\nRetained text\r\n"
    source.write_bytes(raw)
    marker = tmp_path / "stalled"
    destination = tmp_path / "sample.mime-pruned.eml"
    child = subprocess.Popen(
        [sys.executable, "-B", "-c", PROBE, edge, str(marker), str(source)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        deadline = time.monotonic() + 15
        while not marker.exists() and child.poll() is None:
            if time.monotonic() >= deadline:
                pytest.fail("The simulated publication stall was not reached")
            time.sleep(0.01)
        assert marker.exists(), child.communicate(timeout=10)
        assert destination.exists() == (edge == "after")
        if stop_kind == "kill":
            child.kill()
        else:
            child.send_signal(signal.SIGINT)
        stdout, stderr = child.communicate(timeout=10)
        assert child.returncode == (-9 if stop_kind == "kill" else 130)
        assert stdout == b"", stderr
        assert source.read_bytes() == raw
        stages = list(tmp_path.glob(".eml-remove-*.tmp"))
        if edge == "before":
            assert len(stages) == 1
            assert stages[0].read_bytes() == raw
            assert stages[0].stat().st_mode & 0o777 == 0o600
            assert not destination.exists()
        else:
            assert destination.read_bytes() == raw
            assert all(stage.read_bytes() == raw for stage in stages)
    finally:
        if child.poll() is None:
            child.kill()
        child.communicate(timeout=10)
