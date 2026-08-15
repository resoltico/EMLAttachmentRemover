"""Contracts for public-safe Coverage XML paths, parsing, and line records."""

from __future__ import annotations

import socket
import unittest
from unittest.mock import patch

from defusedxml import ElementTree
from tools import coverage_xml_lines, coverage_xml_safety

from tests.coverage_xml_samples import VALID_COVERAGE_XML, altered


def _line(xml: str) -> object:
    """Return one safely parsed synthetic line element.

    Returns:
        The parsed element accepted by the typed validator at runtime.

    """
    return ElementTree.fromstring(xml)


class CoverageXmlPathSafetyTests(unittest.TestCase):
    """Reject every private or nonportable path representation."""

    def test_relative_path_contract_accepts_only_normalized_portable_values(
        self,
    ) -> None:
        coverage_xml_safety.require_relative_path(
            "src/eml_attachment_remover",
            "source",
            required_suffix=None,
        )
        coverage_xml_safety.require_relative_path(
            "public.py",
            "filename",
            required_suffix=".py",
        )
        cases = (
            ("", False),
            (".", False),
            ("..", False),
            (" tools", False),
            ("tools\\private", False),
            ("/private/source", False),
            ("C:\\private\\source", False),
            ("tools//private", False),
            ("tools/../private", False),
            ("public.txt", True),
        )
        for value, python_file in cases:
            with self.subTest(value=value):
                with self.assertRaises(
                    coverage_xml_safety.CoverageXmlError,
                ) as raised:
                    coverage_xml_safety.require_relative_path(
                        value,
                        "public field",
                        required_suffix=".py" if python_file else None,
                    )
                self.assertEqual(str(raised.exception), "invalid relative public field")

    def test_safe_parser_accepts_realistic_xml_and_https_comments(self) -> None:
        root = coverage_xml_safety.require_public_root(VALID_COVERAGE_XML)

        self.assertEqual(root.tag, "coverage")

    def test_safe_parser_rejects_malformed_or_entity_bearing_xml(self) -> None:
        cases = (
            b"\xff",
            b"<coverage>",
            (
                b"<!DOCTYPE coverage [<!ENTITY private 'public'>]>"
                b"<coverage>&private;</coverage>"
            ),
        )
        for content in cases:
            with self.subTest(content=content[:20]):
                with self.assertRaises(
                    coverage_xml_safety.CoverageXmlError,
                ) as raised:
                    coverage_xml_safety.require_public_root(content)
                self.assertEqual(
                    str(raised.exception),
                    "Coverage XML is not safe, valid UTF-8 XML",
                )

    def test_safe_parser_rejects_all_absolute_path_dialects(self) -> None:
        cases = (
            b"/private/report.py",
            b"C:\\Users\\private\\report.py",
            b"\\\\private-host\\share\\report.py",
            b"//private-host/share/report.py",
            b"file:///private/report.py",
            b"file:private-report.py",
            b"/",
            b"&#47;private&#47;encoded.py",
        )
        marker = b'<class name="public.py"'
        for value in cases:
            content = altered(marker, b'<class name="' + value + b'"')
            with self.subTest(value=value):
                with self.assertRaises(
                    coverage_xml_safety.CoverageXmlError,
                ) as raised:
                    coverage_xml_safety.require_public_root(content)
                self.assertEqual(
                    str(raised.exception),
                    "Coverage XML contains private identity or an absolute file path",
                )

    def test_safe_parser_rejects_email_and_runtime_hostname_identity(self) -> None:
        marker = b'<class name="public.py"'
        private_email = altered(
            marker,
            b'<class name="owner@' + b'private.example"',
        )
        hostname = "private-machine.example"
        private_host = altered(marker, f'<class name="{hostname}"'.encode())
        with self.assertRaises(coverage_xml_safety.CoverageXmlError):
            coverage_xml_safety.require_public_root(private_email)
        with (
            patch(
                "tools.repository_hygiene_policy.socket.gethostname",
                return_value=hostname,
            ),
            self.assertRaises(coverage_xml_safety.CoverageXmlError),
        ):
            coverage_xml_safety.require_public_root(private_host)
        self.assertNotEqual(socket.gethostname(), "")


