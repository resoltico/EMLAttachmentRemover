"""Application facade kept deliberately thinner than the CLI boundary."""

from __future__ import annotations

from .cli import main

__all__ = ["main"]
