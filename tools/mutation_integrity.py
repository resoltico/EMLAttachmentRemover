"""Reject mutation evidence collected from a changed shared source workspace."""

from __future__ import annotations

import importlib
import json
from contextlib import contextmanager
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Generator
    from pathlib import Path

    from tools import check_repository_hygiene, mutmut_workspace
else:
    check_repository_hygiene = importlib.import_module(
        "tools.check_repository_hygiene" if __package__ else "check_repository_hygiene"
    )
    mutmut_workspace = importlib.import_module(
        "tools.mutmut_workspace" if __package__ else "mutmut_workspace"
    )

CHECKPOINT_VARIABLE: Final = "EML_MUTATION_WORKSPACE_CHECKPOINT"
FINGERPRINT_FIELDS: Final = (
    "st_dev",
    "st_ino",
    "st_mode",
    "st_size",
    "st_mtime_ns",
    "st_ctime_ns",
)


def _inventory(root: Path) -> dict[str, list[int]]:
    """Inspect approved source inputs without trusting generated mutable sidecars.

    Returns:
        Relative input names and their file identity, mode and change metadata.

    Raises:
        RuntimeError: If the generated workspace contains unapproved artifacts.

    """
    with mutmut_workspace.policy_marker("stats"):
        audited = check_repository_hygiene.audit_repository(root)
    if audited.issues:
        details = [
            f"{issue.path.relative_to(root).as_posix()}: {issue.message}"
            for issue in audited.issues
        ]
        raise RuntimeError(
            "mutation workspace contains unexpected artifacts: " + "; ".join(details)
        )
    result: dict[str, list[int]] = {}
    for path in audited.public_files:
        metadata = path.stat(follow_symlinks=False)
        result[path.relative_to(root).as_posix()] = [
            int(getattr(metadata, field)) for field in FINGERPRINT_FIELDS
        ]
    return result


def capture(root: Path, checkpoint: Path) -> None:
    """Seal the first clean statistics session's source inventory."""
    if checkpoint.exists():
        return
    inventory = _inventory(root)
    with checkpoint.open("x", encoding="utf-8") as output:
        checkpoint.chmod(0o600)
        json.dump(inventory, output, sort_keys=True)


def verify(root: Path, checkpoint: Path) -> None:
    """Require the completed campaign to leave its source inputs unchanged.

    Raises:
        RuntimeError: If its checkpoint is absent or source inputs changed.

    """
    if not checkpoint.is_file():
        message = "mutation workspace integrity checkpoint is missing"
        raise RuntimeError(message)
    expected: object = json.loads(checkpoint.read_text(encoding="utf-8"))
    if not isinstance(expected, dict) or not all(
        isinstance(name, str)
        and isinstance(values, list)
        and len(values) == len(FINGERPRINT_FIELDS)
        and all(type(value) is int for value in values)
        for name, values in expected.items()
    ):
        message = "mutation workspace integrity checkpoint is invalid"
        raise RuntimeError(message)
    actual = _inventory(root)
    if expected != actual:
        changed = sorted(
            name
            for name in expected.keys() | actual.keys()
            if expected.get(name) != actual.get(name)
        )
        raise RuntimeError(
            "mutation workspace source inputs changed: " + ", ".join(changed)
        )


@contextmanager
def guard(root: Path, checkpoint: Path) -> Generator[None]:
    """Check integrity in the parent, preserving a primary campaign failure.

    Yields:
        Control to the mutation command.

    """
    try:
        yield
    except BaseException as error:
        try:
            verify(root, checkpoint)
        except (OSError, ValueError, RuntimeError) as integrity_error:
            error.add_note(str(integrity_error))
        raise
    else:
        verify(root, checkpoint)