class CoverageXmlLineContractTests(unittest.TestCase):
    """Reconcile every line and branch representation exactly."""

    def test_plain_and_complete_branch_lines_return_exact_totals(self) -> None:
        plain = coverage_xml_lines.validate_line(
            _line('<line number="1" hits="0"/>'),  # type: ignore[arg-type]
        )
        branch = coverage_xml_lines.validate_line(
            _line(  # type: ignore[arg-type]
                '<line number="2" hits="1" branch="true" '
                'condition-coverage="100% (2/2)"/>'
            ),
        )
        fractional = coverage_xml_lines.validate_line(
            _line(  # type: ignore[arg-type]
                '<line number="3" hits="1" branch="true" '
                'condition-coverage="33% (1/3)" missing-branches="exit,4"/>'
            ),
        )

        self.assertEqual(plain, coverage_xml_lines.CoverageTotals(1, 0, 0, 0))
        self.assertEqual(branch, coverage_xml_lines.CoverageTotals(1, 1, 2, 2))
        self.assertEqual(
            fractional,
            coverage_xml_lines.CoverageTotals(1, 1, 3, 1),
        )
        self.assertEqual(
            coverage_xml_lines.CoverageTotals.combine((plain, branch)),
            coverage_xml_lines.CoverageTotals(2, 1, 2, 2),
        )
        self.assertEqual(
            coverage_xml_lines.CoverageTotals.combine(()),
            coverage_xml_lines.CoverageTotals(),
        )

    def test_incomplete_branch_records_require_exact_missing_targets(self) -> None:
        record = coverage_xml_lines.validate_line(
            _line(  # type: ignore[arg-type]
                '<line number="1" hits="1" branch="true" '
                'condition-coverage="50% (1/2)" missing-branches="exit"/>'
            ),
        )

        self.assertEqual(record, coverage_xml_lines.CoverageTotals(1, 1, 2, 1))

    def test_invalid_line_and_branch_variants_fail_closed(self) -> None:
        cases = (
            ('<line number="1" hits="1" private="value"/>', "invalid line structure"),
            ('<line number="1" hits="1"><private/></line>', "invalid line structure"),
            ('<line number="1" hits="1">private</line>', "invalid line structure"),
            (
                '<lines><line number="1" hits="1"/>private</lines>',
                "invalid line structure",
            ),
            ('<line number="0" hits="1"/>', "invalid line number"),
            ('<line number="1" hits="2"/>', "invalid line hits"),
            ('<line number="1" hits="1" branch="true"/>', "invalid line structure"),
            (
                (
                    '<line number="1" hits="1" branch="false" '
                    'condition-coverage="100% (2/2)"/>'
                ),
                "invalid branch marker",
            ),
            (
                '<line number="1" hits="1" branch="true" condition-coverage="public"/>',
                "invalid condition coverage",
            ),
            (
                (
                    '<line number="1" hits="1" branch="true" '
                    'condition-coverage="100% (3/2)"/>'
                ),
                "inconsistent condition coverage",
            ),
            (
                (
                    '<line number="1" hits="1" branch="true" '
                    'condition-coverage="40% (1/2)" missing-branches="exit"/>'
                ),
                "inconsistent condition coverage",
            ),
            (
                (
                    '<line number="1" hits="1" branch="true" '
                    'condition-coverage="50% (1/2)" missing-branches="0"/>'
                ),
                "invalid missing branches",
            ),
            (
                (
                    '<line number="1" hits="1" branch="true" '
                    'condition-coverage="50% (1/2)" missing-branches="exit,3"/>'
                ),
                "inconsistent missing branches",
            ),
            (
                (
                    '<line number="1" hits="1" branch="true" '
                    'condition-coverage="50% (1/2)"/>'
                ),
                "inconsistent missing branches",
            ),
        )
        for xml, expected in cases:
            element = ElementTree.fromstring(xml)
            if element.tag == "lines":
                element = next(iter(element))
            with self.subTest(xml=xml):
                with self.assertRaises(
                    coverage_xml_safety.CoverageXmlError,
                ) as raised:
                    coverage_xml_lines.validate_line(element)
                self.assertEqual(str(raised.exception), expected)


if __name__ == "__main__":
    unittest.main(verbosity=2)
