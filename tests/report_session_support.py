"""Shared construction of a real report session for lifecycle tests."""

from __future__ import annotations

from dataclasses import fields
from typing import TYPE_CHECKING

from eml_attachment_remover.report_delivery import StagedChannels
from eml_attachment_remover.report_session import ReportSession

if TYPE_CHECKING:
    from contextlib import ExitStack

    from eml_attachment_remover.domain import BatchLedger


def open_session(
    resources: ExitStack, output_format: str = "human", mode: str = "apply"
) -> ReportSession:
    """Open a real private staging pair owned by ``resources``.

    Returns:
        A session that renders, seals, and delivers to the current standard streams.

    """
    return ReportSession(
        StagedChannels.open(resources, output_format), output_format, mode
    )


def complete_owned(target: BatchLedger, completed: BatchLedger) -> BatchLedger:
    """Populate a preowned ledger in a test execution seam.

    Returns:
        The same authoritative ledger supplied by the CLI.

    """
    for value in fields(completed):
        setattr(target, value.name, getattr(completed, value.name))
    return target
