"""Parse the bounded timeout used by repository-owned tasks."""

from __future__ import annotations

import argparse
import math


def test_timeout(profile: str | None) -> int:
    """Allow full qualification on slower hosts while bounding development runs.

    Returns:
        The test-process allowance in seconds for the selected profile.

    """
    return 1_800 if profile in {"project-ci", "project-thorough"} else 600


def positive_timeout(value: str) -> float:
    """Parse a positive task timeout.

    Returns:
        The positive number of seconds.

    Raises:
        argparse.ArgumentTypeError: If the value is not a positive number.

    """
    try:
        timeout = float(value)
    except ValueError as error:
        message = "timeout must be a positive number of seconds"
        raise argparse.ArgumentTypeError(message) from error
    if not math.isfinite(timeout) or timeout <= 0:
        message = "timeout must be finite and greater than zero"
        raise argparse.ArgumentTypeError(message)
    return timeout
