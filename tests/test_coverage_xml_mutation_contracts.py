"""Exact public diagnostics and independent Coverage XML invariants."""

from __future__ import annotations

import runpy
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

from defusedxml import ElementTree
from tools import coverage_xml_safety, coverage_xml_validation

from tests.coverage_xml_samples import VALID_COVERAGE_XML, altered

PROJECT_ROOT = Path(__file__).resolve().parents[1]
TOOLS_DIRECTORY = PROJECT_ROOT / "tools"


def _replace_element(tag: bytes, replacement: bytes) -> bytes:
    """Replace one complete uniquely named sample element.

    Returns:
        XML with the selected element replaced.

    """
    opening = b"<" + tag
    closing = b"</" + tag + b">"
    start = VALID_COVERAGE_XML.index(opening)
    end = VALID_COVERAGE_XML.index(closing, start) + len(closing)
    return VALID_COVERAGE_XML[:start] + replacement + VALID_COVERAGE_XML[end:]


def _additional_package(*replacements: tuple[bytes, bytes]) -> bytes:
    """Append a deliberately related second package to the sample.

    Returns:
        XML containing the transformed second package.

    """
    start = VALID_COVERAGE_XML.index(b"    <package ")
    end = VALID_COVERAGE_XML.index(b"    </package>", start) + len(b"    </package>")
    package = VALID_COVERAGE_XML[start:end]
    for old, new in replacements:
        if package.count(old) != 1:
            message = "second-package replacement must be unique"
            raise AssertionError(message)
        package = package.replace(old, new)
    return altered(b"  </packages>", package + b"\n  </packages>")


