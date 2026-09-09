"""Create structurally verified MIME-pruned EML working copies."""

from __future__ import annotations

from ._version import PROGRAM_VERSION
from .processing import process_file

__all__ = ["PROGRAM_VERSION", "process_file"]
__version__ = PROGRAM_VERSION
