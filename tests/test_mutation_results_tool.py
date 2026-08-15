"""Behavioral tests for the independent actionable-mutation gate."""

from __future__ import annotations

import io
import json
import tempfile
import unittest
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from tools import check_mutation_results as gate

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence

MUTANTS = (
    "public.module.x_alpha__mutmut_1",
    "public.module.x_beta__mutmut_1",
    "public.module.x_gamma__mutmut_1",
)


def _write_json(path: Path, content: object) -> None:
    """Serialize synthetic public JSON evidence."""
    path.write_text(json.dumps(content), encoding="utf-8")


def _counts(**updates: int) -> dict[str, int]:
    """Return a complete synthetic all-killed export.

    Returns:
        A fresh count object with caller-provided overrides.

    """
    result = {
        "killed": 2,
        "survived": 0,
        "total": 2,
        "no_tests": 0,
        "skipped": 0,
        "suspicious": 0,
        "timeout": 0,
        "check_was_interrupted_by_user": 0,
        "segfault": 0,
    }
    result.update(updates)
    return result


def _write_results(path: Path, statuses: dict[str, str]) -> None:
    """Write normalized named-mutant results."""
    path.write_text(
        "".join(f"{name}: {statuses[name]}\n" for name in sorted(statuses)),
        encoding="utf-8",
    )


