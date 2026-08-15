"""Expose the small public application interface."""

from __future__ import annotations

from ._version import PROGRAM_VERSION
from .cli import main
from .models import ProcessResult
from .processing import process_file

__all__ = ["PROGRAM_VERSION", "ProcessResult", "main", "process_file"]
