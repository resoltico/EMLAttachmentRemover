"""Keep terminal I/O failures from escaping the command-line boundary."""

from __future__ import annotations

import os
import sys
from contextlib import suppress
from typing import Final

BROKEN_PIPE_STATUS: Final = 1


def handle_broken_pipe() -> int:
    """Redirect stdout to the null device and return a stable failure status.

    Returns:
        The documented quiet broken-pipe status.

    """
    descriptor: int | None = None
    try:
        descriptor = os.open(os.devnull, os.O_WRONLY)
        os.dup2(descriptor, sys.stdout.fileno())
    except OSError:
        pass
    finally:
        if descriptor is not None:
            with suppress(OSError):
                os.close(descriptor)
    return BROKEN_PIPE_STATUS


def quiet_error_status() -> int:
    """Return a stable status when even the error stream cannot be written.

    Returns:
        The internal-error status without another write attempt.

    """
    return 70
