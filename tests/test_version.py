"""Verify version discovery derives exclusively from package metadata."""

from __future__ import annotations

import unittest
from importlib.metadata import PackageNotFoundError
from unittest.mock import patch

import eml_attachment_remover._version as version_module
from eml_attachment_remover import __version__


class VersionTests(unittest.TestCase):
    """Cover installed-metadata and source-tree version discovery."""

    def test_runtime_version_matches_project_metadata(self) -> None:
        with patch.object(
            version_module,
            "distribution_version",
            return_value=__version__,
        ) as lookup:
            self.assertEqual(version_module._resolved_version(), __version__)
        lookup.assert_called_once_with(version_module.DISTRIBUTION_NAME)

    def test_source_tree_fallback_reads_pyproject_version(self) -> None:
        with patch(
            "eml_attachment_remover._version.distribution_version",
            side_effect=PackageNotFoundError,
        ):
            self.assertEqual(
                version_module._resolved_version(),
                version_module._project_version(),
            )
