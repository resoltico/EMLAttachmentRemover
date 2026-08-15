"""Apply output metadata and atomically commit verified derived files."""

from __future__ import annotations

import stat
from typing import TYPE_CHECKING

from . import atomic_publish
from .models import CliError, ExitCode
from .paths import _destination_exists, _paths_alias

if TYPE_CHECKING:
    from pathlib import Path


def _apply_source_mode(source: Path, temporary: Path) -> str | None:
    """Apply source permission bits to output as a best-effort operation.

    Returns:
        A warning when permissions cannot be copied, otherwise ``None``.

    """
    try:
        mode = stat.S_IMODE(source.stat().st_mode)
        temporary.chmod(mode)
    except OSError as exc:
        return f"could not copy source permission bits to output: {exc}"
    return None


def _validate_forced_commit(source: Path, destination: Path) -> None:
    """Reject a late source alias or directory before forced replacement.

    Raises:
        CliError: If the destination became a source alias or directory.

    """
    if _paths_alias(source, destination):
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            "refusing to overwrite the source EML through a late alias",
        )
    if _destination_exists(destination) and destination.is_dir():
        raise CliError(
            ExitCode.OUTPUT_CONFLICT,
            f"output path became a directory: {destination}",
        )


def _publish_without_clobber(temporary: Path, destination: Path) -> None:
    """Atomically publish a verified file only while the destination is absent."""
    atomic_publish.publish_without_clobber(temporary, destination)


def _commit_output(
    temporary: Path,
    destination: Path,
    source: Path,
    *,
    force: bool,
) -> None:
    """Move verified output into place with non-destructive race checks.

    Raises:
        CliError: If the destination conflicts or the replacement fails.

    """
    if not force:
        _publish_without_clobber(temporary, destination)
        return
    _validate_forced_commit(source, destination)
    try:
        temporary.replace(destination)
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not move verified output into place at {destination}: {exc}",
        ) from exc
