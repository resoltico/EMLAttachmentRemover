"""Bounded, repository-only evidence for every mutant that was not killed."""

from __future__ import annotations

import json
import subprocess
from collections import Counter
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

UTF8: Final = "utf-8"
KILLED: Final = "killed"
INDEX_NAME: Final = "index.json"
MAX_MUTANTS: Final = 100
MAX_DIFF_BYTES: Final = 16 * 1024
MAX_TESTS: Final = 25
MUTANT_SEPARATOR: Final = "__mutmut_"
TESTS_KEY: Final = "tests_by_mangled_function_name"
UNAVAILABLE: Final = "patch unavailable"


def _survivors(results: Path) -> list[tuple[str, str]]:
    """Read every non-killed mutant and its status, in the capture's lexical order.

    Returns:
        ``(mutant id, status)`` pairs.

    """
    pairs = (
        line.rpartition(": ")[::2]
        for line in results.read_text(encoding=UTF8).splitlines()
        if line
    )
    return [(mutant, status) for mutant, status in pairs if status != KILLED]


def _mapped_tests(stats: Path) -> dict[str, list[str]]:
    """Load mutmut's function-to-test map when the campaign produced one.

    Returns:
        Test node IDs by mangled function name, or an empty map.

    """
    if not stats.is_file():
        return {}
    mapping = json.loads(stats.read_text(encoding=UTF8)).get(TESTS_KEY, {})
    return {name: list(tests) for name, tests in mapping.items()}


def _patch(mutant: str, show: Callable[[str], str]) -> tuple[bytes | None, bool]:
    """Fetch one mutant's diff, bounded, or report that it is unavailable.

    Returns:
        The diff bytes (truncated to the bound) and whether they were truncated.

    """
    try:
        text = show(mutant).encode(UTF8)
    except OSError, subprocess.SubprocessError:
        return None, False
    return text[:MAX_DIFF_BYTES], len(text) > MAX_DIFF_BYTES


def collect(
    results: Path,
    stats: Path,
    output: Path,
    show: Callable[[str], str],
) -> None:
    """Write the diagnostics bundle: an index and one bounded patch per survivor.

    The bundle names at most ``MAX_MUTANTS`` mutants, each patch at most
    ``MAX_DIFF_BYTES`` and each test list ``MAX_TESTS`` entries, and says exactly
    what it left out, so a campaign failure can be reviewed without a re-run. A
    campaign that captured no results has nothing to bundle; its own failure is
    already reported.
    """
    if not results.is_file():
        return
    survivors = _survivors(results)
    tests = _mapped_tests(stats)
    output.mkdir(parents=True, exist_ok=True)
    for stale in output.iterdir():
        stale.unlink()
    entries: list[dict[str, object]] = []
    for number, (mutant, status) in enumerate(survivors[:MAX_MUTANTS], start=1):
        patch, patch_truncated = _patch(mutant, show)
        name = None if patch is None else f"{number:04d}.diff"
        if name is not None and patch is not None:
            (output / name).write_bytes(patch)
        mapped = tests.get(mutant.rpartition(MUTANT_SEPARATOR)[0], [])
        entries.append({
            "mutant": mutant,
            "status": status,
            "patch": name if name is not None else UNAVAILABLE,
            "patch_truncated": patch_truncated,
            "tests": mapped[:MAX_TESTS],
            "tests_truncated": len(mapped) > MAX_TESTS,
        })
    index = {
        "not_killed": len(survivors),
        "listed": len(entries),
        "by_status": dict(sorted(Counter(status for _, status in survivors).items())),
        "mutants": entries,
    }
    (output / INDEX_NAME).write_text(
        json.dumps(index, indent=2, sort_keys=True) + "\n", encoding=UTF8
    )
