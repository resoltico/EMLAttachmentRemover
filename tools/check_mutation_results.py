"""Require a complete 100% actionable mutation-testing result."""

from __future__ import annotations

import argparse
import importlib
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, cast

if TYPE_CHECKING:
    import re
    from collections.abc import Sequence
    from typing import Protocol

    class MutationManifestModule(Protocol):
        """Describe source-bound mutation-manifest validation helpers."""

        MutationResultsError: type[ValueError]
        RESULT_PATTERN: re.Pattern[str]

        def read_text(self, path: Path, label: str) -> str:
            """Read one UTF-8 evidence file."""

        def load_object(self, path: Path, label: str) -> dict[str, object]:
            """Load one strict JSON object."""

        def require_fields(
            self,
            values: dict[str, object],
            expected: frozenset[str],
            label: str,
        ) -> None:
            """Require exact object field names."""

        def source_sha256(self, source_roots: Path | Sequence[Path]) -> str:
            """Hash all production source roots."""

        def load_equivalents(
            self,
            path: Path,
            source_roots: Path | Sequence[Path],
        ) -> dict[str, str]:
            """Load and validate a reviewed-equivalent manifest."""

        def validate_equivalent_entry(
            self,
            entry_value: object,
            index: int,
        ) -> tuple[str, str]:
            """Validate one equivalent-mutant entry."""


mutation_manifest = cast(
    "MutationManifestModule",
    importlib.import_module(
        "tools.mutation_manifest" if __package__ else "mutation_manifest",
    ),
)

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
SOURCE_ROOTS: Final = (
    PROJECT_ROOT / "src" / "eml_attachment_remover",
    PROJECT_ROOT / "tools",
)
DEFAULT_STATISTICS: Final = PROJECT_ROOT / "mutants" / "mutmut-cicd-stats.json"
DEFAULT_RESULTS: Final = PROJECT_ROOT / "build" / "mutmut-results.txt"
DEFAULT_EQUIVALENTS: Final = PROJECT_ROOT / "tools" / "equivalent_mutants.json"
STATISTICS_LABEL: Final = "mutation statistics"
STATISTICS_SCHEMA_LABEL: Final = "mutation-statistics"
RESULTS_LABEL: Final = "mutation results"
KILLED_STATUS: Final = "killed"
SURVIVED_STATUS: Final = "survived"
EMPTY_RESULTS_MESSAGE: Final = "mutation results must contain at least one mutant"
ORDER_MESSAGE: Final = "mutation results must be normalized in lexical mutant-ID order"
COHERENCE_PREFIX: Final = "mutation evidence is stale or inconsistent: "
FAILURE_PREFIX: Final = "actionable mutation gate failed"
SUCCESS_PREFIX: Final = "mutation gate passed: actionable_score=100%, "
STATISTICS_HELP: Final = "mutmut JSON statistics"
RESULTS_HELP: Final = "normalized full mutmut results"
EQUIVALENTS_HELP: Final = "reviewed equivalent-mutant manifest"
STATUS_FIELDS: Final = (
    "killed",
    "survived",
    "no_tests",
    "skipped",
    "suspicious",
    "timeout",
    "check_was_interrupted_by_user",
    "segfault",
)
EXPECTED_FIELDS: Final = frozenset((*STATUS_FIELDS, "total"))
NON_KILLED_FIELDS: Final = STATUS_FIELDS[1:]
STATUS_TO_FIELD: Final[dict[str, str | None]] = {
    "killed": "killed",
    "survived": "survived",
    "no tests": "no_tests",
    "skipped": "skipped",
    "suspicious": "suspicious",
    "timeout": "timeout",
    "check was interrupted by user": "check_was_interrupted_by_user",
    "segfault": "segfault",
    "caught by type check": None,
    "not checked": None,
}
RESULT_PATTERN = mutation_manifest.RESULT_PATTERN
MutationResultsError = mutation_manifest.MutationResultsError
_read_text = mutation_manifest.read_text
_load_object = mutation_manifest.load_object
_require_fields = mutation_manifest.require_fields
_source_sha256 = mutation_manifest.source_sha256
_load_equivalents = mutation_manifest.load_equivalents
_validate_equivalent_entry = mutation_manifest.validate_equivalent_entry


@dataclass(frozen=True, slots=True)
class MutationSummary:
    """Summarize a passing actionable mutation gate."""

    killed: int
    equivalent: int
    total: int


