"""Diagnostic limits in characters and canonical ASCII JSON content bytes."""

from __future__ import annotations

import json
from typing import Final

MAX_ERROR_MESSAGE: Final = 2048
MAX_ERROR_BYTES: Final = 6 * MAX_ERROR_MESSAGE
ELLIPSIS: Final = "\u2026"


def bounded_message(message: str) -> str:
    """Keep a diagnostic within both limits, including its truncation marker.

    Returns:
        The original message or a prefix followed by an ellipsis.

    """
    if len(message) > MAX_ERROR_MESSAGE:
        message = message[: MAX_ERROR_MESSAGE - 1] + ELLIPSIS
    costs = [len(json.dumps(character, ensure_ascii=True)) - 2 for character in message]
    if sum(costs) <= MAX_ERROR_BYTES:
        return message
    available = MAX_ERROR_BYTES - (len(json.dumps(ELLIPSIS, ensure_ascii=True)) - 2)
    used = 0
    index = 0
    while used + costs[index] <= available:
        used += costs[index]
        index += 1
    return message[:index] + ELLIPSIS
