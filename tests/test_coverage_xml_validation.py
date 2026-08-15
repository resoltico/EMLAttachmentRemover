"""Fail-closed schema and consistency contracts for Coverage.py XML."""

from __future__ import annotations

import tomllib
import unittest
from pathlib import Path

import coverage
from tools import coverage_xml_safety, coverage_xml_validation

from tests.coverage_xml_samples import VALID_COVERAGE_XML, altered

PROJECT_CONFIG = Path(__file__).resolve().parents[1] / "pyproject.toml"


def _replace_element(tag: bytes, replacement: bytes) -> bytes:
    """Replace one complete uniquely named sample element.

    Returns:
        XML with the selected complete element replaced.

    """
    openings = (b"<" + tag + b">", b"<" + tag + b" ")
    closing = b"</" + tag + b">"
    start = min(
        position
        for opening in openings
        if (position := VALID_COVERAGE_XML.find(opening)) >= 0
    )
    end = VALID_COVERAGE_XML.index(closing, start) + len(closing)
    return VALID_COVERAGE_XML[:start] + replacement + VALID_COVERAGE_XML[end:]


def _duplicate_element(tag: bytes) -> bytes:
    """Duplicate one complete uniquely named sample element.

    Returns:
        XML containing two consecutive copies of the selected element.

    """
    openings = (b"<" + tag + b">", b"<" + tag + b" ")
    closing = b"</" + tag + b">"
    start = min(
        position
        for opening in openings
        if (position := VALID_COVERAGE_XML.find(opening)) >= 0
    )
    end = VALID_COVERAGE_XML.index(closing, start) + len(closing)
    fragment = VALID_COVERAGE_XML[start:end]
    return VALID_COVERAGE_XML[:end] + fragment + VALID_COVERAGE_XML[end:]


