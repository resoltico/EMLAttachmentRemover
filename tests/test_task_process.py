"""Contracts for owned task process groups and their bounded shutdown."""

from __future__ import annotations

import importlib
import itertools
import os
import signal
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, Final
from unittest.mock import MagicMock, call, patch

from tools import task_process

if TYPE_CHECKING:
    from collections.abc import Sequence

POSIX: Final = os.name != "nt"
# Built once: patching os.name changes pathlib's flavour for paths made inside.
PROJECT: Final = Path("/project")
# A descendant that ignores SIGINT and writes only after its owner has returned.
LATE_WRITER: Final = textwrap.dedent(
    """
    import os, signal, sys, time
    from pathlib import Path

    if os.fork() == 0:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        time.sleep(1.0)
        Path(sys.argv[1]).write_text("late", encoding="utf-8")
        os._exit(0)
    time.sleep(float(sys.argv[2]))
    """
)


def _python(source: str, *arguments: str) -> tuple[str, ...]:
    return (sys.executable, "-c", source, *arguments)


def _environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    return environment


def _fake_run(
    exits: Sequence[bool | BaseException],
    *,
    returncode: int = 0,
    timeout: float | None = 10.0,
) -> tuple[MagicMock, MagicMock, MagicMock, MagicMock]:
    """Run through the POSIX path with a fake leader, clock, and signals.

    Returns:
        The Popen factory, process, group-signal, and sleep doubles.

    """
    process = MagicMock(pid=4242)
    process.wait.return_value = returncode
    clock = itertools.count(start=100.0, step=1.0)
    with (
        patch.object(os, "name", "posix"),
        patch.object(subprocess, "Popen", return_value=process) as popen,
        patch.object(task_process, "_leader_exited", side_effect=exits),
        patch.object(task_process, "_signal_group") as signal_group,
        patch.object(time, "sleep") as sleep,
        patch.object(time, "monotonic", side_effect=clock.__next__),
    ):
        task_process.run(
            ("command", "argument"),
            cwd=PROJECT,
            env={"PUBLIC": "1"},
            timeout=timeout,
            grace_seconds=2.5,
        )
    return popen, process, signal_group, sleep


class PosixProcessGroupTests(unittest.TestCase):
    """Exercise real descendants; Windows lanes cover these paths with fakes."""

    def test_timeout_stops_a_descendant_that_ignores_interruption(self) -> None:
        if not POSIX:
            return
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "late"
            started = time.monotonic()
            with self.assertRaises(subprocess.TimeoutExpired) as raised:
                task_process.run(
                    _python(LATE_WRITER, str(marker), "30"),
                    cwd=Path(directory),
                    env=_environment(),
                    timeout=0.3,
                    grace_seconds=0.2,
                )
            self.assertLess(time.monotonic() - started, 0.9)
            self.assertEqual(raised.exception.timeout, 0.3)
            time.sleep(1.5)
            self.assertFalse(marker.exists())

    def test_normal_exit_sweeps_remaining_group_members(self) -> None:
        if not POSIX:
            return
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "late"
            task_process.run(
                _python(LATE_WRITER, str(marker), "0"),
                cwd=Path(directory),
                env=_environment(),
                timeout=30,
            )
            time.sleep(1.5)
            self.assertFalse(marker.exists())

    def test_interruption_reaches_the_group_before_escalation(self) -> None:
        if not POSIX:
            return
        source = textwrap.dedent(
            """
            import signal, sys, time
            from pathlib import Path

            def stop(_number, _frame):
                Path(sys.argv[1]).write_text("interrupted", encoding="utf-8")
                raise SystemExit(0)

            signal.signal(signal.SIGINT, stop)
            Path(sys.argv[2]).write_text("ready", encoding="utf-8")
            time.sleep(30)
            """
        )
        with tempfile.TemporaryDirectory() as directory:
            interrupted = Path(directory) / "interrupted"
            ready = Path(directory) / "ready"
            started = time.monotonic()
            with self.assertRaises(subprocess.TimeoutExpired):
                task_process.run(
                    _python(source, str(interrupted), str(ready)),
                    cwd=Path(directory),
                    env=_environment(),
                    timeout=2.0,
                    grace_seconds=20,
                )
            self.assertLess(time.monotonic() - started, 10)
            self.assertTrue(ready.exists())
            self.assertEqual(interrupted.read_text(encoding="utf-8"), "interrupted")

    def test_unsuccessful_exit_reports_the_exact_command_and_status(self) -> None:
        if not POSIX:
            return
        command = _python("raise SystemExit(3)")
        with self.assertRaises(subprocess.CalledProcessError) as raised:
            task_process.run(command, cwd=Path.cwd(), env=_environment(), timeout=30)
        self.assertEqual(raised.exception.returncode, 3)
        self.assertEqual(raised.exception.cmd, command)


