"""Bound a fuzz stage's combined diagnostic stream without limiting build files."""

from __future__ import annotations

import sys
from pathlib import Path
from typing import BinaryIO


def write_log(source: BinaryIO, destination: Path, limit: int) -> None:
    """Copy at most ``limit`` bytes and fail if diagnostics exceed that budget.

    Raises:
        ValueError: If the limit is invalid or the stream exceeds it.

    """
    if limit < 1:
        message = "fuzz log byte limit must be positive"
        raise ValueError(message)
    remaining = limit
    with destination.open("xb") as output:
        while chunk := source.read(min(65536, remaining + 1)):
            output.write(chunk[:remaining])
            output.flush()
            remaining -= len(chunk)
            if remaining < 0:
                message = f"fuzz diagnostics exceed {limit} bytes: {destination}"
                raise ValueError(message)


if __name__ == "__main__":
    with open(0, "rb", buffering=0, closefd=False) as stream:
        write_log(stream, Path(sys.argv[1]), int(sys.argv[2]))
