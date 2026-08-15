"""Mutation-resistant contracts for mutation evidence and orchestration tools."""

from __future__ import annotations

import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import check_mutation_results as gate
from tools import mutation_manifest


def _counts(**updates: int) -> dict[str, int]:
    """Return a complete synthetic mutation-statistics mapping.

    Returns:
        A fresh complete count mapping.

    """
    values = dict.fromkeys((*gate.STATUS_FIELDS, "total"), 0)
    values.update({"killed": 1, "total": 1})
    values.update(updates)
    return values


class MutationManifestContracts(unittest.TestCase):
    """Pin exact source binding, UTF-8 input, and entry diagnostics."""

    def test_source_digest_has_a_fixed_cross_platform_wire_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            (source / "nested").mkdir(parents=True)
            (source / "zeta.py").write_text("ZETA = 1\n", encoding="utf-8")
            (source / "nested" / "alpha.py").write_text(
                "ALPHA = 2\n",
                encoding="utf-8",
            )

            digest = mutation_manifest.source_sha256(source)

        self.assertEqual(
            digest,
            "5fefb360:90b9247e:6b6e8974:f685e2d8:1603c688:69c06f4e:ca9d8259:c92c344f",
        )

    def test_evidence_reader_explicitly_requests_utf8(self) -> None:
        real_read_text = Path.read_text
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "public.txt"
            path.write_text("public\n", encoding="utf-8")
            with patch.object(
                Path,
                "read_text",
                autospec=True,
                side_effect=real_read_text,
            ) as read:
                self.assertEqual(
                    mutation_manifest.read_text(path, "public evidence"),
                    "public\n",
                )
        read.assert_called_once_with(path, encoding="utf-8")

    def test_json_loader_preserves_the_exact_evidence_label(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "evidence.json"
            path.write_text("{invalid", encoding="utf-8")
            with self.assertRaises(mutation_manifest.MutationResultsError) as raised:
                mutation_manifest.load_object(path, "public evidence")
        self.assertTrue(
            str(raised.exception).startswith("public evidence is not valid JSON: ")
        )

    def test_manifest_loader_preserves_its_label_at_both_input_boundaries(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            (source / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
            missing = root / "missing.json"
            with self.assertRaises(mutation_manifest.MutationResultsError) as raised:
                mutation_manifest.load_equivalents(missing, source)
            self.assertTrue(
                str(raised.exception).startswith(
                    f"cannot read equivalent-mutant manifest {missing}: "
                )
            )

            manifest = root / "manifest.json"
            manifest.write_text("{}", encoding="utf-8")
            with self.assertRaises(mutation_manifest.MutationResultsError) as raised:
                mutation_manifest.load_equivalents(manifest, source)
        self.assertEqual(
            str(raised.exception),
            "equivalent-mutant manifest schema mismatch: "
            "missing=['equivalents', 'schema_version', 'source_sha256']; unknown=[]",
        )

    def test_equivalent_entry_diagnostics_preserve_exact_one_based_context(
        self,
    ) -> None:
        with self.assertRaises(mutation_manifest.MutationResultsError) as raised:
            mutation_manifest.validate_equivalent_entry(
                {"mutant": "public.module.x_value__mutmut_1"},
                7,
            )
        self.assertEqual(
            str(raised.exception),
            "equivalent-mutant entry 7 schema mismatch: "
            "missing=['rationale']; unknown=[]",
        )

    def test_manifest_passes_the_second_entry_index_to_validation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            source.mkdir()
            (source / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
            manifest = Path(directory) / "manifest.json"
            manifest.write_text(
                json.dumps({
                    "schema_version": 1,
                    "source_sha256": mutation_manifest.source_sha256(source),
                    "equivalents": [
                        {
                            "mutant": "public.module.x_alpha__mutmut_1",
                            "rationale": "Reviewed.",
                        },
                        {"mutant": "public.module.x_beta__mutmut_1"},
                    ],
                }),
                encoding="utf-8",
            )

            with self.assertRaises(mutation_manifest.MutationResultsError) as raised:
                mutation_manifest.load_equivalents(manifest, source)

        self.assertIn(
            "equivalent-mutant entry 1 schema mismatch", str(raised.exception)
        )


class MutationGateContracts(unittest.TestCase):
    """Pin parser types, evidence labels, line numbers, and exact diagnostics."""

    def test_parser_converts_custom_paths_and_documents_every_input(self) -> None:
        parser = gate._build_parser()  # ruff: ignore[private-member-access]
        arguments = parser.parse_args([
            "public-statistics.json",
            "--results",
            "public-results.txt",
            "--equivalents",
            "public-equivalents.json",
        ])
        self.assertEqual(arguments.statistics, Path("public-statistics.json"))
        self.assertEqual(arguments.results, Path("public-results.txt"))
        self.assertEqual(arguments.equivalents, Path("public-equivalents.json"))
        help_text = parser.format_help()
        self.assertIn(
            "Require a complete 100% actionable mutation-testing result", help_text
        )
        self.assertIn("mutmut JSON statistics", help_text)
        self.assertIn("normalized full mutmut results", help_text)
        self.assertIn("reviewed equivalent-mutant manifest", help_text)

    def test_loaders_pass_exact_public_labels_and_one_based_line_numbers(self) -> None:
        values = _counts()
        with (
            patch.object(gate, "_load_object", return_value=values) as load,
            patch.object(gate, "_require_fields") as require,
        ):
            self.assertEqual(gate._load_counts(Path("statistics.json")), values)  # ruff: ignore[private-member-access]
        load.assert_called_once_with(Path("statistics.json"), "mutation statistics")
        require.assert_called_once_with(
            values, gate.EXPECTED_FIELDS, "mutation-statistics"
        )

        results_path = Path("results.txt")
        with patch.object(
            gate,
            "_read_text",
            return_value="public.module.x_alpha__mutmut_1: killed\n",
        ) as read:
            self.assertEqual(
                gate._load_results(results_path),  # ruff: ignore[private-member-access]
                {"public.module.x_alpha__mutmut_1": "killed"},
            )
        read.assert_called_once_with(results_path, "mutation results")

        with tempfile.TemporaryDirectory() as directory:
            results = Path(directory) / "results.txt"
            results.write_text(
                "public.module.x_alpha__mutmut_1: killed\ninvalid\n",
                encoding="utf-8",
            )
            with self.assertRaises(gate.MutationResultsError) as raised:
                gate._load_results(results)  # ruff: ignore[private-member-access]
        self.assertEqual(
            str(raised.exception),
            "invalid mutation-results line 2: 'invalid'",
        )

    def test_failure_message_classifies_every_exact_set(self) -> None:
        counts = _counts(killed=1, survived=1, timeout=1, total=3)
        results = {
            "public.module.x_alpha__mutmut_1": "killed",
            "public.module.x_beta__mutmut_1": "survived",
            "public.module.x_gamma__mutmut_1": "timeout",
        }
        equivalents = {
            "public.module.x_alpha__mutmut_1": "Wrong status.",
            "public.module.x_missing__mutmut_1": "Stale.",
        }

        message = gate._failure_message(  # ruff: ignore[private-member-access]
            counts,
            results,
            equivalents,
        )

        self.assertIn("killed=1, total=3, survived=1", message)
        self.assertIn(
            "stale_equivalents=['public.module.x_missing__mutmut_1']",
            message,
        )
        self.assertIn(
            "misclassified_equivalents={'public.module.x_alpha__mutmut_1': 'killed'}",
            message,
        )
        self.assertIn(
            "unreviewed_non_killed={'public.module.x_beta__mutmut_1': 'survived', "
            "'public.module.x_gamma__mutmut_1': 'timeout'}",
            message,
        )
        self.assertNotIn(
            "'public.module.x_alpha__mutmut_1': 'killed'}. Strengthen",
            message,
        )

    def test_single_killed_mutant_is_a_valid_nonempty_actionable_run(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            (source / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
            statistics = root / "statistics.json"
            results = root / "results.txt"
            equivalents = root / "equivalents.json"
            statistics.write_text(json.dumps(_counts()), encoding="utf-8")
            results.write_text(
                "public.module.x_alpha__mutmut_1: killed\n",
                encoding="utf-8",
            )
            equivalents.write_text(
                json.dumps({
                    "schema_version": 1,
                    "source_sha256": mutation_manifest.source_sha256(source),
                    "equivalents": [],
                }),
                encoding="utf-8",
            )

            summary = gate.check_mutation_results(
                statistics,
                results,
                equivalents,
                source,
            )

        self.assertEqual(summary, gate.MutationSummary(killed=1, equivalent=0, total=1))

    def test_success_output_is_exact(self) -> None:
        summary = gate.MutationSummary(killed=2, equivalent=1, total=3)
        with (
            patch.object(gate, "check_mutation_results", return_value=summary),
            patch("sys.stdout", new_callable=io.StringIO) as stdout,
        ):
            self.assertEqual(gate.main([]), 0)
        self.assertEqual(
            stdout.getvalue(),
            "mutation gate passed: actionable_score=100%, killed=2, "
            "equivalent=1, total=3\n",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
