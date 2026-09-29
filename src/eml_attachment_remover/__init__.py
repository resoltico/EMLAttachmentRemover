"""Create structurally verified MIME-pruned EML working copies."""

from __future__ import annotations

from ._version import program_version
from .processing import process_file

__all__ = [  # ruff: ignore[undefined-export] - PROGRAM_VERSION is resolved on first access by __getattr__.
    "PROGRAM_VERSION",
    "process_file",
]


def __getattr__(name: str) -> str:
    """Resolve the version names on first access instead of at import.

    Returns:
        The program version for ``PROGRAM_VERSION`` and ``__version__``.

    Raises:
        AttributeError: If any other missing attribute is requested.

    """
    if name in {"PROGRAM_VERSION", "__version__"}:
        return program_version()
    message = f"module {__name__!r} has no attribute {name!r}"
    raise AttributeError(message)