def _make_source(base: Path) -> Path:
    """Create a minimal public source tree.

    Returns:
        The source root.

    """
    source = base / "source"
    source.mkdir()
    (source / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
    return source


def _write_evidence(
    base: Path,
    counts: dict[str, int],
    statuses: dict[str, str],
    equivalents: Sequence[Mapping[str, object]],
) -> tuple[Path, Path, Path, Path]:
    """Write one complete source-bound evidence set.

    Returns:
        Statistics, named results, manifest, and source paths.

    """
    statistics = base / "statistics.json"
    results = base / "results.txt"
    manifest = base / "manifest.json"
    source = _make_source(base)
    _write_json(statistics, counts)
    _write_results(results, statuses)
    _write_json(
        manifest,
        {
            "schema_version": 1,
            "source_sha256": gate._source_sha256(source),  # ruff: ignore[private-member-access]
            "equivalents": equivalents,
        },
    )
    return statistics, results, manifest, source


class MutationJsonTests(unittest.TestCase):
    """Exercise shared JSON and strict aggregate-count validation."""

    def test_counts_accept_exact_schema_and_reject_invalid_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "statistics.json"
            expected = _counts()
            _write_json(path, expected)
            self.assertEqual(gate._load_counts(path), expected)  # ruff: ignore[private-member-access]
            for invalid in (True, -1):
                content: dict[str, object] = dict(expected)
                content["survived"] = invalid
                _write_json(path, content)
                with (
                    self.subTest(invalid=invalid),
                    self.assertRaisesRegex(gate.MutationResultsError, "nonnegative"),
                ):
                    gate._load_counts(path)  # ruff: ignore[private-member-access]

    def test_json_loader_reports_read_syntax_outer_and_key_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.json"
            with self.assertRaisesRegex(gate.MutationResultsError, "cannot read"):
                gate._load_object(path, "public evidence")  # ruff: ignore[private-member-access]
            path.write_text("{invalid", encoding="utf-8")
            with self.assertRaisesRegex(gate.MutationResultsError, "not valid JSON"):
                gate._load_object(path, "public evidence")  # ruff: ignore[private-member-access]
            _write_json(path, [])
            with self.assertRaisesRegex(gate.MutationResultsError, "JSON object"):
                gate._load_object(path, "public evidence")  # ruff: ignore[private-member-access]
            with (
                patch("tools.mutation_manifest.json.loads", return_value={1: 2}),
                self.assertRaisesRegex(gate.MutationResultsError, "must be strings"),
            ):
                gate._load_object(path, "public evidence")  # ruff: ignore[private-member-access]

    def test_counts_report_missing_and_unknown_fields(self) -> None:
        content = _counts()
        del content["survived"]
        content["unknown"] = 0
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "statistics.json"
            _write_json(path, content)
            with self.assertRaisesRegex(
                gate.MutationResultsError,
                r"missing=\['survived'\]; unknown=\['unknown'\]",
            ):
                gate._load_counts(path)  # ruff: ignore[private-member-access]


class NamedResultTests(unittest.TestCase):
    """Exercise exact named-status parsing and aggregate coherence."""

    def test_results_accept_every_known_status(self) -> None:
        statuses = {
            f"public.module.x_value__mutmut_{index}": status
            for index, status in enumerate(gate.STATUS_TO_FIELD, start=10)
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.txt"
            _write_results(path, statuses)
            self.assertEqual(gate._load_results(path), statuses)  # ruff: ignore[private-member-access]

    def test_results_reject_invalid_evidence(self) -> None:
        cases = (
            ("", "at least one mutant"),
            ("not a result\n", "invalid mutation-results line"),
            (f"{MUTANTS[0]}: unknown\n", "unknown mutation status"),
            (
                f"{MUTANTS[0]}: killed\n{MUTANTS[0]}: survived\n",
                "duplicate mutant ID",
            ),
            (
                f"{MUTANTS[1]}: killed\n{MUTANTS[0]}: killed\n",
                "lexical mutant-ID order",
            ),
        )
        for content, message in cases:
            with (
                self.subTest(message=message),
                tempfile.TemporaryDirectory() as directory,
            ):
                path = Path(directory) / "results.txt"
                path.write_text(content, encoding="utf-8")
                with self.assertRaisesRegex(gate.MutationResultsError, message):
                    gate._load_results(path)  # ruff: ignore[private-member-access]

    def test_count_coherence_includes_unexported_and_rejects_mismatch(self) -> None:
        results = {
            f"public.module.x_value__mutmut_{index}": status
            for index, status in enumerate(gate.STATUS_TO_FIELD, start=10)
        }
        counts = dict.fromkeys(gate.STATUS_FIELDS, 0)
        for field in gate.STATUS_TO_FIELD.values():
            if field is not None:
                counts[field] += 1
        counts["total"] = len(results)
        gate._validate_count_coherence(counts, results)  # ruff: ignore[private-member-access]
        for invalid in (_counts(total=3), _counts(killed=1, survived=1)):
            with self.assertRaisesRegex(gate.MutationResultsError, "inconsistent"):
                gate._validate_count_coherence(  # ruff: ignore[private-member-access]
                    invalid,
                    {MUTANTS[0]: "killed", MUTANTS[1]: "killed"},
                )


class ActionableGateTests(unittest.TestCase):
    """Exercise passing and failing actionable mutation outcomes."""

    def test_gate_passes_all_killed_and_reviewed_equivalent_runs(self) -> None:
        cases: tuple[
            tuple[
                dict[str, int],
                dict[str, str],
                list[dict[str, object]],
                gate.MutationSummary,
            ],
            ...,
        ] = (
            (
                _counts(),
                {MUTANTS[0]: "killed", MUTANTS[1]: "killed"},
                [],
                gate.MutationSummary(2, 0, 2),
            ),
            (
                _counts(killed=1, survived=1),
                {MUTANTS[0]: "killed", MUTANTS[1]: "survived"},
                [{"mutant": MUTANTS[1], "rationale": "Reviewed equivalent."}],
                gate.MutationSummary(1, 1, 2),
            ),
        )
        for counts, statuses, equivalents, expected in cases:
            with tempfile.TemporaryDirectory() as directory:
                inputs = _write_evidence(Path(directory), counts, statuses, equivalents)
                self.assertEqual(gate.check_mutation_results(*inputs), expected)

    def test_gate_rejects_empty_all_equivalent_and_unreviewed_runs(self) -> None:
        cases: tuple[
            tuple[dict[str, int], dict[str, str], list[dict[str, object]]],
            ...,
        ] = (
            (_counts(killed=0, total=0), {}, []),
            (
                _counts(killed=0, survived=1, total=1),
                {MUTANTS[0]: "survived"},
                [{"mutant": MUTANTS[0], "rationale": "Reviewed equivalent."}],
            ),
            (
                _counts(killed=1, survived=1),
                {MUTANTS[0]: "killed", MUTANTS[1]: "survived"},
                [],
            ),
        )
        for index, (counts, statuses, equivalents) in enumerate(cases):
            with tempfile.TemporaryDirectory() as directory:
                inputs = _write_evidence(Path(directory), counts, statuses, equivalents)
                expected_message = (
                    "at least one mutant"
                    if index == 0
                    else "actionable mutation gate failed"
                )
                with self.assertRaisesRegex(
                    gate.MutationResultsError,
                    expected_message,
                ):
                    gate.check_mutation_results(*inputs)

    def test_failure_reports_stale_misclassified_and_unreviewed_ids(self) -> None:
        equivalents: list[dict[str, object]] = [
            {"mutant": MUTANTS[0], "rationale": "Wrong current status."},
            {
                "mutant": "public.module.x_missing__mutmut_1",
                "rationale": "No longer generated.",
            },
        ]
        with tempfile.TemporaryDirectory() as directory:
            inputs = _write_evidence(
                Path(directory),
                _counts(killed=1, survived=1, timeout=1, total=3),
                {
                    MUTANTS[0]: "killed",
                    MUTANTS[1]: "survived",
                    MUTANTS[2]: "timeout",
                },
                equivalents,
            )
            with self.assertRaises(gate.MutationResultsError) as raised:
                gate.check_mutation_results(*inputs)
        message = str(raised.exception)
        self.assertIn(
            "stale_equivalents=['public.module.x_missing__mutmut_1']", message
        )
        self.assertIn(f"'{MUTANTS[0]}': 'killed'", message)
        self.assertIn(f"'{MUTANTS[1]}': 'survived'", message)
        self.assertIn(f"'{MUTANTS[2]}': 'timeout'", message)

    def test_failure_excludes_killed_actionable_mutants_from_unreviewed(self) -> None:
        message = gate._failure_message(  # ruff: ignore[private-member-access]
            _counts(killed=1, survived=1),
            {MUTANTS[0]: "killed", MUTANTS[1]: "survived"},
            {},
        )

        self.assertIn(
            f"unreviewed_non_killed={{'{MUTANTS[1]}': 'survived'}}. Strengthen",
            message,
        )


class MutationCommandTests(unittest.TestCase):
    """Exercise parser defaults and both command exit statuses."""

    def test_main_prints_pass_and_failure(self) -> None:
        summary = gate.MutationSummary(killed=2, equivalent=1, total=3)
        with (
            patch.object(gate, "check_mutation_results", return_value=summary) as check,
            patch("sys.stdout", new_callable=io.StringIO) as stdout,
        ):
            self.assertEqual(gate.main([]), 0)
        check.assert_called_once_with(
            gate.DEFAULT_STATISTICS,
            gate.DEFAULT_RESULTS,
            gate.DEFAULT_EQUIVALENTS,
            gate.SOURCE_ROOTS,
        )
        self.assertIn("actionable_score=100%", stdout.getvalue())
        with (
            patch.object(
                gate,
                "check_mutation_results",
                side_effect=gate.MutationResultsError("public error"),
            ),
            patch("sys.stderr", new_callable=io.StringIO) as stderr,
        ):
            self.assertEqual(gate.main([]), 1)
        self.assertEqual(stderr.getvalue(), "public error\n")

    def test_parser_exposes_all_default_paths(self) -> None:
        arguments = gate._build_parser().parse_args([])  # ruff: ignore[private-member-access]
        self.assertEqual(arguments.statistics, gate.DEFAULT_STATISTICS)
        self.assertEqual(arguments.results, gate.DEFAULT_RESULTS)
        self.assertEqual(arguments.equivalents, gate.DEFAULT_EQUIVALENTS)


if __name__ == "__main__":
    unittest.main(verbosity=2)
