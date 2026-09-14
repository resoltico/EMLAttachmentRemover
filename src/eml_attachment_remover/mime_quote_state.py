"""Closed internal state for bounded structured-field quote scanning."""

from __future__ import annotations

from typing import Final

from .domain import AppError, ExitCode

ESCAPE_CLEAR: Final = 0
ESCAPE_PENDING: Final = 1
QUOTE_CLOSED: Final = 0
QUOTE_OPEN: Final = 1


def validated_escape_state(value: object) -> int:
    """Return a legal internal quote-escape state or fail closed.

    Returns:
        The validated escape-state marker.

    Raises:
        AppError: If the parser state is outside its closed two-value domain.

    """
    if value not in {ESCAPE_CLEAR, ESCAPE_PENDING}:
        raise AppError(ExitCode.PARSE_ERROR, "invalid MIME quoted escape state")
    return ESCAPE_PENDING if value == ESCAPE_PENDING else ESCAPE_CLEAR
