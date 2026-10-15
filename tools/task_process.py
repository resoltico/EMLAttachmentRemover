"""Run task commands in owned POSIX process groups with bounded shutdown."""

from __future__ import annotations

import contextlib
import math
import os
import signal
import subprocess
import sys
import threading
import time
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable, Generator, Mapping, Sequence
    from pathlib import Path
    from types import FrameType

DEFAULT_GRACE_SECONDS: Final = 5.0
POLL_SECONDS: Final = 0.05


class _TerminationRequested(SystemExit):
    """Unwind through owned-process cleanup without waiting for SIGINT grace."""


@contextlib.contextmanager
def _termination_cleanup() -> Generator[Callable[[], None]]:
    """Temporarily unwind default main-thread SIGTERM through process cleanup.

    Existing ignored or custom policies belong to the caller. Python cannot
    install signal handlers from other threads, which retain their caller's
    process-wide termination policy.

    Yields:
        A check that raises pending default termination at an owned boundary.

    """
    if (
        threading.current_thread() is not threading.main_thread()
        or signal.getsignal(signal.SIGTERM) != signal.SIG_DFL
    ):
        yield lambda: None
        return
    requested: int | None = None

    def request_termination(number: int, _frame: FrameType | None) -> None:
        nonlocal requested
        requested = number

    def check_termination() -> None:
        """Raise pending termination only after acquiring the child handle.

        Raises:
            _TerminationRequested: If SIGTERM requested process cleanup.

        """
        if requested is not None:
            raise _TerminationRequested(128 + requested)

    previous = signal.signal(signal.SIGTERM, request_termination)
    try:
        yield check_termination
    finally:
        signal.signal(signal.SIGTERM, previous)


if sys.platform == "win32":
    _POSIX_UNAVAILABLE: Final = "process groups are unavailable on Windows"
    _KILL_SIGNAL: Final = signal.SIGTERM

    def _signal_group(_group: int, _number: int) -> None:
        """Reject POSIX group signalling when type-checking on Windows.

        Raises:
            OSError: Always, because Windows has no POSIX process groups.

        """
        raise OSError(_POSIX_UNAVAILABLE)

    def _leader_exited(_pid: int) -> bool:
        """Reject POSIX non-reaping waits when type-checking on Windows.

        Raises:
            OSError: Always, because Windows has no POSIX ``waitid``.

        """
        raise OSError(_POSIX_UNAVAILABLE)

else:
    _KILL_SIGNAL: Final = signal.SIGKILL

    def _signal_group(group: int, number: int) -> None:
        """Signal every member of an owned group that can still receive it.

        Raises:
            ValueError: If the group is the caller's own (0) or init's (1), which
                are never owned child groups and must never be signalled.

        """
        if group <= 1:
            message = f"refusing to signal unowned process group {group}"
            raise ValueError(message)
        # ESRCH: the group is gone. EPERM: macOS reports a group whose remaining
        # members are all zombies this way; zombies run no code.
        with contextlib.suppress(ProcessLookupError, PermissionError):
            os.killpg(group, number)

    def _leader_exited(pid: int) -> bool:
        """Return whether the group leader exited, without reaping it.

        An unreaped leader keeps its PID, so its group ID cannot be reused by an
        unrelated group while this module may still signal it.

        Returns:
            Whether the leader has exited.

        """
        options = os.WEXITED | os.WNOHANG | os.WNOWAIT
        return os.waitid(os.P_PID, pid, options) is not None


def _await_leader(
    pid: int,
    command: Sequence[str],
    timeout: float | None,
    check_termination: Callable[[], None],
) -> None:
    """Wait for the leader to exit without reaping it.

    Raises:
        subprocess.TimeoutExpired: If the leader outlives the timeout.

    """
    limit = math.inf if timeout is None else timeout
    deadline = time.monotonic() + limit
    while True:
        check_termination()
        if _leader_exited(pid):
            return
        if time.monotonic() >= deadline:
            raise subprocess.TimeoutExpired(command, limit)
        time.sleep(POLL_SECONDS)


def _interrupt(
    pid: int,
    grace_seconds: float,
    check_termination: Callable[[], None],
) -> None:
    """Ask the group to stop and give its leader a bounded chance to exit."""
    _signal_group(pid, signal.SIGINT)
    deadline = time.monotonic() + grace_seconds
    while not _leader_exited(pid) and time.monotonic() < deadline:
        check_termination()
        time.sleep(POLL_SECONDS)


def run(
    command: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str],
    timeout: float | None,
    grace_seconds: float = DEFAULT_GRACE_SECONDS,
) -> None:
    """Run one command; on POSIX, own its process group and always sweep it.

    On timeout or interruption the group receives SIGINT, then SIGKILL after the
    grace period. After a normal exit, leftover group members receive SIGKILL, so
    no owned descendant can still write once this function returns or raises.
    Default main-thread SIGTERM skips the grace period and immediately sweeps
    the group. Custom, ignored and non-main-thread signal policies remain owned
    by the caller; Windows stops only the direct child on timeout.

    Raises:
        subprocess.CalledProcessError: If the command exits unsuccessfully.
        _TerminationRequested: If default main-thread termination requests cleanup.

    """
    if os.name == "nt":
        subprocess.run(command, check=True, cwd=cwd, env=env, timeout=timeout)
        return
    with _termination_cleanup() as check_termination:
        process = subprocess.Popen(command, cwd=cwd, env=env, start_new_session=True)
        try:
            _await_leader(process.pid, command, timeout, check_termination)
        except _TerminationRequested:
            raise
        except BaseException:
            _interrupt(process.pid, grace_seconds, check_termination)
            raise
        finally:
            _signal_group(process.pid, _KILL_SIGNAL)
            returncode = process.wait()
        check_termination()
    if returncode:
        raise subprocess.CalledProcessError(returncode, command)