class CoverageXmlDiagnosticContracts(unittest.TestCase):
    """Expose stable rejection reasons at the validation boundary."""

    def _assert_rejected(self, content: bytes, expected: str) -> None:
        """Require one exact validation diagnostic."""
        with self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised:
            coverage_xml_validation.validate(content)
        self.assertEqual(str(raised.exception), expected)

    def test_container_tail_diagnostic_is_exact_at_the_helper_boundary(self) -> None:
        """Bind non-whitespace tail rejection directly to its diagnostic."""
        container = ElementTree.fromstring(b"<public-container/>")
        container.tail = "unexpected"
        with self.assertRaises(coverage_xml_safety.CoverageXmlError) as raised:
            coverage_xml_validation._require_children(  # ruff: ignore[private-member-access]
                container,
                (),
            )
        self.assertEqual(
            str(raised.exception),
            "unexpected public-container tail",
        )

    def test_root_diagnostics_are_exact(self) -> None:
        cases = (
            (
                VALID_COVERAGE_XML.replace(b"<coverage ", b"<public ", 1).replace(
                    b"</coverage>", b"</public>"
                ),
                "root is not coverage",
            ),
            (altered(b' version="7.15.4"', b""), "invalid coverage attributes"),
            (altered(b'lines-valid="2"', b'lines-valid="01"'), "invalid lines-valid"),
            (
                altered(b'lines-covered="1"', b'lines-covered="01"'),
                "invalid lines-covered",
            ),
            (
                altered(b'branches-valid="2"', b'branches-valid="01"'),
                "invalid branches-valid",
            ),
            (
                altered(b'branches-covered="1"', b'branches-covered="01"'),
                "invalid branches-covered",
            ),
            (
                altered(b'lines-covered="1"', b'lines-covered="3"'),
                "covered total exceeds valid total",
            ),
            (
                altered(b'branches-covered="1"', b'branches-covered="3"'),
                "covered total exceeds valid total",
            ),
            (
                altered(
                    b'line-rate="0.5"\n branches-valid',
                    b'line-rate="public"\n branches-valid',
                ),
                "invalid line-rate",
            ),
            (
                altered(
                    b'branches-covered="1" branch-rate="0.5" complexity="0">',
                    b'branches-covered="1" branch-rate="2" complexity="0">',
                ),
                "invalid branch-rate",
            ),
            (
                altered(b'complexity="0">\n  <!--', b'complexity="1">\n  <!--'),
                "invalid complexity",
            ),
            (
                altered(b'timestamp="1786680000000"', b'timestamp="bad"'),
                "invalid timestamp",
            ),
            (altered(b'version="7.15.4"', b'version="7.15.3"'), "invalid version"),
            (
                altered(b"  <sources>", b"unexpected\n  <sources>"),
                "unexpected coverage text",
            ),
            (
                altered(b"  <sources>", b"  <unexpected/>\n  <sources>"),
                "invalid coverage children",
            ),
        )
        for content, expected in cases:
            with self.subTest(expected=expected):
                self._assert_rejected(content, expected)

    def test_source_diagnostics_are_exact(self) -> None:
        cases = (
            (
                altered(b"<sources>", b'<sources extra="1">'),
                "invalid sources attributes",
            ),
            (
                altered(b"<sources>\n", b"<sources>unexpected\n"),
                "unexpected sources text",
            ),
            (
                altered(b"</sources>\n  <packages>", b"</sources>unexpected<packages>"),
                "unexpected sources tail",
            ),
            (_replace_element(b"sources", b"<sources/>"), "invalid sources children"),
            (
                _replace_element(b"sources", b"<sources><unexpected/></sources>"),
                "invalid sources children",
            ),
            (
                altered(b"<source>tools</source>", b"<source/>"),
                "invalid source content",
            ),
            (
                altered(b"<source>tools</source>", b'<source extra="1">tools</source>'),
                "invalid source attributes",
            ),
            (
                altered(b"<source>tools</source>", b"<source><unexpected/></source>"),
                "invalid source content",
            ),
            (
                altered(
                    b"<source>tools</source>\n", b"<source>tools</source>unexpected\n"
                ),
                "unexpected source tail",
            ),
            (
                altered(b"<source>tools</source>", b"<source>.</source>"),
                "invalid relative source",
            ),
            (
                altered(b"<source>tools</source>", b"<source>other</source>"),
                "sources do not match configured coverage scopes",
            ),
        )
        for content, expected in cases:
            with self.subTest(expected=expected):
                self._assert_rejected(content, expected)

    def test_container_and_class_diagnostics_are_exact(self) -> None:
        cases = (
            (
                altered(b"<packages>", b'<packages extra="1">'),
                "invalid packages attributes",
            ),
            (
                altered(b"<packages>\n", b"<packages>unexpected\n"),
                "unexpected packages text",
            ),
            (
                altered(b"</packages>\n", b"</packages>unexpected\n"),
                "unexpected packages tail",
            ),
            (
                _replace_element(b"packages", b"<packages/>"),
                "invalid packages children",
            ),
            (
                _replace_element(b"packages", b"<packages><unexpected/></packages>"),
                "invalid packages children",
            ),
            (
                altered(b'<package name="."', b'<package extra="1" name="."'),
                "invalid package attributes",
            ),
            (
                altered(
                    b'<package name="." line-rate="0.5"',
                    b'<package name="." line-rate="2"',
                ),
                "invalid package line-rate",
            ),
            (
                altered(
                    b'branch-rate="0.5" complexity="0">\n      <classes',
                    b'branch-rate="NaN" complexity="0">\n      <classes',
                ),
                "invalid package branch-rate",
            ),
            (
                altered(b'<package name="."', b'<package name="   "'),
                "invalid package name",
            ),
            (
                altered(
                    b'complexity="0">\n      <classes>',
                    b'complexity="1">\n      <classes>',
                ),
                "invalid package complexity",
            ),
            (
                altered(b"<classes>\n", b"<classes>unexpected\n"),
                "unexpected classes text",
            ),
            (
                altered(b"</classes>\n", b"</classes>unexpected\n"),
                "unexpected classes tail",
            ),
            (_replace_element(b"classes", b"<classes/>"), "invalid classes children"),
            (
                altered(
                    b'<class name="public.py"', b'<class extra="1" name="public.py"'
                ),
                "invalid class attributes",
            ),
            (
                altered(b"<methods/>", b'<methods extra="1"/>'),
                "invalid methods attributes",
            ),
            (
                altered(b"<methods/>", b"<methods><unexpected/></methods>"),
                "invalid methods children",
            ),
            (altered(b"<lines>\n", b"<lines>unexpected\n"), "unexpected lines text"),
            (altered(b"</lines>\n", b"</lines>unexpected\n"), "unexpected lines tail"),
            (
                altered(b"<lines>\n", b"<lines>\n<unexpected/>"),
                "invalid lines children",
            ),
            (
                altered(b"<lines>\n", b'<lines>\n<line number="2" hits="0"/>'),
                "duplicate line number",
            ),
            (
                altered(b'<class name="public.py"', b'<class name="   "'),
                "invalid class name",
            ),
            (
                altered(b'filename="public.py"', b'filename="public.txt"'),
                "invalid relative filename",
            ),
            (
                altered(
                    b'complexity="0"\n         line-rate',
                    b'complexity="1"\n         line-rate',
                ),
                "invalid class complexity",
            ),
            (
                altered(
                    b'line-rate="0.5" branch-rate="0.5">\n          <methods',
                    b'line-rate="0.4" branch-rate="0.5">\n          <methods',
                ),
                "inconsistent class line-rate",
            ),
            (
                altered(
                    b'line-rate="0.5" branch-rate="0.5">\n          <methods',
                    b'line-rate="0.5" branch-rate="0.4">\n          <methods',
                ),
                "inconsistent class branch-rate",
            ),
        )
        for content, expected in cases:
            with self.subTest(expected=expected):
                self._assert_rejected(content, expected)

    def test_independent_collision_and_reconciliation_diagnostics(self) -> None:
        duplicate_filename = _additional_package(
            (
                b'<package name="." line-rate="0.5"',
                b'<package name="second" line-rate="1"',
            ),
            (
                b'complexity="0"\n         line-rate="0.5"',
                b'complexity="0"\n         line-rate="1"',
            ),
            (b'            <line number="2" hits="0"/>\n', b""),
        )
        duplicate_package = _additional_package(
            (
                b'<class name="public.py" filename="public.py"',
                b'<class name="second.py" filename="second.py"',
            ),
        )
        cases = (
            (duplicate_filename, "duplicate package or filename"),
            (duplicate_package, "duplicate package or filename"),
            (
                altered(b'lines-valid="2"', b'lines-valid="1"'),
                "root totals cannot reconcile with line records",
            ),
            (
                altered(b'branches-valid="2"', b'branches-valid="1"'),
                "root totals cannot reconcile with line records",
            ),
            (
                altered(
                    b'lines-covered="1" line-rate="0.5"',
                    b'lines-covered="0" line-rate="0"',
                ),
                "root totals cannot reconcile with line records",
            ),
            (
                altered(
                    b'branches-covered="1" branch-rate="0.5"',
                    b'branches-covered="0" branch-rate="0"',
                ),
                "root totals cannot reconcile with line records",
            ),
            (
                altered(
                    b'lines-valid="2" lines-covered="1" line-rate="0.5"',
                    b'lines-valid="3" lines-covered="3" line-rate="1"',
                ),
                "root totals cannot reconcile with line records",
            ),
            (
                altered(b'lines-valid="2"', b'lines-valid="3"'),
                "inconsistent line-rate",
            ),
            (
                altered(b'branches-valid="2"', b'branches-valid="3"'),
                "inconsistent branch-rate",
            ),
        )
        for content, expected in cases:
            with self.subTest(expected=expected):
                self._assert_rejected(content, expected)

    def test_zero_rates_and_x_in_package_name_are_valid(self) -> None:
        content = altered(b'<package name="."', b'<package name="XML.package"')
        content = altered(
            b'lines-covered="1" line-rate="0.5"',
            b'lines-covered="0" line-rate="0"',
            content,
        )
        content = altered(
            b'branches-covered="1" branch-rate="0.5"',
            b'branches-covered="0" branch-rate="0"',
            content,
        )
        content = altered(
            b'<package name="XML.package" line-rate="0.5" branch-rate="0.5"',
            b'<package name="XML.package" line-rate="0" branch-rate="0"',
            content,
        )
        content = altered(
            b'complexity="0"\n         line-rate="0.5" branch-rate="0.5"',
            b'complexity="0"\n         line-rate="0" branch-rate="0"',
            content,
        )
        content = altered(b'number="1" hits="1"', b'number="1" hits="0"', content)
        content = altered(
            b'condition-coverage="50% (1/2)" missing-branches="exit"',
            b'condition-coverage="0% (0/2)" missing-branches="exit,3"',
            content,
        )

        coverage_xml_validation.validate(content)
        self.assertIn(b'name="XML.package"', content)

    def test_direct_tool_import_compatibility_is_executable(self) -> None:
        script_path = [
            str(TOOLS_DIRECTORY),
            *(entry for entry in sys.path if entry not in {"", str(PROJECT_ROOT)}),
        ]
        cases = (
            ("coverage_xml_lines.py", "validate_line"),
            ("coverage_xml_validation.py", "validate"),
            ("coverage_xml_publication.py", "publish"),
        )
        with patch.object(sys, "path", script_path):
            for filename, public_name in cases:
                with self.subTest(filename=filename):
                    namespace = runpy.run_path(str(TOOLS_DIRECTORY / filename))
                    self.assertIn(public_name, namespace)


if __name__ == "__main__":
    unittest.main(verbosity=2)
