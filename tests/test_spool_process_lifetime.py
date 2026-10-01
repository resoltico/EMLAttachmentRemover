"""OS-owned receipt storage disappears on completion and both forced-exit routes."""

from __future__ import annotations

import os
import subprocess
import sys
import time
from typing import TYPE_CHECKING

import pytest

from tests.live_report_support import MESSAGE

if TYPE_CHECKING:
    from pathlib import Path

SCRIPT = """
import signal, sys, threading
from pathlib import Path
from eml_attachment_remover import cancellation, cli, report_delivery
case, marker, *sources = sys.argv[1:]
cancellation.GRACE_SECONDS = 0.4
original = report_delivery.StagedChannels.deliver
def deliver(self, fmt, guard):
    Path(marker).write_text('ready')
    if case in ('grace', 'repeat'):
        guard.record(signal.SIGINT, None)
    if case == 'repeat':
        threading.Timer(0.05, guard.record, (signal.SIGINT, None)).start()
    original(self, fmt, guard)
report_delivery.StagedChannels.deliver = deliver
raise SystemExit(cli.main(['--output-format=json', '--existing=verify', *sources]))
"""


# Exercise fresh pipe lifetimes repeatedly: the Windows CRT's closed-reader
# failure depends on whether the close precedes or interrupts the first write.
@pytest.mark.parametrize(
    "case",
    [
        "complete",
        "grace",
        "repeat",
        *(pytest.param("broken", id=f"broken-{index}") for index in range(20)),
    ],
)
def test_private_directory_has_no_orphaned_receipts_after_process_exit(
    tmp_path: Path,
    case: str,
) -> None:
    storage = tmp_path / "private-storage"
    storage.mkdir()
    sources = []
    for index in range(110):
        source = tmp_path / f"source-{index}.eml"
        source.write_bytes(MESSAGE)
        sources.append(str(source))
    marker = tmp_path / "ready"
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.startswith("COVERAGE_")
    }
    environment.update(
        TMPDIR=str(storage), TEMP=str(storage), TMP=str(storage), PYTHONUNBUFFERED="1"
    )
    command = [sys.executable, "-B", "-c", SCRIPT, case, str(marker), *sources]
    if case == "complete":
        result = subprocess.run(
            command, env=environment, capture_output=True, check=False, timeout=30
        )
        assert result.returncode == 0, result.stderr
    else:
        _cancelled_process(command, environment, marker, case)
    assert not list(storage.iterdir())
    assert len(list(tmp_path.glob("*.mime-pruned.eml"))) == 110
    for argument in sources:
        assert (tmp_path / argument.rsplit("/", 1)[-1]).read_bytes() == MESSAGE


def _cancelled_process(
    command: list[str], environment: dict[str, str], marker: Path, case: str
) -> None:
    """Keep the reader open and ensure the fixture exits without a watchdog kill."""
    with subprocess.Popen(
        command, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    ) as child:
        try:
            _await_marker(child, marker)
            assert child.stdout is not None
            if case == "broken":
                child.stdout.close()
                child.stdout = None
            status = child.wait(timeout=5)
            _output, diagnostics = child.communicate(timeout=5)
            assert status == (1 if case == "broken" else 130), diagnostics
        finally:
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=5)


def _await_marker(child: subprocess.Popen[bytes], marker: Path) -> None:
    """Wait for complete processing and the actual delivery boundary."""
    deadline = time.monotonic() + 15
    while not marker.exists() and time.monotonic() < deadline:
        assert child.poll() is None
        time.sleep(0.01)
    assert marker.exists()
