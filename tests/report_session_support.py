"""Shared construction of a real report session for lifecycle tests."""

from __future__ import annotations

from typing import TYPE_CHECKING

from eml_attachment_remover.report_delivery import StagedChannels
from eml_attachment_remover.report_session import ReportSession

if TYPE_CHECKING:
    from contextlib import ExitStack


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
