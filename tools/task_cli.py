"""Build the command-line interface of the repository task runner."""

from __future__ import annotations

import argparse
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable

TASKS: Final = (
    "build",
    "check",
    "ci",
    "coverage",
    "mutation",
    "quality",
    "release",
    "test",
    "thorough",
)
WORKERS_HELP: Final = "Mutmut workers: auto or 1-64 (default: auto)"


def build_parser(
    description: str | None,
    positive_timeout: Callable[[str], float],
    workers: Callable[[str], int | None],
) -> argparse.ArgumentParser:
    """Create the task parser with every documented task and option.

    Returns:
        The configured argument parser.

    """
    parser = argparse.ArgumentParser(description=description)
    tasks = parser.add_subparsers(dest="task", required=True)
    parsers = {name: tasks.add_parser(name) for name in TASKS}
    parsers["ci"].add_argument(
        "--release-tag",
        help="tag to validate like the release workflow (default: v<version>)",
    )
    parsers["mutation"].add_argument(
        "--allow-stale-manifest",
        action="store_true",
        help="skip only the equivalence preflight, to gather rebind evidence",
    )
    for name in ("ci", "mutation"):
        parsers[name].add_argument("--workers", type=workers, help=WORKERS_HELP)
    parsers["quality"].add_argument(
        "--native",
        action="store_true",
        help="skip the static checks that another lane runs; keep audit and coverage",
    )
    parsers["thorough"].add_argument(
        "--timeout-seconds",
        type=positive_timeout,
        help="whole pytest timeout",
    )
    parsers["thorough"].add_argument(
        "--observable",
        action="store_true",
        help="write public Hypothesis observations",
    )
    return parser
