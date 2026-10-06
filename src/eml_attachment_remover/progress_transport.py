"""Best-effort bounded progress writes to a borrowed cross-platform pipe."""

from __future__ import annotations

import errno
import json
import os
import stat
from contextlib import contextmanager
from typing import TYPE_CHECKING, Final

from . import native_windows_pipe
from .domain import AppError, ExitCode

if TYPE_CHECKING:
    from collections.abc import Generator

PREFIX: Final = b"EML_PROGRESS "
MIN_DESCRIPTOR: Final = 3


class ProgressPipe:
    """Emit complete small records or permanently stop advisory output."""

    def __init__(self, descriptor: int) -> None:
        """Retain a scope-owned descriptor and initially unknown batch counts."""
        self.descriptor: int | None = descriptor
        self.counts: tuple[int, int] | None = None

    def update(self, completed: int, total: int) -> None:
        """Report completed attempts; a channel error cannot fail the batch."""
        self.counts = completed, total
        self.emit("processing")

    def reporting(self) -> None:
        """Mark final-report preparation without claiming accepted outcomes."""
        self.emit("reporting")

    def emit(self, stage: str) -> None:
        """Write one atomic-sized record once; failed/partial writes disable output."""
        if self.descriptor is None or self.counts is None:
            return
        completed, total = self.counts
        record = (
            PREFIX
            + json.dumps(
                {"schema": 1, "stage": stage, "completed": completed, "total": total},
                separators=(",", ":"),
            ).encode("ascii")
            + b"\n"
        )
        try:
            written = os.write(self.descriptor, record)
        except OSError:
            self.descriptor = None
        else:
            if written != len(record):
                self.descriptor = None


@contextmanager
def open_pipe(original: int) -> Generator[ProgressPipe]:
    """Duplicate valid pipe output and restore the caller's descriptor flags.

    Yields:
        The scope-owned advisory writer.

    Raises:
        AppError: If the requested descriptor is not an available pipe above stdio.

    """
    if original < MIN_DESCRIPTOR:
        raise AppError(ExitCode.USAGE, "progress descriptor must be a pipe above stdio")
    try:
        blocking = _pipe_blocking(original)
        descriptor = os.dup(original)
    except OSError as error:
        raise AppError(ExitCode.USAGE, "progress pipe is unavailable") from error
    try:
        os.set_blocking(descriptor, False)
        try:
            os.write(descriptor, b"")
        except OSError as error:
            if error.errno == errno.EBADF:
                raise AppError(
                    ExitCode.USAGE, "progress pipe must be writable"
                ) from error
        yield ProgressPipe(descriptor)
    finally:
        try:
            os.set_blocking(descriptor, blocking)
        finally:
            os.close(descriptor)


def _pipe_blocking(original: int) -> bool:
    """Validate a pipe's type and access before borrowing its flags.

    Returns:
        Its current blocking mode.

    Raises:
        AppError: If the descriptor cannot carry progress output.

    """
    if not stat.S_ISFIFO(os.fstat(original).st_mode):
        raise AppError(ExitCode.USAGE, "progress descriptor must be a pipe")
    if os.name == "nt" and not native_windows_pipe.has_write_access(original):
        raise AppError(ExitCode.USAGE, "progress pipe must be writable")
    return os.get_blocking(original)
