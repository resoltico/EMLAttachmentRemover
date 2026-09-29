"""Exclusive lease on one checkout's mutation workspace and evidence paths."""

from __future__ import annotations

import os
import sys
from contextlib import contextmanager
from typing import TYPE_CHECKING, Final

if sys.platform != "win32":
    import fcntl

if TYPE_CHECKING:
    from collections.abc import Iterator
    from pathlib import Path

LOCK_NAME: Final = "mutation.lock"
HOLDER_BYTES: Final = 32
UNKNOWN_HOLDER: Final = "unknown"


@contextmanager
def lease(build_directory: Path) -> Iterator[None]:
    """Own the checkout's ``mutants/`` workspace and evidence until the block ends.

    The kernel drops the lock with its holder, so a crashed run never leaves a stale
    lease; the recorded pid only names the current holder for the diagnostic.

    Yields:
        Control while this process is the only mutation run in the checkout.

    Raises:
        RuntimeError: If another mutation run already owns this checkout.

    """
    build_directory.mkdir(exist_ok=True)
    descriptor = os.open(
        build_directory / LOCK_NAME,
        os.O_RDWR | os.O_CREAT | getattr(os, "O_CLOEXEC", 0),
        0o600,
    )
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            holder = os.pread(descriptor, HOLDER_BYTES, 0).decode("ascii", "replace")
            message = (
                "another mutation run owns this checkout "
                f"(pid {holder.strip() or UNKNOWN_HOLDER}); wait for it to finish "
                "or use a separate checkout"
            )
            raise RuntimeError(message) from error
        os.ftruncate(descriptor, 0)
        os.pwrite(descriptor, f"{os.getpid()}\n".encode("ascii"), 0)
        yield
    finally:
        os.close(descriptor)
