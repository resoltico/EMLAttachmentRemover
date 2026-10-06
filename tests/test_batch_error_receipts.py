"""Exact public batch post-publication error receipts."""

from __future__ import annotations

from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.publication_error import publication_error


def test_publication_error_preserves_or_classifies_every_cause_exactly() -> None:
    """Post-edge causes have a closed, user-safe error mapping."""
    source_error = AppError(ExitCode.VERIFICATION_ERROR, "verified failure")
    assert publication_error(source_error) is source_error
    assert publication_error(CancellationSignal(1, "SIGHUP")) == AppError(
        ExitCode.INTERRUPTED, "interrupted after publication"
    )
    assert publication_error(KeyboardInterrupt()) == AppError(
        ExitCode.INTERRUPTED, "interrupted after publication"
    )
    assert publication_error(SystemExit()) == AppError(
        ExitCode.INTERNAL_ERROR, "unexpected SystemExit after publication"
    )
    assert publication_error(OSError("receipt lost")) == AppError(
        ExitCode.WRITE_ERROR, "post-publication receipt failed: receipt lost"
    )