class CoverageXmlValidationTests(unittest.TestCase):
    """Validate only Coverage.py's exact internally consistent XML dialect."""

    def test_realistic_coverage_xml_is_accepted(self) -> None:
        coverage_xml_validation.validate(VALID_COVERAGE_XML)
        self.assertIn(b"<coverage ", VALID_COVERAGE_XML)

    def test_expected_sources_and_runtime_version_match_pyproject(self) -> None:
        with PROJECT_CONFIG.open("rb") as project_file:
            configuration = tomllib.load(project_file)
        configured_sources = tuple(configuration["tool"]["coverage"]["run"]["source"])
        development = tuple(configuration["dependency-groups"]["dev"])

        self.assertEqual(
            configured_sources,
            coverage_xml_validation.EXPECTED_SOURCES,
        )
        self.assertIn(f"coverage=={coverage.__version__}", development)

    def test_root_name_attributes_layout_and_metadata_are_exact(self) -> None:
        cases = (
            VALID_COVERAGE_XML.replace(b"<coverage ", b"<public ", 1).replace(
                b"</coverage>", b"</public>"
            ),
            altered(b' version="7.15.4"', b""),
            altered(b'<coverage version="7.15.4"', b'<coverage private="1"'),
            altered(b'lines-valid="2"', b'lines-valid="01"'),
            altered(b'lines-covered="1"', b'lines-covered="3"'),
            altered(b'branches-valid="2"', b'branches-valid="01"'),
            altered(b'branches-covered="1"', b'branches-covered="3"'),
            altered(
                b'lines-covered="1" line-rate="0.5"',
                b'lines-covered="1" line-rate="public"',
            ),
            altered(
                b'lines-covered="1" line-rate="0.5"',
                b'lines-covered="1" line-rate="NaN"',
            ),
            altered(
                b'branches-covered="1" branch-rate="0.5" complexity="0">',
                b'branches-covered="1" branch-rate="2" complexity="0">',
            ),
            altered(b'timestamp="1786680000000"', b'timestamp="private"'),
            altered(b'version="7.15.4"', b'version="public"'),
            altered(b'version="7.15.4"', b'version="7.15.3"'),
            altered(
                b'branches-covered="1" branch-rate="0.5" complexity="0">',
                b'branches-covered="1" branch-rate="0.5" complexity="1">',
            ),
            altered(
                b'branches-covered="1" branch-rate="0.5" complexity="0">',
                b'branches-covered="1" branch-rate="0.5" complexity="0">private',
            ),
            altered(b"  <sources>", b"  <private/>\n  <sources>"),
            altered(b"</coverage>", b"<private/></coverage>"),
        )
        for content in cases:
            with (
                self.subTest(content=content[:100]),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                coverage_xml_validation.validate(content)

    def test_source_container_and_relative_roots_are_exact(self) -> None:
        cases = (
            altered(b"<sources>", b'<sources private="1">'),
            altered(b"<sources>\n", b"<sources>private\n"),
            altered(b"</sources>\n  <packages>", b"</sources>private<packages>"),
            _replace_element(b"sources", b"<sources/>"),
            _replace_element(b"sources", b"<sources><private/></sources>"),
            altered(b"<source>tools</source>", b"<source/>"),
            altered(
                b"<source>tools</source>",
                b'<source private="1">tools</source>',
            ),
            altered(
                b"<source>tools</source>",
                b"<source><private/></source>",
            ),
            altered(
                b"<source>tools</source>\n",
                b"<source>tools</source>private\n",
            ),
            altered(
                b"<source>tools</source>",
                b"<source>tools/../private</source>",
            ),
            altered(
                b"    <source>src/eml_attachment_remover</source>\n",
                b"",
            ),
            altered(
                b"<source>tools</source>",
                b"<source>public</source>",
            ),
            altered(
                b"<source>tools</source>",
                b"<source>tools</source><source>tools</source>",
            ),
        )
        for content in cases:
            with (
                self.subTest(content=content[:120]),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                coverage_xml_validation.validate(content)

    def test_package_and_classes_containers_are_exact(self) -> None:
        cases = (
            altered(b"<packages>", b'<packages private="1">'),
            altered(b"<packages>\n", b"<packages>private\n"),
            altered(b"</packages>\n", b"</packages>private\n"),
            _replace_element(b"packages", b"<packages/>"),
            _replace_element(b"packages", b"<packages><private/></packages>"),
            altered(b'<package name="."', b'<package private="1" name="."'),
            altered(
                b'<package name="." line-rate="0.5" branch-rate="0.5" complexity="0">',
                b'<package name="." line-rate="0.5" '
                b'branch-rate="0.5" complexity="0">private',
            ),
            altered(b"</package>\n", b"</package>private\n"),
            _replace_element(b"package", b'<package name="."/>'),
            altered(b"<classes>", b'<classes private="1">'),
            altered(b"<classes>\n", b"<classes>private\n"),
            altered(b"</classes>\n", b"</classes>private\n"),
            _replace_element(b"classes", b"<classes/>"),
            _replace_element(b"classes", b"<classes><private/></classes>"),
            _duplicate_element(b"package"),
            _duplicate_element(b"class"),
        )
        for content in cases:
            with (
                self.subTest(content=content[:120]),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                coverage_xml_validation.validate(content)

    def test_class_structure_identity_and_rates_are_exact(self) -> None:
        cases = (
            altered(b'<class name="public.py"', b'<class private="1"'),
            altered(b"<methods/>", b'<methods private="1"/>'),
            altered(b"<methods/>", b"<methods><private/></methods>"),
            altered(b"<methods/>", b"<methods>private</methods>"),
            altered(b"<methods/>\n", b"<methods/>private\n"),
            altered(b"<lines>\n", b'<lines private="1">\n'),
            altered(b"<lines>\n", b"<lines>private\n"),
            altered(b"</lines>\n", b"</lines>private\n"),
            altered(b"<lines>\n", b"<lines>\n<private/>"),
            altered(b"<lines>\n", b'<lines>\n<line number="2" hits="0"/>'),
            altered(b'<class name="public.py"', b'<class name="   "'),
            altered(b'filename="public.py"', b'filename="public.txt"'),
            altered(
                b'complexity="0"\n         line-rate="0.5"',
                b'complexity="1"\n         line-rate="0.5"',
            ),
            altered(
                b'line-rate="0.5" branch-rate="0.5">\n          <methods',
                b'line-rate="0.4" branch-rate="0.5">\n          <methods',
            ),
            altered(
                b'line-rate="0.5" branch-rate="0.5">\n          <methods',
                b'line-rate="0.5" branch-rate="0.4">\n          <methods',
            ),
        )
        for content in cases:
            with (
                self.subTest(content=content[:140]),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                coverage_xml_validation.validate(content)

    def test_package_and_root_totals_and_rates_must_reconcile(self) -> None:
        cases = (
            altered(
                b'<package name="." line-rate="0.5"',
                b'<package name="." line-rate="2"',
            ),
            altered(
                b'<package name="." line-rate="0.5" branch-rate="0.5"',
                b'<package name="." line-rate="0.5" branch-rate="NaN"',
            ),
            altered(b'<package name="."', b'<package name="   "'),
            altered(b'<package name="."', b'<package name="private/name"'),
            altered(
                b'<package name="." line-rate="0.5" branch-rate="0.5" complexity="0">',
                b'<package name="." line-rate="0.5" branch-rate="0.5" complexity="1">',
            ),
            altered(b'lines-valid="2"', b'lines-valid="1"'),
            altered(b'lines-valid="2"', b'lines-valid="3"'),
            altered(b'branches-valid="2"', b'branches-valid="3"'),
            altered(
                b'lines-covered="1" line-rate="0.5"',
                b'lines-covered="1" line-rate="0.50"',
            ),
            altered(
                b'branches-covered="1" branch-rate="0.5" complexity="0">',
                b'branches-covered="1" branch-rate="0.50" complexity="0">',
            ),
        )
        for content in cases:
            with (
                self.subTest(content=content[:140]),
                self.assertRaises(coverage_xml_safety.CoverageXmlError),
            ):
                coverage_xml_validation.validate(content)

    def test_hidden_same_name_source_totals_allow_package_rate_divergence(
        self,
    ) -> None:
        content = altered(
            b'lines-valid="2" lines-covered="1" line-rate="0.5"',
            b'lines-valid="3" lines-covered="2" line-rate="0.6667"',
        )
        content = altered(
            b'branches-valid="2" branches-covered="1" branch-rate="0.5"',
            b'branches-valid="4" branches-covered="3" branch-rate="0.75"',
            content,
        )
        content = altered(
            b'<package name="." line-rate="0.5" branch-rate="0.5"',
            b'<package name="." line-rate="0.6667" branch-rate="0.75"',
            content,
        )

        coverage_xml_validation.validate(content)
        self.assertIn(b'line-rate="0.6667"', content)


if __name__ == "__main__":
    unittest.main(verbosity=2)
