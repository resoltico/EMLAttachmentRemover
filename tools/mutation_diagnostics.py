"""Bounded, repository-only evidence for every mutant that was not killed."""

from __future__ import annotations

import importlib
import json
import subprocess
from collections import Counter
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence
    from pathlib import Path
    from typing import Protocol

    class MutationManifestModule(Protocol):
        def load_equivalents(
            self, manifest: Path, source_roots: Sequence[Path]
        ) -> dict[str, str]: ...

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


def _reviewed_ids(manifest: Path) -> frozenset[str]:
    """Use the canonical reviewed-equivalent validator only for evidence ordering.

    Returns:
        Reviewed IDs, or an empty set when the manifest is unavailable or invalid.

    """
    service = cast(
        "MutationManifestModule",
        importlib.import_module(
            "tools.mutation_manifest" if __package__ else "mutation_manifest"
        ),
    )
    source_roots = (
        manifest.parent.parent / "src" / "eml_attachment_remover",
        manifest.parent,
    )
    try:
        return frozenset(service.load_equivalents(manifest, source_roots))
    except (OSError, ValueError):
        # The strict mutation gate reports the invalid manifest separately.
        # Diagnostics should still describe the unfinished campaign.
        return frozenset()


def collect(
    results: Path,
    stats: Path,
    output: Path,
    show: Callable[[str], str],
    *,
    reviewed_manifest: Path | None = None,
) -> None:
    """Write the diagnostics bundle: an index and one bounded patch per survivor.

    The bundle names at most ``MAX_MUTANTS`` mutants, each patch at most
    ``MAX_DIFF_BYTES`` and each test list ``MAX_TESTS`` entries, and says exactly
    what it left out, so a campaign failure can be reviewed without a re-run. A
    campaign that captured no results has nothing to bundle; its own failure is
    already reported. Review-approved equivalents follow unreviewed survivors
    within the bounded evidence budget; the former cannot hide new regressions.
    """
    if not results.is_file():
        return
    # In an interrupted campaign, hundreds of pending mutants may sort before
    # real survivors. Preserve the bounded evidence budget for substantive
    # survivors first, then the remaining statuses in deterministic name order.
    reviewed = (
        _reviewed_ids(reviewed_manifest)
        if reviewed_manifest is not None
        else frozenset()
    )
    survivors = sorted(
        _survivors(results),
        key=lambda item: (item[1] != "survived", item[0] in reviewed, item[0]),
    )
    tests = _mapped_tests(stats)
    output.mkdir(parents=True, exist_ok=True)
    for stale in output.iterdir():
        stale.unlink()
    entries: list[dict[str, object]] = []
    for number, (mutant, status) in enumerate(survivors[:MAX_MUTANTS], start=1):
        patch, patch_truncated = _patch(mutant, show)
        name = None
        if patch is not None:
            name = f"{number:04d}.diff"
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
