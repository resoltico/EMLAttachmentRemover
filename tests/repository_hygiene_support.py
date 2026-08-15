"""Shared synthetic repository helpers for hygiene tests."""

from __future__ import annotations

from pathlib import Path

from tools import check_repository_hygiene as hygiene


def make_public_root(directory: str) -> Path:
    """Create a minimal documented public repository root.

    Returns:
        The populated temporary root.

    """
    root = Path(directory).resolve()
    for name in hygiene.PUBLIC_ROOT_DIRECTORIES:
        (root / name).mkdir()
    for name in hygiene.PUBLIC_ROOT_FILES:
        (root / name).write_text("PUBLIC\n", encoding="utf-8")
    return root


def issue_messages(audit: hygiene.HygieneAudit) -> list[str]:
    """Return all diagnostics without their paths.

    Returns:
        The ordered diagnostic messages.

    """
    return [issue.message for issue in audit.issues]
