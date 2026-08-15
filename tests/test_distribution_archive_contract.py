"""Behavioral tests for canonical distribution-archive contracts."""

from __future__ import annotations

import io
import runpy
import sys
import tempfile
import unittest
import zipfile
from dataclasses import replace
from email import policy
from email.parser import BytesParser
from pathlib import Path, PurePosixPath
from unittest.mock import patch

from tools import archive_reproducibility_policy
from tools import distribution_archive_contract as contract
from tools import distribution_project_metadata as project_metadata

from tests.distribution_archive_support import create_distribution


def _replace_config(config: Path, old: str, new: str) -> None:
    """Replace one exact synthetic project setting."""
    text = config.read_text(encoding="utf-8")
    assert old in text
    config.write_text(text.replace(old, new), encoding="utf-8")


class ArchiveContractTests(unittest.TestCase):
    """Exercise metadata, source mapping, and exact surface derivation."""

    def test_unix_unspecified_directory_type_is_a_regular_wheel_member(self) -> None:
        member = zipfile.ZipInfo("public/")
        member.external_attr = 0
        self.assertTrue(archive_reproducibility_policy.regular_wheel_member(member))

    def test_author_license_and_nested_build_tables_are_strictly_typed(self) -> None:
        table_cases: tuple[tuple[str, str, str], ...] = (
            (
                'authors = [{name = "Public Example"}]',
                'authors = "Public"',
                "project authors must be a list",
            ),
            (
                'authors = [{name = "Public Example"}]',
                'authors = ["Public Author"]',
                "each project author must be a table",
            ),
            (
                'authors = [{name = "Public Example"}]',
                "authors = [{name = 7}]",
                "each project author name must be text",
            ),
            (
                'license-files = ["LICENSE"]',
                'license-files = "LICENSE"',
                "project license-files must be a string list",
            ),
            (
                '[build-system]\nrequires = ["hatchling==1.32.0"]',
                '[build-system]\nrequires = "hatchling==1.32.0"',
                "project configuration field 'requires' must be a string list",
            ),
            (
                'build-backend = "hatchling.build"',
                "build-backend = 7",
                "project configuration field 'build-backend' must be text",
            ),
            (
                'implementation = "CPython"',
                "implementation = 7",
                "project configuration field 'implementation' must be text",
            ),
        )
        for old, replacement, message in table_cases:
            with (
                self.subTest(replacement=replacement),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace_config(distribution.config, old, replacement)
                with self.assertRaises(
                    contract.DistributionArchiveError,
                ) as raised:
                    contract.ArchiveContract.from_project(
                        distribution.root,
                        distribution.config,
                        distribution.public_files,
                    )
                self.assertEqual(str(raised.exception), message)

    def test_author_without_a_name_is_allowed_but_not_exported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            _replace_config(
                distribution.config,
                'authors = [{name = "Public Example"}]',
                'authors = [{email = "public@example.test"}]',
            )
            loaded = contract.ArchiveContract.from_project(
                distribution.root,
                distribution.config,
                distribution.public_files,
            )
        self.assertEqual(loaded.authors, ())

    def test_build_generator_and_requirement_normalization_are_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            loaded = contract.ArchiveContract.from_project(
                distribution.root,
                distribution.config,
                distribution.public_files,
            )
        self.assertEqual(loaded.wheel_generator, "hatchling 1.32.0")
        self.assertEqual(
            project_metadata.normalized_requirement("<3.15, >=3.14"),
            "<3.15,>=3.14",
        )

    def test_contract_derives_every_canonical_release_value(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            declared = distribution.contract
            self.assertEqual(declared.source_root, "public_project-1.2.3")
            self.assertEqual(declared.dist_info, "public_project-1.2.3.dist-info")
            self.assertEqual(
                set(declared.wheel_sources()),
                {
                    "public_package/__init__.py",
                    "public_project-1.2.3.dist-info/licenses/LICENSE",
                },
            )
            self.assertEqual(
                declared.wheel_generated_files(),
                frozenset({
                    "public_project-1.2.3.dist-info/METADATA",
                    "public_project-1.2.3.dist-info/RECORD",
                    "public_project-1.2.3.dist-info/WHEEL",
                    "public_project-1.2.3.dist-info/entry_points.txt",
                }),
            )
            self.assertEqual(
                declared.expected_entry_points(),
                "[console_scripts]\npublic-command = public_package:main\n",
            )
            self.assertEqual(
                declared.expected_metadata()["Project-URL"],
                (
                    "Homepage, https://example.test/public-project",
                    "Repository, https://example.test/public-project.git",
                ),
            )
            readme = distribution.root / declared.readme
            readme.write_bytes(b"Public synthetic README\r\n")
            metadata = BytesParser(policy=policy.compat32).parsebytes(
                distribution.metadata,
            )
            declared.verify_metadata(metadata)

    def test_contract_without_scripts_omits_entry_points(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            declared = create_distribution(Path(directory), scripts=False).contract
        self.assertFalse(
            any(
                name.endswith("entry_points.txt")
                for name in declared.wheel_generated_files()
            ),
        )
        self.assertEqual(declared.expected_entry_points(), "[console_scripts]\n")

    def test_contract_rejects_missing_package_and_license_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            declared = distribution.contract
            missing_package = replace(
                declared,
                public_sources={"LICENSE": distribution.root / "LICENSE"},
            )
            with self.assertRaisesRegex(
                contract.DistributionArchiveError, "no audited"
            ):
                missing_package.wheel_sources()
            missing_license = replace(
                declared,
                public_sources={
                    "src/public_package/__init__.py": distribution.root
                    / "src/public_package/__init__.py",
                },
            )
            with self.assertRaisesRegex(contract.DistributionArchiveError, "license"):
                missing_license.wheel_sources()

    def test_contract_rejects_overlapping_package_destinations(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            distribution = create_distribution(root)
            other = root / "other" / "public_package" / "__init__.py"
            other.parent.mkdir(parents=True)
            other.write_text("PUBLIC = True\n", encoding="utf-8")
            declared = distribution.contract
            overlapping = replace(
                declared,
                package_roots=(
                    PurePosixPath("src/public_package"),
                    PurePosixPath("other/public_package"),
                ),
                public_sources={
                    **declared.public_sources,
                    "other/public_package/__init__.py": other,
                },
            )
            with self.assertRaisesRegex(contract.DistributionArchiveError, "overlap"):
                overlapping.wheel_sources()

    def test_metadata_mismatch_is_field_specific(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            bad = distribution.metadata.replace(
                b"Summary: Public synthetic distribution",
                b"Summary: Changed",
            )
            metadata = BytesParser(policy=policy.compat32).parsebytes(bad)
            with self.assertRaisesRegex(
                contract.DistributionArchiveError,
                "Summary mismatch",
            ):
                distribution.contract.verify_metadata(metadata)

    def test_metadata_rejects_undeclared_dependencies_and_readme_drift(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            distribution = create_distribution(Path(directory))
            unexpected_dependency = distribution.metadata.replace(
                b"Requires-Python:",
                b"Requires-Dist: private-package\nRequires-Python:",
            )
            changed_readme = distribution.metadata.replace(
                b"Public synthetic README",
                b"Changed public README",
            )
            for metadata_bytes, expected in (
                (unexpected_dependency, "Requires-Dist mismatch"),
                (changed_readme, "description does not match"),
            ):
                with (
                    self.subTest(expected=expected),
                    self.assertRaisesRegex(
                        contract.DistributionArchiveError,
                        expected,
                    ),
                ):
                    metadata = BytesParser(policy=policy.compat32).parsebytes(
                        metadata_bytes
                    )
                    distribution.contract.verify_metadata(metadata)

    def test_contract_rejects_ambiguous_or_unaudited_readme_metadata(self) -> None:
        cases = (
            (
                'readme = {file = "README.md"}',
                "project readme must be one static public path",
            ),
            (
                'readme = "LICENSE"',
                "project readme must be an audited Markdown source",
            ),
            (
                'readme = "missing.md"',
                "project readme must be an audited Markdown source",
            ),
        )
        for replacement, expected in cases:
            with (
                self.subTest(replacement=replacement),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace_config(
                    distribution.config,
                    'readme = "README.md"',
                    replacement,
                )
                with self.assertRaises(
                    contract.DistributionArchiveError,
                ) as raised:
                    contract.ArchiveContract.from_project(
                        distribution.root,
                        distribution.config,
                        distribution.public_files,
                    )
                self.assertEqual(str(raised.exception), expected)

    def test_contract_rejects_ambiguous_backend_and_runtime_metadata(self) -> None:
        cases = (
            (
                'build-backend = "hatchling.build"',
                'build-backend = "public.backend"',
                "release backend must be one exactly pinned Hatchling requirement",
            ),
            (
                'requires = ["hatchling==1.32.0"]',
                'requires = ["public-backend==1.0"]',
                "release backend must be one exactly pinned Hatchling requirement",
            ),
            (
                'requires = ["hatchling==1.32.0"]',
                'requires = ["hatchling>=1.32.0"]',
                "release backend must be one exactly pinned Hatchling requirement",
            ),
            (
                'requires = ["hatchling==1.32.0"]',
                'requires = ["hatchling==1.32.0==unexpected"]',
                "release backend must be one exactly pinned Hatchling requirement",
            ),
            (
                'requires = ["hatchling==1.32.0"]',
                'requires = ["hatchling==1.32.0", "hatchling==1.31.0"]',
                "release backend must be one exactly pinned Hatchling requirement",
            ),
            (
                'requires-python = ">=3.14,<3.15"',
                'requires-python = ">=3.14"',
                "release wheel requires exact CPython major/minor bounds",
            ),
            (
                'implementation = "CPython"',
                'implementation = "PyPy"',
                "release wheel requires exact CPython major/minor bounds",
            ),
            (
                'requires-python = ">=3.14,<3.15"',
                'requires-python = ">=3.14,<3.16"',
                "release wheel requires one supported CPython minor version",
            ),
        )
        for old, replacement, expected in cases:
            with (
                self.subTest(replacement=replacement),
                tempfile.TemporaryDirectory() as directory,
            ):
                distribution = create_distribution(Path(directory))
                _replace_config(distribution.config, old, replacement)
                with self.assertRaises(
                    contract.DistributionArchiveError,
                ) as raised:
                    contract.ArchiveContract.from_project(
                        distribution.root,
                        distribution.config,
                        distribution.public_files,
                    )
                self.assertEqual(str(raised.exception), expected)

    def test_contract_module_supports_direct_portable_execution(self) -> None:
        module_path = Path(contract.__file__).resolve()
        with patch.object(sys, "path", [str(module_path.parent), *sys.path]):
            namespace = runpy.run_path(
                str(module_path),
                run_name="distribution_archive_contract_direct",
            )
        error_type = namespace["DistributionArchiveError"]
        self.assertEqual(error_type.__module__, "archive_surface_policy")


class ArchivePathTests(unittest.TestCase):
    """Exercise portable member names, directory sets, and byte comparison."""

    def test_safe_parts_accepts_files_and_directory_suffixes(self) -> None:
        self.assertEqual(
            contract.safe_parts("public/file", directory=False), ("public", "file")
        )
        self.assertEqual(
            contract.safe_parts("public/dir/", directory=True), ("public", "dir")
        )

    def test_safe_parts_rejects_every_ambiguous_form(self) -> None:
        unsafe = (
            ("", "unsafe or ambiguous"),
            ("/public", "unsafe or ambiguous"),
            ("public\\file", "unsafe or ambiguous"),
            ("public\x00file", "unsafe or ambiguous"),
            (".", "unsafe or ambiguous"),
            ("..", "unsafe or ambiguous"),
            ("a/../b", "unsafe or ambiguous"),
            ("C:public", "unsafe or ambiguous"),
            ("c:public", "unsafe or ambiguous"),
            ("a//b", "unsafe or ambiguous"),
            ("public/CON.txt", "non-portable"),
            ("public/name?.txt", "non-portable"),
            ("public/trailing. ", "non-portable"),
        )
        for name, category in unsafe:
            with (
                self.subTest(name=name),
                self.assertRaises(contract.DistributionArchiveError) as raised,
            ):
                contract.safe_parts(name, directory=False)
            self.assertEqual(
                str(raised.exception),
                f"{category} archive member name: {name!r}",
            )

    def test_directory_derivation_and_exact_set(self) -> None:
        self.assertEqual(
            contract.allowed_directories({"a/b/c.py", "a/d.py"}),
            {"a", "a/b"},
        )
        contract.require_exact_set("public", {"a"}, {"a"})
        with self.assertRaisesRegex(
            contract.DistributionArchiveError,
            r"missing=\['b'\]; unknown=\['a'\]",
        ):
            contract.require_exact_set("public", {"a"}, {"b"})

    def test_stream_comparison_reads_all_chunks_and_detects_differences(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "public.bin"
            source.write_bytes(b"abcdef")
            self.assertTrue(contract.same_content(io.BytesIO(b"abcdef"), source, 2))
            self.assertFalse(contract.same_content(io.BytesIO(b"abcdeg"), source, 2))


if __name__ == "__main__":
    unittest.main(verbosity=2)
