"""Focused command-boundary tests for zipapp output safety."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import build_zipapp


class ZipappOutputSafetyTests(unittest.TestCase):
    """Verify aliases are rejected before archive construction starts."""

    def test_main_rejects_same_checksum_path_before_building(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "public.pyz"
            with (
                patch.object(build_zipapp, "build_zipapp") as build,
                self.assertRaisesRegex(ValueError, "distinct files"),
            ):
                build_zipapp.main([
                    "--target",
                    str(target),
                    "--checksum-file",
                    str(target),
                ])
        build.assert_not_called()

    def test_parser_defaults_to_the_documented_target(self) -> None:
        arguments = build_zipapp._build_parser().parse_args([])  # ruff: ignore[private-member-access]
        self.assertEqual(arguments.target, build_zipapp.DEFAULT_TARGET)
        self.assertIsNone(arguments.checksum_file)
        self.assertFalse(arguments.no_verify)


if __name__ == "__main__":
    unittest.main(verbosity=2)
