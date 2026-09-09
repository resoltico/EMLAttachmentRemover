"""Exact public batch post-publication error receipts."""

from __future__ import annotations

from eml_attachment_remover import batch
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import AppError, ExitCode


def test_publication_error_preserves_or_classifies_every_cause_exactly() -> None:
    """Post-edge causes have a closed, user-safe error mapping."""
    source_error = AppError(ExitCode.VERIFICATION_ERROR, "verified failure")
    assert batch._publication_error(source_error) is source_error  # ruff: ignore[private-member-access] - expected failures retain identity.
    assert batch._publication_error(  # ruff: ignore[private-member-access] - cancellation cause mapping.
        CancellationSignal(1, "SIGHUP")
    ) == AppError(ExitCode.INTERRUPTED, "interrupted after publication")
    assert batch._publication_error(KeyboardInterrupt()) == AppError(  # ruff: ignore[private-member-access] - keyboard cancellation mapping.
        ExitCode.INTERRUPTED, "interrupted after publication"
    )
    assert batch._publication_error(SystemExit()) == AppError(  # ruff: ignore[private-member-access] - process-exit mapping.
        ExitCode.INTERNAL_ERROR, "unexpected SystemExit after publication"
    )
    assert batch._publication_error(OSError("receipt lost")) == AppError(  # ruff: ignore[private-member-access] - ordinary post-edge failure mapping.
        ExitCode.WRITE_ERROR, "post-publication receipt failed: receipt lost"
    )
