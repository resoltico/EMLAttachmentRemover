"""Shared, safely encoded batch diagnostics outside machine-readable stdout."""

from __future__ import annotations

import sys
from collections.abc import Mapping

from .domain import AppError
from .native_values import safe_display
from .report_diagnostics import bounded_message


def batch_error_line(error: object) -> str:
    """Format one bounded batch error for the final diagnostic channel.

    Returns:
        A safely encoded line, or an empty string when no batch error exists.

    """
    if isinstance(error, AppError):
        code, message = error.code.name, error.message
    elif isinstance(error, Mapping):
        code, message = str(error.get("code")), str(error.get("message"))
    else:
        return ""
    text = safe_display(f"Batch: {code}: {bounded_message(message)}")
    encoding = sys.stderr.encoding or "utf-8"
    return text.encode(encoding, "backslashreplace").decode(encoding) + "\n"


def write_batch_error(error: AppError | None) -> None:
    """Emit the batch error once, separately from per-item publication outcomes."""
    if line := batch_error_line(error):
        sys.stderr.write(line)


def interruption_line(reason: str) -> str:
    """Render one invocation-level notice independently of successful item results.

    Returns:
        A bounded, channel-safe explanatory line including its newline.

    """
    text = safe_display("Interrupted: " + bounded_message(reason))
    encoding = sys.stderr.encoding or "utf-8"
    return text.encode(encoding, "backslashreplace").decode(encoding) + "\n"
