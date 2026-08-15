"""Behavioral tests for source-bound equivalent-mutant manifests."""

from __future__ import annotations

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
)


def _make_source(base: Path) -> Path:
    """Create a small public source tree.

    Returns:
        The source root.

    """
    source = base / "source"
    (source / "nested").mkdir(parents=True)
    (source / "zeta.py").write_text("ZETA = 1\n", encoding="utf-8")
    (source / "nested" / "alpha.py").write_text("ALPHA = 2\n", encoding="utf-8")
    (source / "ignored.txt").write_text("ignored", encoding="utf-8")
    return source


def _write_json(path: Path, content: object) -> None:
    """Serialize synthetic public JSON evidence."""
    path.write_text(json.dumps(content), encoding="utf-8")


def _write_manifest(
    path: Path,
    source: Path,
    entries: Sequence[Mapping[str, object]],
) -> None:
    """Write a source-bound reviewed-equivalent manifest."""
    _write_json(
        path,
        {
            "schema_version": 1,
            "source_sha256": gate._source_sha256(source),  # ruff: ignore[private-member-access]
            "equivalents": entries,
        },
    )


class SourceDigestTests(unittest.TestCase):
    """Exercise deterministic source binding and unsafe-tree rejection."""

    def test_digest_is_bounded_deterministic_and_ignores_other_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = _make_source(Path(directory))
            first = gate._source_sha256(source)  # ruff: ignore[private-member-access]
            (source / "ignored.txt").write_text("changed", encoding="utf-8")
            second = gate._source_sha256(source)  # ruff: ignore[private-member-access]
            (source / "zeta.py").write_text("ZETA = 3\n", encoding="utf-8")
            third = gate._source_sha256(source)  # ruff: ignore[private-member-access]
        self.assertRegex(first, r"^[0-9a-f]{8}(?::[0-9a-f]{8}){7}$")
        self.assertEqual(first, second)
        self.assertNotEqual(first, third)

    def test_digest_rejects_empty_symbolic_and_unreadable_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            empty_roots: tuple[Path, ...] = ()
            with self.assertRaisesRegex(
                gate.MutationResultsError,
                "roots must not be empty",
            ):
                gate._source_sha256(empty_roots)  # ruff: ignore[private-member-access]
            non_directory = base / "not-a-directory"
            non_directory.write_text("PUBLIC\n", encoding="utf-8")
            with self.assertRaisesRegex(
                gate.MutationResultsError,
                "root must be a real directory",
            ):
                gate._source_sha256(non_directory)  # ruff: ignore[private-member-access]
            source = base / "source"
            source.mkdir()
            with self.assertRaisesRegex(gate.MutationResultsError, "no Python files"):
                gate._source_sha256(source)  # ruff: ignore[private-member-access]
            target = source / "target.txt"
            target.write_text("public", encoding="utf-8")
            symbolic = source / "symbolic.py"
            symbolic.symlink_to(target)
            with self.assertRaisesRegex(
                gate.MutationResultsError,
                "regular non-symbolic file",
            ):
                gate._source_sha256(source)  # ruff: ignore[private-member-access]
            symbolic.unlink()
            (source / "public.py").write_text("PUBLIC = 1\n", encoding="utf-8")
            with (
                patch.object(Path, "read_bytes", side_effect=OSError("denied")),
                self.assertRaisesRegex(
                    gate.MutationResultsError,
                    "cannot read production source",
                ),
            ):
                gate._source_sha256(source)  # ruff: ignore[private-member-access]


