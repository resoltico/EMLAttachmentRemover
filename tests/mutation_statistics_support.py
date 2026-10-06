"""Separate mutation workspace setup from the calls exercised by a test."""

from __future__ import annotations

from typing import TYPE_CHECKING

from mutmut.state import state as mutation_state
from tools import mutation_integrity

if TYPE_CHECKING:
    from pathlib import Path


def capture_workspace(workspace: Path, checkpoint: Path) -> None:
    """Seal source inputs without attributing safety setup to every test.

    A test's own calls remain measured. Capture errors still abort the session.
    """
    calls = mutation_state()._stats  # ruff: ignore[private-member-access] - pinned Mutmut measured-call mapping.
    measured = calls.copy()
    try:
        mutation_integrity.capture(workspace, checkpoint)
    finally:
        calls.intersection_update(measured)