class FakeProcessGroupTests(unittest.TestCase):
    """Exercise every control path deterministically on every platform."""

    def test_normal_exit_waits_without_reaping_then_sweeps(self) -> None:
        popen, process, signal_group, sleep = _fake_run([False, False, True])
        popen.assert_called_once_with(
            ("command", "argument"),
            cwd=PROJECT,
            env={"PUBLIC": "1"},
            start_new_session=True,
        )
        self.assertEqual(sleep.call_args_list, [call(task_process.POLL_SECONDS)] * 2)
        signal_group.assert_called_once_with(4242, task_process._KILL_SIGNAL)
        process.wait.assert_called_once_with()

    def test_missing_timeout_never_expires(self) -> None:
        _popen, _process, signal_group, sleep = _fake_run(
            [False] * 5 + [True], timeout=None
        )
        self.assertEqual(sleep.call_count, 5)
        signal_group.assert_called_once_with(4242, task_process._KILL_SIGNAL)

    def test_unsuccessful_exit_raises_after_the_sweep(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as raised:
            _fake_run([True], returncode=7)
        self.assertEqual(raised.exception.returncode, 7)
        self.assertEqual(raised.exception.cmd, ("command", "argument"))

    def test_timeout_interrupts_waits_for_grace_then_kills(self) -> None:
        signals: list[int] = []
        process = MagicMock(pid=4242)
        clock = itertools.count(start=100.0, step=1.0)
        # Exactly the polls a correct run makes; a loop that ignores its deadline
        # exhausts them and fails fast instead of spinning forever.
        polls = [False] * 5 + [AssertionError("polled past a deadline")]
        with (
            patch.object(os, "name", "posix"),
            patch.object(subprocess, "Popen", return_value=process),
            patch.object(task_process, "_leader_exited", side_effect=polls),
            patch.object(
                task_process,
                "_signal_group",
                side_effect=lambda _group, number: signals.append(number),
            ),
            patch.object(time, "sleep") as sleep,
            patch.object(time, "monotonic", side_effect=clock.__next__),
            self.assertRaises(subprocess.TimeoutExpired) as raised,
        ):
            task_process.run(
                ("command",),
                cwd=PROJECT,
                env={},
                timeout=3.0,
                grace_seconds=2.0,
            )
        self.assertEqual(raised.exception.timeout, 3.0)
        self.assertEqual(raised.exception.cmd, ("command",))
        self.assertEqual(signals, [signal.SIGINT, task_process._KILL_SIGNAL])
        # Clock 100 sets the 103 deadline; 101 and 102 sleep; 103 expires. 104 sets
        # the 106 grace deadline; 105 sleeps; 106 ends the grace period.
        self.assertEqual(sleep.call_count, 3)
        process.wait.assert_called_once_with()

    def test_repeated_interruption_during_grace_still_kills_the_group(self) -> None:
        with self.assertRaises(KeyboardInterrupt):
            _fake_run([KeyboardInterrupt(), KeyboardInterrupt()])

    def test_windows_keeps_the_standard_bounded_subprocess_call(self) -> None:
        # Patching os.name changes pathlib's flavour, so build paths beforehand.
        project = PROJECT
        with (
            patch.object(os, "name", "nt"),
            patch.object(subprocess, "run") as run,
            patch.object(subprocess, "Popen") as popen,
            # A faked Popen's pid converts to 1; never let it reach a real signal.
            patch.object(task_process, "_signal_group") as signal_group,
            patch.object(task_process, "_leader_exited") as leader_exited,
        ):
            task_process.run(
                ("command",),
                cwd=project,
                env={"PUBLIC": "1"},
                timeout=9,
            )
        self.assertEqual(
            run.call_args_list,
            [
                call(
                    ("command",),
                    check=True,
                    cwd=project,
                    env={"PUBLIC": "1"},
                    timeout=9,
                )
            ],
        )
        self.assertEqual(popen.call_count, 0)
        self.assertEqual(signal_group.call_count + leader_exited.call_count, 0)

    def test_default_grace_leaves_room_before_github_escalates(self) -> None:
        # GitHub Actions follows a cancellation SIGINT with SIGTERM after 7.5 s.
        self.assertEqual(task_process.DEFAULT_GRACE_SECONDS, 5.0)
        self.assertEqual(task_process.POLL_SECONDS, 0.05)


class PlatformPrimitiveTests(unittest.TestCase):
    """Load both platform branches on every lane, as native_posix tests do."""

    def test_windows_primitives_refuse_posix_group_control(self) -> None:
        self.addCleanup(importlib.reload, task_process)
        with patch.object(sys, "platform", "win32"):
            importlib.reload(task_process)
        self.assertEqual(task_process._KILL_SIGNAL, signal.SIGTERM)
        for primitive in (
            lambda: task_process._signal_group(1, 2),
            lambda: task_process._leader_exited(1),
        ):
            with self.assertRaisesRegex(OSError, "unavailable on Windows"):
                primitive()

    def test_posix_primitives_signal_and_wait_without_reaping(self) -> None:
        fake_os = SimpleNamespace(
            P_PID=1,
            WEXITED=2,
            WNOHANG=4,
            WNOWAIT=8,
            killpg=MagicMock(),
            waitid=MagicMock(side_effect=[None, object()]),
        )
        self.addCleanup(importlib.reload, task_process)
        with (
            patch.object(sys, "platform", "linux"),
            patch.object(signal, "SIGKILL", 9, create=True),
        ):
            importlib.reload(task_process)
        self.assertEqual(task_process._KILL_SIGNAL, 9)
        with patch.object(task_process, "os", fake_os):
            self.assertFalse(task_process._leader_exited(77))
            self.assertTrue(task_process._leader_exited(77))
            task_process._signal_group(2, 15)
            for gone in (ProcessLookupError(), PermissionError()):
                fake_os.killpg.side_effect = gone
                task_process._signal_group(77, 15)
            for unowned in (1, 0, -1):
                with self.assertRaisesRegex(
                    ValueError, f"refusing to signal unowned process group {unowned}$"
                ):
                    task_process._signal_group(unowned, 15)
        self.assertEqual(fake_os.waitid.call_args_list, [call(1, 77, 14)] * 2)
        self.assertEqual(
            fake_os.killpg.call_args_list, [call(2, 15), call(77, 15), call(77, 15)]
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
