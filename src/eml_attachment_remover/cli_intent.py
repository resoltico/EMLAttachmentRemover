"""Parsed report intent, with conservative inference only for parsing failures."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

from .cli_parser import raw_dry_run_requested, raw_json_requested, raw_source_candidates

if TYPE_CHECKING:
    from argparse import Namespace


@dataclass(frozen=True, slots=True)
class OutputIntent:
    """The selected output contract and exact source arguments before validation."""

    output_format: str
    mode: str
    sources: tuple[str, ...]

    @classmethod
    def parsed(cls, namespace: Namespace) -> OutputIntent:
        """Capture the parser's final selection, including repeated options.

        Returns:
            The selected format, mode, and input sequence.

        """
        return cls(
            str(namespace.output_format),
            "dry-run" if namespace.dry_run else "apply",
            tuple(namespace.source),
        )

    @classmethod
    def inferred(cls, arguments: list[str]) -> OutputIntent:
        """Infer intent when argument parsing itself failed, respecting ``--``.

        Returns:
            The recoverable raw request facts.

        """
        return cls(
            "json" if raw_json_requested(arguments) else "human",
            "dry-run" if raw_dry_run_requested(arguments) else "apply",
            tuple(raw_source_candidates(arguments)),
        )
