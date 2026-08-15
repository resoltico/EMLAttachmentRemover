"""Strict runtime contracts for release-critical project metadata."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from tools.distribution_archive_contract import (
    ArchiveContract,
    DistributionArchiveError,
)

from tests.distribution_archive_support import create_distribution


def _replace(config: Path, old: str, new: str) -> None:
    """Replace one exact synthetic project fragment."""
    content = config.read_text(encoding="utf-8")
    assert old in content
    config.write_text(content.replace(old, new), encoding="utf-8")


def _load(root: Path) -> ArchiveContract:
    """Load the edited synthetic project contract.

    Returns:
        The validated release archive contract.

    """
    public_files = (
        root / "LICENSE",
        root / "README.md",
        root / "pyproject.toml",
        root / "src/public_package/__init__.py",
    )
    return ArchiveContract.from_project(root, root / "pyproject.toml", public_files)


class DistributionProjectTableTests(unittest.TestCase):
    """Reject every absent required TOML table at its exact boundary."""

    def test_every_required_nested_table_is_fail_closed(self) -> None:
        cases: tuple[tuple[str, tuple[tuple[str, str], ...]], ...] = (
            (
                "project",
                (
                    ("[project]", "[public-project]"),
                    ("[project.urls]", "[public-project.urls]"),
                ),
            ),
            ("build-system", (("[build-system]", "[public-build-system]"),)),
            (
                "tool",
                (
                    (
                        "[tool.eml-attachment-remover.runtime]",
                        "[public.remover.runtime]",
                    ),
                    (
                        "[tool.hatch.build.targets.wheel]",
                        "[public.hatch.build.targets.wheel]",
                    ),
                ),
            ),
            (
                "eml-attachment-remover",
                (
                    (
                        "[tool.eml-attachment-remover.runtime]",
                        "[tool.public-remover.runtime]",
                    ),
                ),
            ),
            (
                "runtime",
                (
                    (
                        "[tool.eml-attachment-remover.runtime]",
                        "[tool.eml-attachment-remover.public-runtime]",
                    ),
                ),
            ),
            (
                "hatch",
                (
                    (
                        "[tool.hatch.build.targets.wheel]",
                        "[tool.public-hatch.build.targets.wheel]",
                    ),
                ),
            ),
            (
                "build",
                (
                    (
                        "[tool.hatch.build.targets.wheel]",
                        "[tool.hatch.public-build.targets.wheel]",
                    ),
                ),
            ),
            (
                "targets",
                (
                    (
                        "[tool.hatch.build.targets.wheel]",
                        "[tool.hatch.build.public-targets.wheel]",
                    ),
                ),
            ),
            (
                "wheel",
                (
                    (
                        "[tool.hatch.build.targets.wheel]",
                        "[tool.hatch.build.targets.public-wheel]",
                    ),
                ),
            ),
            ("urls", (("[project.urls]", "[project.public-urls]"),)),
        )
        for field, replacements in cases:
            with (
                self.subTest(field=field),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                distribution = create_distribution(root, scripts=False)
                for old, new in replacements:
                    _replace(distribution.config, old, new)
                with self.assertRaises(DistributionArchiveError) as raised:
                    _load(root)
                self.assertEqual(
                    str(raised.exception),
                    f"project configuration field {field!r} must be a table",
                )


class DistributionProjectValueTests(unittest.TestCase):
    """Reject malformed scalar and collection metadata without type casts."""

    def test_every_required_text_field_is_runtime_validated(self) -> None:
        cases = (
            ('name = "public-project"', "name = 7", "name"),
            ('version = "1.2.3"', "version = 7", "version"),
            (
                'description = "Public synthetic distribution"',
                "description = 7",
                "description",
            ),
            (
                'requires-python = ">=3.14,<3.15"',
                "requires-python = 7",
                "requires-python",
            ),
            ('license = "MIT"', "license = 7", "license"),
            ('build-backend = "hatchling.build"', "build-backend = 7", "build-backend"),
            ('implementation = "CPython"', "implementation = 7", "implementation"),
        )
        for old, replacement, field in cases:
            with (
                self.subTest(field=field),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace(distribution.config, old, replacement)
                with self.assertRaises(DistributionArchiveError) as raised:
                    _load(distribution.root)
                self.assertEqual(
                    str(raised.exception),
                    f"project configuration field {field!r} must be text",
                )

    def test_every_string_collection_is_runtime_validated(self) -> None:
        cases = (
            (
                'requires = ["hatchling==1.32.0"]',
                'requires = "bad"',
                "configuration field 'requires'",
            ),
            (
                'requires = ["hatchling==1.32.0"]',
                "requires = [7]",
                "configuration field 'requires'",
            ),
            (
                'packages = ["src/public_package"]',
                'packages = "bad"',
                "configuration field 'packages'",
            ),
            (
                'packages = ["src/public_package"]',
                "packages = [7]",
                "configuration field 'packages'",
            ),
            ('license-files = ["LICENSE"]', 'license-files = "bad"', "license-files"),
            ('license-files = ["LICENSE"]', "license-files = [7]", "license-files"),
            ('keywords = ["synthetic", "public"]', 'keywords = "bad"', "keywords"),
            (
                'keywords = ["synthetic", "public"]',
                'keywords = ["synthetic", 7]',
                "keywords",
            ),
            (
                'classifiers = ["Topic :: Utilities"]',
                'classifiers = "bad"',
                "classifiers",
            ),
            ("dependencies = []", 'dependencies = "bad"', "dependencies"),
        )
        for old, replacement, label in cases:
            with (
                self.subTest(label=label),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace(distribution.config, old, replacement)
                with self.assertRaises(DistributionArchiveError) as raised:
                    _load(distribution.root)
                self.assertEqual(
                    str(raised.exception),
                    f"project {label} must be a string list",
                )

    def test_script_table_requires_text_targets(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                'public-command = "public_package:main"',
                "public-command = 7",
            )
            with self.assertRaises(DistributionArchiveError) as raised:
                _load(distribution.root)
        self.assertEqual(
            str(raised.exception),
            "project scripts must be a text-to-text table",
        )

    def test_script_field_requires_a_table(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                '[project.scripts]\npublic-command = "public_package:main"\n',
                'scripts = "bad"\n',
            )
            with self.assertRaises(DistributionArchiveError) as raised:
                _load(distribution.root)
        self.assertEqual(
            str(raised.exception),
            "project scripts must be a text-to-text table",
        )

    def test_url_table_requires_public_text_destinations(self) -> None:
        cases = (
            (
                'Homepage = "https://example.test/public-project"',
                "Homepage = 7",
                "project URLs must be a text-to-text table",
            ),
            (
                (
                    'Homepage = "https://example.test/public-project"\n'
                    'Repository = "https://example.test/public-project.git"\n'
                ),
                "",
                "project URLs must contain at least one public destination",
            ),
        )
        for old, replacement, expected in cases:
            with (
                self.subTest(expected=expected),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace(distribution.config, old, replacement)
                with self.assertRaises(DistributionArchiveError) as raised:
                    _load(distribution.root)
                self.assertEqual(str(raised.exception), expected)

    def test_optional_author_and_license_fields_may_be_absent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                'license-files = ["LICENSE"]\n',
                "",
            )
            _replace(
                distribution.config,
                'authors = [{name = "Public Example"}]\n',
                "",
            )
            loaded = _load(distribution.root)
            wheel_sources = loaded.wheel_sources()
        self.assertEqual(loaded.license_files, ())
        self.assertEqual(loaded.authors, ())
        self.assertEqual(set(wheel_sources), {"public_package/__init__.py"})

    def test_nameless_author_does_not_hide_later_named_authors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                'authors = [{name = "Public Example"}]',
                'authors = [{email = "first@example.test"}, {name = "Second"}]',
            )
            loaded = _load(distribution.root)
        self.assertEqual(loaded.authors, ("Second",))

    def test_pinned_hatchling_version_accepts_the_declared_ascii_grammar(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                "hatchling==1.32.0",
                "hatchling==A1",
            )
            loaded = _load(distribution.root)
        self.assertEqual(loaded.wheel_generator, "hatchling A1")

    def test_project_paths_are_validated_before_normalization(self) -> None:
        cases = (
            (
                'packages = ["src/public_package"]',
                'packages = ["src//public_package"]',
                "unsafe or ambiguous archive member name: 'src//public_package'",
            ),
            (
                'license-files = ["LICENSE"]',
                'license-files = ["./LICENSE"]',
                "unsafe or ambiguous archive member name: './LICENSE'",
            ),
            (
                'readme = "README.md"',
                'readme = "README.md/"',
                "unsafe or ambiguous archive member name: 'README.md/'",
            ),
        )
        for old, replacement, expected in cases:
            with (
                self.subTest(replacement=replacement),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace(distribution.config, old, replacement)
                with self.assertRaises(DistributionArchiveError) as raised:
                    _load(distribution.root)
                self.assertEqual(str(raised.exception), expected)

    def test_project_requires_at_least_one_package_root(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace(
                distribution.config,
                'packages = ["src/public_package"]',
                "packages = []",
            )
            with self.assertRaises(DistributionArchiveError) as raised:
                _load(distribution.root)
        self.assertEqual(
            str(raised.exception),
            "project packages must contain at least one public package root",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
