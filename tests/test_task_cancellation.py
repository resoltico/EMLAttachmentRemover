"""Runner-PID signals must retire the task's separately owned descendants."""

from __future__ import annotations

import contextlib
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest
from tools import task_process


def _start_task(tmp_path: Path, cancellation: str) -> subprocess.Popen[bytes]:
    """Start the actual shell/uv/task ownership boundary with private fixtures.

    Returns:
        The exec launcher with captured inherited output pipes.

    """
    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv launcher is unavailable")
    child = tmp_path / "child.py"
    child.write_text(
        "import json, os, signal, sys, time\n"
        "from pathlib import Path\n"
        "root = Path(sys.argv[1])\n"
        "def interrupt(number, frame):\n"
        "    (root / 'interrupted').touch()\n"
        "    if sys.argv[2] != 'escalate':\n"
        "        raise SystemExit(0)\n"
        "signal.signal(signal.SIGINT, interrupt)\n"
        "if os.fork() == 0:\n"
        "    signal.signal(signal.SIGINT, signal.SIG_IGN)\n"
        "    time.sleep(1)\n"
        "    (root / 'late-write').touch()\n"
        "    os._exit(0)\n"
        "pending = root / 'child-pid.pending'\n"
        "pending.write_text(json.dumps({'pid': os.getpid()}))\n"
        "pending.replace(root / 'child.json')\n"
        "print('ready', flush=True)\n"
        "time.sleep(30)\n",
        encoding="utf-8",
    )
    parent = tmp_path / "parent.py"
    parent.write_text(
        "import os, sys\n"
        "from pathlib import Path\n"
        "import cancellation_import_probe\n"
        "from tools import tasks\n"
        "Path(sys.argv[1], 'parent.pid').write_text(str(os.getpid()))\n"
        "tasks._run((sys.executable, '-B', sys.argv[2], sys.argv[1], sys.argv[3]))\n",
        encoding="utf-8",
    )
    command = [
        uv,
        "run",
        "--no-project",
        "--python",
        sys.executable,
        "python",
        "-B",
        str(parent),
        str(tmp_path),
        str(child),
        cancellation,
    ]
    wrapper = tmp_path / "step.sh"
    wrapper.write_text("exec " + shlex.join(command) + "\n", encoding="utf-8")
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("COVERAGE_")
    }
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    environment["PYTHONPATH"] = os.pathsep.join(
        filter(
            None,
            (
                str(Path(task_process.__file__).resolve().parents[1]),
                environment.get("PYTHONPATH"),
            ),
        )
    )
    return subprocess.Popen(
        ["/bin/bash", "--noprofile", "--norc", "-e", "-o", "pipefail", str(wrapper)],
        env=environment,
        start_new_session=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def _wait_for(path: Path, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while not path.exists():
        assert time.monotonic() < deadline, f"fixture did not produce {path.name}"
        time.sleep(0.01)


@pytest.fixture
def forwarded_child_import(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Prove incoming child import roots survive beside mutation bootstrap.

    Returns:
        The marker containing the Python task owner's PID after startup.

    """
    import_root = tmp_path / "child-imports"
    import_root.mkdir()
    marker = tmp_path / "import.pid"
    (import_root / "cancellation_import_probe.py").write_text(
        "import os\n"
        "from pathlib import Path\n"
        f"Path({str(marker)!r}).write_text(str(os.getpid()))\n",
        encoding="utf-8",
    )
    monkeypatch.setenv(
        "PYTHONPATH",
        os.pathsep.join(
            filter(
                None,
                (
                    str(import_root),
                    os.environ.get("PYTHONPATH"),
                ),
            )
        ),
    )
    return marker


def _cleanup_failed_fixture(
    process: subprocess.Popen[bytes],
    child_pid: int | None,
    parent_pid: int | None,
) -> None:
    if sys.platform != "win32":
        for group in filter(None, (child_pid, process.pid)):
            with contextlib.suppress(ProcessLookupError):
                os.killpg(group, signal.SIGKILL)
        if parent_pid is not None:
            with contextlib.suppress(ProcessLookupError):
                os.kill(parent_pid, signal.SIGKILL)
        process.communicate(timeout=5)


@pytest.mark.skipif(os.name == "nt", reason="requires POSIX process groups")
@pytest.mark.parametrize("cancellation", ["interrupt", "terminate", "escalate"])
def test_exec_shell_uv_task_cancellation_closes_pipes_and_stops_descendants(
    tmp_path: Path,
    cancellation: str,
    forwarded_child_import: Path,
) -> None:
    process = _start_task(tmp_path, cancellation)
    child_pid = None
    parent_pid = None
    retired = False
    try:
        _wait_for(tmp_path / "child.json", 10)
        child_pid = json.loads((tmp_path / "child.json").read_text())["pid"]
        parent_pid = int((tmp_path / "parent.pid").read_text())
        assert int(forwarded_child_import.read_text(encoding="utf-8")) == parent_pid
        first = signal.SIGTERM if cancellation == "terminate" else signal.SIGINT
        process.send_signal(first)
        if cancellation == "escalate":
            _wait_for(tmp_path / "interrupted", 2)
            process.send_signal(signal.SIGTERM)
        # communicate also requires EOF from every descendant holding the pipe.
        output, _ = process.communicate(timeout=2)
        retired = True
        expected = 143 if cancellation != "interrupt" else 130
        assert process.returncode == expected, output.decode(errors="replace")
        time.sleep(1.1)
        assert not (tmp_path / "late-write").exists()
    finally:
        if not retired:
            _cleanup_failed_fixture(process, child_pid, parent_pid)