class EquivalentManifestTests(unittest.TestCase):
    """Exercise source binding, entry schema, uniqueness, and ordering."""

    def test_manifest_accepts_empty_and_sorted_reviewed_entries(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = _make_source(base)
            manifest = base / "manifest.json"
            _write_manifest(manifest, source, [])
            self.assertEqual(gate._load_equivalents(manifest, source), {})  # ruff: ignore[private-member-access]
            entries: list[dict[str, object]] = [
                {"mutant": MUTANTS[0], "rationale": "Same public behavior."},
                {"mutant": MUTANTS[1], "rationale": "Alias is identical."},
            ]
            _write_manifest(manifest, source, entries)
            self.assertEqual(
                gate._load_equivalents(manifest, source),  # ruff: ignore[private-member-access]
                {str(entry["mutant"]): str(entry["rationale"]) for entry in entries},
            )

    def test_manifest_rejects_schema_digest_and_array_errors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = _make_source(base)
            manifest = base / "manifest.json"
            cases = (
                ({"schema_version": 1}, "schema mismatch"),
                (
                    {
                        "schema_version": True,
                        "source_sha256": ":".join(("00000000",) * 8),
                        "equivalents": [],
                    },
                    "integer 1",
                ),
                (
                    {
                        "schema_version": 1,
                        "source_sha256": "invalid",
                        "equivalents": [],
                    },
                    "colon-delimited lowercase hexadecimal words",
                ),
                (
                    {
                        "schema_version": 1,
                        "source_sha256": ":".join(("00000000",) * 8),
                        "equivalents": [],
                    },
                    "does not match production source",
                ),
                (
                    {
                        "schema_version": 1,
                        "source_sha256": gate._source_sha256(source),  # ruff: ignore[private-member-access]
                        "equivalents": {},
                    },
                    "must be a JSON array",
                ),
            )
            for content, message in cases:
                with self.subTest(message=message):
                    self._assert_invalid_manifest(manifest, source, content, message)

    def _assert_invalid_manifest(
        self,
        manifest: Path,
        source: Path,
        content: object,
        message: str,
    ) -> None:
        _write_json(manifest, content)
        with self.assertRaisesRegex(gate.MutationResultsError, message):
            gate._load_equivalents(manifest, source)  # ruff: ignore[private-member-access]

    def test_entries_reject_invalid_shapes_values_duplicates_and_order(self) -> None:
        invalid_entries: tuple[tuple[object, str], ...] = (
            ([], "must be a JSON object"),
            ({1: "value"}, "must be a JSON object"),
            ({"mutant": MUTANTS[0]}, "schema mismatch"),
            ({"mutant": True, "rationale": "Reviewed."}, "invalid mutant ID"),
            ({"mutant": "invalid ID", "rationale": "Reviewed."}, "invalid mutant ID"),
            ({"mutant": MUTANTS[0], "rationale": True}, "nonempty rationale"),
            ({"mutant": MUTANTS[0], "rationale": ""}, "nonempty rationale"),
            ({"mutant": MUTANTS[0], "rationale": " padded "}, "must be normalized"),
        )
        for entry, message in invalid_entries:
            with self.subTest(message=message):
                with self.assertRaisesRegex(gate.MutationResultsError, message):
                    gate._validate_equivalent_entry(entry, 0)  # ruff: ignore[private-member-access]
        entry = {"mutant": MUTANTS[0], "rationale": "Reviewed."}
        cases = (
            ([entry, entry], "duplicate equivalent-mutant ID"),
            (
                [
                    {"mutant": MUTANTS[1], "rationale": "Reviewed."},
                    {"mutant": MUTANTS[0], "rationale": "Reviewed."},
                ],
                "lexical mutant-ID order",
            ),
        )
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            source = _make_source(base)
            manifest = base / "manifest.json"
            for entries, message in cases:
                with self.subTest(message=message):
                    self._assert_invalid_entries(manifest, source, entries, message)

    def _assert_invalid_entries(
        self,
        manifest: Path,
        source: Path,
        entries: Sequence[Mapping[str, object]],
        message: str,
    ) -> None:
        _write_manifest(manifest, source, entries)
        with self.assertRaisesRegex(gate.MutationResultsError, message):
            gate._load_equivalents(manifest, source)  # ruff: ignore[private-member-access]


if __name__ == "__main__":
    unittest.main(verbosity=2)
