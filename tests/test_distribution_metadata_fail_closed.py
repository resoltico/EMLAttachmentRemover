"""Fail-closed contracts for generated distribution metadata."""

from __future__ import annotations

import tempfile
import unittest
from email import policy
from email.parser import BytesParser
from pathlib import Path

from tools.distribution_archive_contract import DistributionArchiveError

from tests.distribution_archive_support import create_distribution


class GeneratedMetadataFailClosedTests(unittest.TestCase):
    """Reject unmodeled or unsupported generated metadata fields."""

    def test_locked_backend_metadata_version_is_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            metadata_bytes = distribution.metadata.replace(
                b"Metadata-Version: 2.5",
                b"Metadata-Version: 2.4",
            )
            metadata = BytesParser(policy=policy.compat32).parsebytes(metadata_bytes)
            with self.assertRaises(DistributionArchiveError) as raised:
                distribution.contract.verify_metadata(metadata)
        self.assertEqual(
            str(raised.exception),
            "generated metadata Metadata-Version mismatch: "
            "expected=('2.5',); actual=('2.4',)",
        )

    def test_undeclared_metadata_field_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            metadata_bytes = distribution.metadata.replace(
                b"Name: public-project",
                b"Name: public-project\nX-Public: undeclared",
            )
            metadata = BytesParser(policy=policy.compat32).parsebytes(metadata_bytes)
            with self.assertRaises(DistributionArchiveError) as raised:
                distribution.contract.verify_metadata(metadata)
        self.assertEqual(
            str(raised.exception),
            "generated metadata contains unsupported fields: ['X-Public']",
        )

    def test_project_urls_reject_wrong_missing_extra_and_duplicate_values(self) -> None:
        expected = (
            "Homepage, https://example.test/public-project",
            "Repository, https://example.test/public-project.git",
        )
        cases = (
            (
                b"Repository, https://example.test/public-project.git",
                b"Repository, https://wrong.example/project",
                (*expected[:1], "Repository, https://wrong.example/project"),
            ),
            (
                b"Project-URL: Repository, https://example.test/public-project.git\n",
                b"",
                expected[:1],
            ),
            (
                b"Description-Content-Type:",
                (
                    b"Project-URL: Docs, https://example.test/docs\n"
                    b"Description-Content-Type:"
                ),
                (*expected, "Docs, https://example.test/docs"),
            ),
            (
                b"Project-URL: Repository,",
                (
                    b"Project-URL: Homepage, https://example.test/public-project\n"
                    b"Project-URL: Repository,"
                ),
                (expected[0], expected[0], expected[1]),
            ),
        )
        for old, replacement, actual in cases:
            with (
                self.subTest(actual=actual),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                metadata_bytes = distribution.metadata.replace(old, replacement)
                metadata = BytesParser(policy=policy.compat32).parsebytes(
                    metadata_bytes
                )
                with self.assertRaises(DistributionArchiveError) as raised:
                    distribution.contract.verify_metadata(metadata)
                self.assertEqual(
                    str(raised.exception),
                    "generated metadata Project-URL mismatch: "
                    f"expected={expected!r}; actual={actual!r}",
                )


if __name__ == "__main__":
    unittest.main(verbosity=2)
