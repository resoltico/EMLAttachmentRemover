"""Select actionable mutation evidence before reviewed or incomplete mutants."""

from __future__ import annotations

import json
from pathlib import Path
from typing import TypedDict, cast
from unittest.mock import patch

from tools import mutation_diagnostics


class _Report(TypedDict):
    """The subset of diagnostic fields asserted by these regression tests."""

    not_killed: int
    listed: int
    mutants: list[dict[str, object]]


def _case(tmp_path: Path, lines: str) -> tuple[Path, Path, Path]:
    """Build one isolated diagnostic input without a mutation runtime.

    Returns:
        The named results, statistics, and output paths.

    """
    results = tmp_path / "mutmut-results.txt"
    results.write_text(lines, encoding="utf-8")
    stats = tmp_path / "mutmut-stats.json"
    stats.write_text("{}", encoding="utf-8")
    return results, stats, tmp_path / "diagnostics"


def _index(output: Path) -> _Report:
    """Read the emitted machine-readable summary.

    Returns:
        The diagnostics index JSON object.

    """
    return cast(
        "_Report", json.loads((output / "index.json").read_text(encoding="utf-8"))
    )


def test_slow_campaign_retains_survivors_ahead_of_unfinished_mutants(
    tmp_path: Path,
) -> None:
    """Pending mutants cannot exhaust the bounded evidence budget."""
    unfinished = "".join(
        f"pkg.a.x_f__mutmut_{n}: not checked\n"
        for n in range(1, mutation_diagnostics.MAX_MUTANTS + 5)
    )
    paths = _case(tmp_path, unfinished + "pkg.z.x_regression__mutmut_1: survived\n")
    mutation_diagnostics.collect(*paths, lambda mutant: f"patch {mutant}")
    index = _index(paths[2])
    assert index["not_killed"] == mutation_diagnostics.MAX_MUTANTS + 5
    assert index["listed"] == mutation_diagnostics.MAX_MUTANTS
    assert index["mutants"][0]["mutant"] == "pkg.z.x_regression__mutmut_1"
    assert index["mutants"][0]["status"] == "survived"


def test_unreviewed_survivor_outweighs_hundreds_of_reviewed_equivalences(
    tmp_path: Path,
) -> None:
    """A full budget of reviewed mutations must not conceal one real regression."""
    reviewed = tuple(
        f"pkg.a.x_f__mutmut_{n}" for n in range(1, mutation_diagnostics.MAX_MUTANTS + 5)
    )
    lines = "".join(f"{name}: survived\n" for name in reviewed)
    lines += "pkg.z.x_regression__mutmut_1: survived\n"
    paths = _case(tmp_path, lines)
    manifest = tmp_path / "tools" / "equivalent_mutants.json"
    with patch(
        "tools.mutation_manifest.load_equivalents",
        return_value={name: "reviewed" for name in reviewed},
    ) as loader:
        mutation_diagnostics.collect(
            *paths, lambda mutant: f"patch {mutant}", reviewed_manifest=manifest
        )
    loader.assert_called_once_with(
        manifest,
        (tmp_path / "src" / "eml_attachment_remover", tmp_path / "tools"),
    )
    index = _index(paths[2])
    assert index["mutants"][0]["mutant"] == "pkg.z.x_regression__mutmut_1"
    assert index["listed"] == mutation_diagnostics.MAX_MUTANTS


def test_invalid_review_manifest_cannot_suppress_mutation_failure_diagnostics(
    tmp_path: Path,
) -> None:
    """The strict validator owns admission; a broken review file cannot hide evidence."""
    paths = _case(tmp_path, "pkg.z.x_unreviewed__mutmut_1: survived\n")
    with patch(
        "tools.mutation_manifest.load_equivalents",
        side_effect=ValueError("stale source binding"),
    ):
        mutation_diagnostics.collect(
            *paths,
            lambda mutant: f"patch {mutant}",
            reviewed_manifest=tmp_path / "broken.json",
        )
    assert _index(paths[2])["mutants"][0]["status"] == "survived"
