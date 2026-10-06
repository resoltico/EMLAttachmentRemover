"""Ownership of a framed-input pipe and a bounded frontend-loss monitor."""

from __future__ import annotations

import os
import signal
import stat
import threading
from _thread import interrupt_main  # ruff: ignore[import-private-name] - documented public stdlib main-thread signal dispatch.
from contextlib import contextmanager
from functools import partial
from typing import TYPE_CHECKING

from .cancellation import GRACE_SECONDS, INTERRUPTED_STATUS, POLL_SECONDS, hard_exit
from .cancellation_owner import cleanup_actions
from .domain import AppError, ExitCode
from .request_transport import read

if TYPE_CHECKING:
    from collections.abc import Generator
    from typing import IO


def _watch(descriptor: int, stop: threading.Event) -> None:
    """Request cancellation on EOF and bound work that cannot handle it."""
    while not stop.wait(POLL_SECONDS):
        try:
            os.read(descriptor, 1)
        except BlockingIOError:
            continue
        except OSError:
            pass
        # EOF or data after the single frame invalidates the lifetime channel.
        # Dispatch to the handler currently installed in the main thread: the
        # processing owner or delivery guard. Scheduling alone cannot bound a
        # native call that never returns to Python, so retain a wakeable deadline.
        interrupt_main(signal.SIGINT)
        if not stop.wait(GRACE_SECONDS):
            hard_exit(INTERRUPTED_STATUS)
        return


def _join(worker: threading.Thread | None) -> None:
    """Retire a launched monitor before releasing its descriptor.

    Raises:
        RuntimeError: If the monitor does not acknowledge shutdown.

    """
    if worker is not None and worker.ident is not None:
        worker.join(GRACE_SECONDS)
        if worker.is_alive():
            message = "request owner monitor did not stop"
            raise RuntimeError(message)


@contextmanager
def sources(stream: IO[str]) -> Generator[list[str]]:
    """Read one request and retain pipe ownership until all run resources retire.

    Yields:
        The validated selection while EOF cancels its owned run.

    Raises:
        AppError: If stdin is not a usable request pipe or framing is invalid.

    """
    stop = threading.Event()
    worker: threading.Thread | None = None
    try:
        original = _pipe_input(stream)
        blocking = os.get_blocking(original)
        descriptor = os.dup(original)
    except (OSError, ValueError) as error:
        raise AppError(
            ExitCode.USAGE, "request input must be a readable pipe"
        ) from error
    primary: BaseException | None = None
    try:
        os.set_blocking(descriptor, False)
        paths = read(descriptor)
        worker = threading.Thread(
            target=_watch,
            args=(descriptor, stop),
            daemon=True,
            name="eml-request-owner",
        )
        worker.start()
        yield paths
    except BaseException as error:
        primary = error
        raise
    finally:
        cleanup_actions(
            primary,
            (
                stop.set,
                partial(_join, worker),
                partial(os.set_blocking, descriptor, blocking),
                partial(os.close, descriptor),
            ),
        )


def _pipe_input(stream: IO[str]) -> int:
    """Validate pipe input before acquiring a duplicate descriptor.

    Returns:
        The caller's borrowed pipe descriptor.

    Raises:
        AppError: If input is a terminal or ordinary file.

    """
    descriptor = stream.fileno()
    if not stat.S_ISFIFO(os.fstat(descriptor).st_mode):
        raise AppError(ExitCode.USAGE, "request input must be a readable pipe")
    return descriptor
