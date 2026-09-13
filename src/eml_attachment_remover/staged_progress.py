"""Verified arithmetic for one staged native write."""

from __future__ import annotations

from .domain import AppError, ExitCode


def advance_position(position: int, written: int, size: int) -> int:
    """Return one strictly advancing in-range staged-write position.

    Returns:
        The exact next byte offset after the accepted native write.

    """
    return _validated_next_position(position, position + written, size)


def _validated_next_position(position: int, next_position: int, size: int) -> int:
    """Return one independently range-checked staged-write position.

    Returns:
        The next offset when it is strictly advanced and inside the staged candidate.

    Raises:
        AppError: If the provided transition is not a strict in-range advancement.

    """
    if not position < next_position <= size:
        raise AppError(ExitCode.WRITE_ERROR, "short write while staging candidate")
    return next_position
