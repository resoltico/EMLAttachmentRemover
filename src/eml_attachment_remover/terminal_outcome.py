"""One immutable publication/status/error commitment for a completed input."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .domain import AppError, ItemStatus, PublicationReceipt


@dataclass(frozen=True, slots=True)
class TerminalOutcome:
    """The terminal decision is published to readers through one reference."""

    status: ItemStatus
    error: AppError | None
    publication: PublicationReceipt | None
