"""Translate genuine post-publication failures without losing the receipt."""

from __future__ import annotations

from .cancellation import CancellationSignal
from .domain import AppError, ExitCode


def publication_error(cause: BaseException) -> AppError:
    """Keep the original failure class in a detached publication error.

    Returns:
        The expected application failure associated with the strongest receipt.

    """
    if isinstance(cause, AppError):
        return cause
    if isinstance(cause, (CancellationSignal, KeyboardInterrupt)):
        return AppError(ExitCode.INTERRUPTED, "interrupted after publication")
    if isinstance(cause, SystemExit):
        return AppError(
            ExitCode.INTERNAL_ERROR, "unexpected SystemExit after publication"
        )
    return AppError(ExitCode.WRITE_ERROR, f"post-publication receipt failed: {cause}")
