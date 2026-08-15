"""Contracts for the bounded pytest xUnit2 outer structure."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

from defusedxml import ElementTree
from tools import junit_report

SOURCE = Path("public-report.xml")


class JunitXunit2SchemaTests(unittest.TestCase):
    """Reject valid XML that is not a bounded pytest xUnit2 report."""

    def assert_invalid_structure(self, content: bytes, issue: str) -> None:
        """Require one exact, source-bound outer-structure diagnostic."""
        with self.assertRaisesRegex(
            junit_report.JunitReportError,
            re.escape(f"invalid pytest xUnit2 structure in {SOURCE}: {issue}"),
        ):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                content,
                (),
                SOURCE,
            )

    def test_accepts_pytest_hierarchy_without_parsing_leaf_prose(self) -> None:
        content = b"""<testsuites extension='public'>
          <testsuite extension='public'>
            <properties><property name='public' value='public'/></properties>
            <testcase extension='public'>
              <properties><property name='case' value='public'/></properties>
              <failure message='public'>arbitrary &lt;failure prose&gt;</failure>
              <system-out>public output</system-out>
              <system-err>public error output</system-err>
            </testcase>
            <testcase><error message='public'>public error</error></testcase>
            <testcase><skipped message='public'>public reason</skipped></testcase>
            <testcase/>
          </testsuite>
        </testsuites>"""

        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            content,
            (),
            SOURCE,
        )

        root = ElementTree.fromstring(sanitized)
        self.assertEqual(root.tag, "testsuites")
        self.assertEqual(root.attrib["extension"], "public")
        self.assertIn("arbitrary <failure prose>", "".join(root.itertext()))

    def test_accepts_a_zero_test_suite(self) -> None:
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            b"<testsuites><testsuite/></testsuites>",
            (),
            SOURCE,
        )
        self.assertEqual(ElementTree.fromstring(sanitized).tag, "testsuites")

    def test_rejects_wrong_or_empty_roots_and_non_suite_root_children(self) -> None:
        cases = (
            (b"<not-junit/>", "root element must be testsuites"),
            (
                b"<testsuites/>",
                "testsuites must contain at least one testsuite",
            ),
            (
                b"<testsuites><testcase/></testsuites>",
                "testsuites cannot contain testcase",
            ),
            (
                b"<testsuites><testsuite/><unknown/></testsuites>",
                "testsuites cannot contain unknown",
            ),
        )
        for content, issue in cases:
            with self.subTest(issue=issue):
                self.assert_invalid_structure(content, issue)

    def test_rejects_unknown_or_incorrectly_nested_elements(self) -> None:
        cases = (
            (
                b"<testsuites><testsuite><testsuite/></testsuite></testsuites>",
                "testsuite cannot contain testsuite",
            ),
            (
                b"<testsuites><testsuite><system-out/></testsuite></testsuites>",
                "testsuite cannot contain system-out",
            ),
            (
                b"<testsuites><testsuite><system-err/></testsuite></testsuites>",
                "testsuite cannot contain system-err",
            ),
            (
                (
                    b"<testsuites><testsuite><properties><testcase/>"
                    b"</properties></testsuite></testsuites>"
                ),
                "properties cannot contain testcase",
            ),
            (
                (
                    b"<testsuites><testsuite><testcase><property/>"
                    b"</testcase></testsuite></testsuites>"
                ),
                "testcase cannot contain property",
            ),
            (
                (
                    b"<testsuites><testsuite><testcase><unknown/>"
                    b"</testcase></testsuite></testsuites>"
                ),
                "testcase cannot contain unknown",
            ),
            (
                (
                    b"<testsuites><testsuite><testcase><failure><system-out/>"
                    b"</failure></testcase></testsuite></testsuites>"
                ),
                "failure cannot contain system-out",
            ),
        )
        for content, issue in cases:
            with self.subTest(issue=issue):
                self.assert_invalid_structure(content, issue)

    def test_rejects_invalid_properties_order_and_multiple_outcomes(self) -> None:
        property_element = (
            b"<properties><property name='public' value='public'/></properties>"
        )
        cases = (
            (
                b"<testsuites><testsuite><properties/></testsuite></testsuites>",
                "properties must contain at least one property",
            ),
            (
                b"<testsuites><testsuite>"
                + property_element * 2
                + b"</testsuite></testsuites>",
                "testsuite cannot contain duplicate properties",
            ),
            (
                b"<testsuites><testsuite><testcase/>"
                + property_element
                + b"</testsuite></testsuites>",
                "testsuite properties must be first",
            ),
            (
                b"<testsuites><testsuite><testcase>"
                + property_element * 2
                + b"</testcase></testsuite></testsuites>",
                "testcase cannot contain duplicate properties",
            ),
            (
                b"<testsuites><testsuite><testcase><failure/>"
                + property_element
                + b"</testcase></testsuite></testsuites>",
                "testcase properties must be first",
            ),
            (
                (
                    b"<testsuites><testsuite><testcase><failure/><error/>"
                    b"</testcase></testsuite></testsuites>"
                ),
                "testcase cannot contain multiple outcomes",
            ),
        )
        for content, issue in cases:
            with self.subTest(issue=issue):
                self.assert_invalid_structure(content, issue)

    def test_rejects_nonwhitespace_structural_text_and_tails(self) -> None:
        cases = (
            (
                b"<testsuites>text<testsuite/></testsuites>",
                "testsuites cannot contain non-whitespace text",
            ),
            (
                b"<testsuites><testsuite>text</testsuite></testsuites>",
                "testsuite cannot contain non-whitespace text",
            ),
            (
                (
                    b"<testsuites><testsuite><properties>text"
                    b"<property name='public' value='public'/></properties>"
                    b"</testsuite></testsuites>"
                ),
                "properties cannot contain non-whitespace text",
            ),
            (
                (
                    b"<testsuites><testsuite><testcase>text</testcase>"
                    b"</testsuite></testsuites>"
                ),
                "testcase cannot contain non-whitespace text",
            ),
            (
                b"<testsuites><testsuite><testcase/>tail</testsuite></testsuites>",
                "testcase cannot have non-whitespace tail",
            ),
        )
        for content, issue in cases:
            with self.subTest(issue=issue):
                self.assert_invalid_structure(content, issue)


if __name__ == "__main__":
    unittest.main(verbosity=2)
