"""Public xUnit2 hierarchy acceptance and rejection contracts."""

from __future__ import annotations

import pytest
from defusedxml import ElementTree
from tools import junit_xunit2_schema


def _issue(content: bytes) -> str | None:
    return junit_xunit2_schema.validation_issue(ElementTree.fromstring(content))


def test_schema_accepts_zero_test_suite_and_complete_testcase_structure() -> None:
    assert _issue(b"<testsuites><testsuite/></testsuites>") is None
    assert (
        _issue(
            b"<testsuites><testsuite><properties><property name='x' value='y'/>"
            b"</properties><testcase><properties><property name='x' value='y'/>"
            b"</properties><failure/><system-out/><system-err/></testcase>"
            b"<testcase><error/></testcase><testcase><skipped/></testcase>"
            b"</testsuite></testsuites>"
        )
        is None
    )


@pytest.mark.parametrize(
    ("content", "expected"),
    [
        (b"<not-junit/>", "root element must be testsuites"),
        (b"<testsuites/>", "testsuites must contain at least one testsuite"),
        (
            b"<testsuites><testcase/></testsuites>",
            "testsuites cannot contain testcase",
        ),
        (
            b"<testsuites>text<testsuite/></testsuites>",
            "testsuites cannot contain non-whitespace text",
        ),
        (
            b"<testsuites><testsuite><unknown/></testsuite></testsuites>",
            "testsuite cannot contain unknown",
        ),
        (
            b"<testsuites><testsuite><properties/></testsuite></testsuites>",
            "properties must contain at least one property",
        ),
        (
            (
                b"<testsuites><testsuite><testcase/><properties><property "
                b"name='x' value='y'/></properties></testsuite></testsuites>"
            ),
            "testsuite properties must be first",
        ),
        (
            (
                b"<testsuites><testsuite><testcase><properties><property "
                b"name='x' value='y'/></properties><properties><property "
                b"name='x' value='y'/></properties></testcase></testsuite>"
                b"</testsuites>"
            ),
            "testcase cannot contain duplicate properties",
        ),
        (
            (
                b"<testsuites><testsuite><testcase><failure/><error/></testcase>"
                b"</testsuite></testsuites>"
            ),
            "testcase cannot contain multiple outcomes",
        ),
        (
            b"<testsuites><testsuite><testcase/>tail</testsuite></testsuites>",
            "testcase cannot have non-whitespace tail",
        ),
    ],
)
def test_schema_reports_exact_structural_issue(content: bytes, expected: str) -> None:
    assert _issue(content) == expected
