"""Create verified text-only EML working copies without ordinary attachments."""

from __future__ import annotations

from typing import TYPE_CHECKING

from ._version import PROGRAM_VERSION

if TYPE_CHECKING:
    from .models import ProcessResult
    from .processing import process_file

__all__ = ["PROGRAM_VERSION", "ProcessResult", "process_file"]
__version__ = PROGRAM_VERSION


def __getattr__(name: str) -> object:
    """Load the public processing API only when a caller requests it.

    Returns:
        The requested public model or processing function.

    Raises:
        AttributeError: If the name is not part of the public lazy interface.

    """
    if name == "ProcessResult":
        from .models import (  # ruff: ignore[import-outside-top-level]
            ProcessResult,
        )

        return ProcessResult
    if name == "process_file":
        from .processing import (  # ruff: ignore[import-outside-top-level] - Lazy API.
            process_file,
        )

        return process_file
    raise AttributeError(name)
