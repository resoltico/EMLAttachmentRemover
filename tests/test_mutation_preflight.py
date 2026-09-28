"""Contracts for the cheap equivalence preflight and the rebind digest."""

from __future__ import annotations

import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from tools import check_mutation_results

ENTRY = {
    "mutant": "tools.example.x_example__mutmut_1",
    "rationale": "Both spellings select the same public branch.",
}


def _manifest(directory: str, source_sha256: str) -> Path:
    path = Path(directory) / "equivalents.json"
    path.write_text(
        json.dumps({
            "schema_version": 1,
            "source_sha256": source_sha256,
            "equivalents": [ENTRY],
        }),
        encoding="utf-8",
    )
    return path


def _main(*arguments: str) -> tuple[int, str, str]:
    output = io.StringIO()
    errors = io.StringIO()
    with redirect_stdout(output), redirect_stderr(errors):
        status = check_mutation_results.main(list(arguments))
    return status, output.getvalue(), errors.getvalue()


class MutationPreflightTests(unittest.TestCase):
    """Validate only the manifest binding, before any campaign work."""

    def test_source_digest_is_the_binding_a_manifest_must_carry(self) -> None:
        status, output, errors = _main("--source-sha256")
        self.assertEqual(status, 0)
        self.assertEqual(errors, "")
        self.assertEqual(
            output,
            check_mutation_results.mutation_manifest.source_sha256(
                check_mutation_results.SOURCE_ROOTS
            )
            + "\n",
        )

    def test_bound_manifest_passes_without_campaign_evidence(self) -> None:
        digest = _main("--source-sha256")[1].strip()
        with tempfile.TemporaryDirectory() as directory:
            manifest = _manifest(directory, digest)
            status, output, errors = _main(
                "--manifest-only",
                "--equivalents",
                str(manifest),
                str(Path(directory) / "absent-statistics.json"),
            )
        self.assertEqual((status, errors), (0, ""))
        self.assertEqual(
            output,
            "equivalent-mutant manifest matches the source: equivalent=1\n",
        )

    def test_stale_manifest_fails_with_the_rebind_diagnostic(self) -> None:
        stale = ":".join(["00000000"] * 8)
        with tempfile.TemporaryDirectory() as directory:
            status, output, errors = _main(
                "--manifest-only",
                "--equivalents",
                str(_manifest(directory, stale)),
            )
        self.assertEqual((status, output), (1, ""))
        self.assertIn("source_sha256 does not match production source", errors)

    def test_modes_are_documented_in_help(self) -> None:
        output = io.StringIO()
        with redirect_stdout(output), self.assertRaises(SystemExit) as raised:
            check_mutation_results.main(["--help"])
        self.assertEqual(raised.exception.code, 0)
        text = " ".join(output.getvalue().split())
        self.assertIn(
            "--manifest-only validate only the manifest and its source binding "
            "(campaign preflight)",
            text,
        )
        self.assertIn(
            "--source-sha256 print the source digest a rebound manifest must carry",
            text,
        )

    def test_modes_are_mutually_exclusive(self) -> None:
        with self.assertRaises(SystemExit) as raised:
            _main("--manifest-only", "--source-sha256")
        self.assertEqual(raised.exception.code, 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
