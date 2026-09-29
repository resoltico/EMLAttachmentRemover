"""Single-line, display-safe program diagnostics for standard error."""

from __future__ import annotations

import sys

from .domain import PROGRAM_NAME, AppError
from .native_values import safe_display


def error_line(error: AppError) -> str:
    """Format one expected failure as a single terminal-safe line.

    Returns:
        The diagnostic, including its terminating newline.

    """
    text = (
        f"{PROGRAM_NAME}: error[{error.code.name}:{int(error.code)}]: {error.message}"
    )
    return safe_display(text) + "\n"


def write_error(error: AppError) -> int:
    """Write one diagnostic to standard error.

    Returns:
        The stable exit status encoded by the error.

    """
    sys.stderr.write(error_line(error))
    return int(error.code)