def _build_parser() -> argparse.ArgumentParser:
    """Create the mutation-results command-line parser.

    Returns:
        The configured argument parser.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "statistics",
        nargs="?",
        type=Path,
        default=DEFAULT_STATISTICS,
        help=f"{STATISTICS_HELP} (default: {DEFAULT_STATISTICS})",
    )
    parser.add_argument(
        "--results",
        type=Path,
        default=DEFAULT_RESULTS,
        help=f"{RESULTS_HELP} (default: {DEFAULT_RESULTS})",
    )
    parser.add_argument(
        "--equivalents",
        type=Path,
        default=DEFAULT_EQUIVALENTS,
        help=f"{EQUIVALENTS_HELP} (default: {DEFAULT_EQUIVALENTS})",
    )
    return parser


def _load_counts(path: Path) -> dict[str, int]:
    """Load and strictly validate one exported statistics object.

    Returns:
        Every expected mutation-status count and the exported total.

    """
    values = _load_object(path, STATISTICS_LABEL)
    _require_fields(values, EXPECTED_FIELDS, STATISTICS_SCHEMA_LABEL)
    counts: dict[str, int] = {}
    for field in sorted(EXPECTED_FIELDS):
        value = values[field]
        if type(value) is not int or value < 0:
            message = (
                f"mutation-statistics field {field!r} must be a nonnegative integer"
            )
            raise MutationResultsError(message)
        counts[field] = value
    return counts


def _load_results(path: Path) -> dict[str, str]:
    """Parse a normalized, complete ``mutmut results --all`` capture.

    Returns:
        Every exact mutant ID mapped to its current status.

    """
    lines = _read_text(path, RESULTS_LABEL).splitlines()
    if not lines:
        raise MutationResultsError(EMPTY_RESULTS_MESSAGE)
    results: dict[str, str] = {}
    for line_number, line in enumerate(lines, start=1):
        match = RESULT_PATTERN.fullmatch(line)
        if match is None:
            message = f"invalid mutation-results line {line_number}: {line!r}"
            raise MutationResultsError(message)
        mutant = match["mutant"]
        status = match["status"]
        if status not in STATUS_TO_FIELD:
            message = f"unknown mutation status for {mutant}: {status!r}"
            raise MutationResultsError(message)
        if mutant in results:
            message = f"duplicate mutant ID in mutation results: {mutant}"
            raise MutationResultsError(message)
        results[mutant] = status
    if lines != sorted(lines):
        raise MutationResultsError(ORDER_MESSAGE)
    return results


def _validate_count_coherence(
    counts: dict[str, int],
    results: dict[str, str],
) -> None:
    """Require the aggregate export and named result capture to agree exactly."""
    actual: Counter[str] = Counter()
    for status in results.values():
        field = STATUS_TO_FIELD[status]
        actual[field if field is not None else "unexported"] += 1
    expected = {field: counts[field] for field in STATUS_FIELDS}
    expected["unexported"] = counts["total"] - sum(expected.values())
    mismatches = {
        field: (expected[field], actual[field])
        for field in (*STATUS_FIELDS, "unexported")
        if expected[field] != actual[field]
    }
    if len(results) != counts["total"] or mismatches:
        message = (
            COHERENCE_PREFIX
            + f"exported_total={counts['total']}, named_total={len(results)}, "
            f"status_mismatches={mismatches}"
        )
        raise MutationResultsError(message)


def _failure_message(
    counts: dict[str, int],
    results: dict[str, str],
    equivalents: dict[str, str],
) -> str:
    """Return exact stale, unreviewed, and misclassified-mutant diagnostics.

    Returns:
        An actionable failure summary.

    """
    stale = sorted(set(equivalents) - set(results))
    misclassified = {
        mutant: results[mutant]
        for mutant in sorted(set(equivalents) & set(results))
        if results[mutant] != SURVIVED_STATUS
    }
    unreviewed = {
        mutant: status
        for mutant, status in sorted(results.items())
        if status != KILLED_STATUS and mutant not in equivalents
    }
    details = ", ".join(
        f"{field}={counts[field]}" for field in ("killed", "total", *NON_KILLED_FIELDS)
    )
    return (
        f"{FAILURE_PREFIX} ({details}); "
        f"stale_equivalents={stale}; misclassified_equivalents={misclassified}; "
        f"unreviewed_non_killed={unreviewed}. Strengthen tests or add a reviewed "
        "exact equivalent-mutant entry with a public rationale."
    )


def check_mutation_results(
    statistics_path: Path,
    results_path: Path,
    equivalents_path: Path,
    source_roots: Path | Sequence[Path],
) -> MutationSummary:
    """Require every actionable mutant killed and every equivalent reviewed.

    Returns:
        Counts for a passing, nonempty 100% actionable mutation run.

    """
    counts = _load_counts(statistics_path)
    results = _load_results(results_path)
    equivalents = _load_equivalents(equivalents_path, source_roots)
    _validate_count_coherence(counts, results)
    actionable = {
        mutant: status
        for mutant, status in results.items()
        if mutant not in equivalents
    }
    passed = (
        bool(actionable)
        and all(status == KILLED_STATUS for status in actionable.values())
        and all(results.get(mutant) == SURVIVED_STATUS for mutant in equivalents)
    )
    if not passed:
        raise MutationResultsError(_failure_message(counts, results, equivalents))
    return MutationSummary(
        killed=counts["killed"],
        equivalent=len(equivalents),
        total=counts["total"],
    )


def main(argv: list[str] | None = None) -> int:
    """Validate mutation evidence as an independent actionable-quality gate.

    Returns:
        Zero only for a 100% actionable mutation score, otherwise one.

    """
    arguments = _build_parser().parse_args(argv)
    try:
        summary = check_mutation_results(
            arguments.statistics,
            arguments.results,
            arguments.equivalents,
            SOURCE_ROOTS,
        )
    except MutationResultsError as error:
        print(error, file=sys.stderr)
        return 1
    print(
        SUCCESS_PREFIX + f"killed={summary.killed}, equivalent={summary.equivalent}, "
        f"total={summary.total}",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
