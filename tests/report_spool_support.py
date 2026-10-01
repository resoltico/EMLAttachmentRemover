"""Corrupt owned descriptor data without relying on an exposed spool pathname."""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from eml_attachment_remover.report_spool import ReportSpool


def replace_data(spool: ReportSpool, payload: bytes) -> None:
    """Replace backing data without changing the accounting under test."""
    spool.file.seek(0)
    spool.file.truncate()
    spool.file.write(payload)
    spool.file.flush()


def append_data(spool: ReportSpool, payload: bytes) -> None:
    """Append unaccounted bytes to an already owned private stream."""
    spool.file.seek(0, 2)
    spool.file.write(payload)
    spool.file.flush()
