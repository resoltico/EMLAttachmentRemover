"""Fail-closed privacy contracts for encoded pytest XML statistics."""

from __future__ import annotations

import base64
import html
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from defusedxml import ElementTree
from tools import junit_report

from tests.test_junit_report import _xml

PUBLIC_NODE_ID = "tests/test_public.py::test_public"


def _encoded(value: str) -> str:
    """Return canonical base64 for one UTF-8 statistics value.

    Returns:
        ASCII base64 with standard padding.

    """
    return base64.b64encode(value.encode()).decode("ascii")


def _statistics_xml(
    value: str,
    *,
    suffix: str = PUBLIC_NODE_ID,
    extra_attributes: str = "",
) -> bytes:
    """Return pytest's exact Hypothesis statistics property shape.

    Returns:
        One UTF-8 xUnit document.

    """
    name = html.escape(
        f"{junit_report.HYPOTHESIS_STATISTICS_PREFIX}{suffix}", quote=True
    )
    encoded = html.escape(value, quote=True)
    return (
        "<?xml version='1.0' encoding='utf-8'?>"
        "<testsuites><testsuite name='pytest'>"
        "<properties>"
        f"<property name='{name}' value='{encoded}'{extra_attributes}/>"
        "</properties><testcase name='public'/></testsuite></testsuites>"
    ).encode()


def _statistics_text(body: str, *, suffix: str = PUBLIC_NODE_ID) -> str:
    """Return Hypothesis's node-ID-bound human-readable statistics text.

    Returns:
        The expected first line followed by the supplied statistics body.

    """
    return f"{suffix}:\n{body}"


def _published_statistics(content: bytes) -> str:
    """Decode the sole published Hypothesis statistics property.

    Returns:
        Its strict UTF-8 plaintext.

    """
    root = ElementTree.fromstring(content)
    properties = root.findall("./testsuite/properties/property")
    assert len(properties) == 1
    encoded = properties[0].attrib["value"]
    return base64.b64decode(encoded, validate=True).decode()


class EncodedStatisticsPrivacyTests(unittest.TestCase):
    """Require encoded statistics to receive the full public-text policy."""

    def test_valid_statistics_are_sanitized_and_canonically_preserved(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            isolated = root / "isolated"
            project = root / "project"
            isolated.mkdir()
            project.mkdir()
            source = isolated / junit_report.REPORT_NAME
            destination = project / "build" / "public.xml"
            statistics = _statistics_text(
                f"source={project / 'tests' / 'test_public.py'}\n"
                "documentation=https://example.test/path/to/report",
            )
            source.write_bytes(_statistics_xml(_encoded(statistics)))

            junit_report.publish(source, destination, project)

            published = destination.read_bytes()
        decoded = _published_statistics(published)
        self.assertEqual(
            decoded,
            f"{PUBLIC_NODE_ID}:\nsource=PROJECT_ROOT/tests/test_public.py\n"
            "documentation=https://example.test/path/to/report",
        )
        root_element = ElementTree.fromstring(published)
        property_element = root_element.find("./testsuite/properties/property")
        assert property_element is not None
        self.assertEqual(property_element.attrib["value"], _encoded(decoded))

    def test_encoded_absolute_paths_and_private_identity_fail_closed(self) -> None:
        unsafe_values = (
            "/private/var/folders/private-report",
            "C:\\" + "Users" + r"\private-user\report",
            r"\\private-server\private-share\report",
            "file:///private/var/private-report",
        )
        for unsafe in unsafe_values:
            with (
                self.subTest(value=unsafe),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "absolute path remains in encoded Hypothesis statistics|"
                    "JUnit report contains private content",
                ),
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    _statistics_xml(_encoded(_statistics_text(unsafe))),
                    (),
                    Path("public-report.xml"),
                )
        with self.assertRaisesRegex(
            junit_report.JunitReportError,
            "JUnit report contains private content",
        ):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                _statistics_xml(_encoded(_statistics_text("private@" + "example.com"))),
                (),
                Path("public-report.xml"),
            )

    def test_malformed_noncanonical_and_non_utf8_statistics_are_rejected(
        self,
    ) -> None:
        cases = (
            ("%%%", "invalid Hypothesis statistics base64"),
            ("AB==", "noncanonical Hypothesis statistics base64"),
            ("/w==", "Hypothesis statistics are not valid UTF-8"),
        )
        for encoded, message in cases:
            with (
                self.subTest(message=message),
                self.assertRaisesRegex(junit_report.JunitReportError, message),
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    _statistics_xml(encoded),
                    (),
                    Path("public-report.xml"),
                )

    def test_unexpected_statistics_property_schemas_are_rejected(self) -> None:
        encoded = _encoded(_statistics_text("public statistics"))
        duplicate = _statistics_xml(encoded).replace(
            b"</properties>",
            (
                "<property name='hypothesis-statistics-"
                f"{PUBLIC_NODE_ID}' value='{encoded}'/></properties>"
            ).encode(),
        )
        wrong_parent = (
            _statistics_xml(encoded)
            .replace(
                b"<properties>",
                b"<properties></properties>",
            )
            .replace(b"</properties><testcase", b"<testcase")
        )
        missing_value = _statistics_xml(encoded).replace(
            f" value='{encoded}'".encode(),
            b"",
        )
        child_payload = _statistics_xml(encoded).replace(
            b"/>", b"><child/></property>", 1
        )
        text_payload = _statistics_xml(encoded).replace(b"/>", b">text</property>", 1)
        cases = (
            _statistics_xml(""),
            _statistics_xml(encoded, suffix=""),
            _statistics_xml(encoded, extra_attributes=" extra='unexpected'"),
            duplicate,
            wrong_parent,
            missing_value,
            child_payload,
            text_payload,
            _statistics_xml(encoded)
            .replace(b"<testsuites>", b"")
            .replace(b"</testsuites>", b""),
            _statistics_xml(encoded)
            .replace(b"<property", b"<system-out")
            .replace(b"/>", b"></system-out>", 1),
            _statistics_xml(encoded).replace(
                b"hypothesis-statistics-",
                b"Hypothesis-statistics-",
            ),
            _statistics_xml(encoded).replace(
                b"hypothesis-statistics-",
                b"hypothesis-statistics",
            ),
            _statistics_xml(encoded).replace(
                b"hypothesis-statistics-",
                b" hypothesis-statistics-",
            ),
        )
        for content in cases:
            with (
                self.subTest(content=content),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "invalid Hypothesis statistics property schema",
                ),
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    content,
                    (),
                    Path("public-report.xml"),
                )

    def test_unsafe_encoded_report_does_not_replace_prior_publication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / "source.xml"
            destination = root / "build" / "report.xml"
            destination.parent.mkdir()
            previous = _xml("previous public report")
            destination.write_bytes(previous)
            source.write_bytes(
                _statistics_xml(
                    _encoded(_statistics_text("/unknown/private/report/path"))
                )
            )

            with self.assertRaises(junit_report.JunitReportError):
                junit_report.publish(source, destination, root)

            self.assertEqual(destination.read_bytes(), previous)
            self.assertFalse(tuple(destination.parent.glob(".report.xml.*.tmp")))

    def test_node_id_binding_and_unknown_base64_scope_are_explicit(
        self,
    ) -> None:
        mismatched = _statistics_xml(
            _encoded(
                _statistics_text("public", suffix="tests/test_other.py::test_other")
            )
        )
        with self.assertRaisesRegex(
            junit_report.JunitReportError,
            "node ID does not match property",
        ):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                mismatched,
                (),
                Path("public-report.xml"),
            )
        unrelated = _statistics_xml(_encoded(_statistics_text("public"))).replace(
            b"hypothesis-statistics-",
            b"public-statistics-",
        )
        # Unknown properties stay opaque: guessing Base64 would misclassify IDs/hashes.
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            unrelated,
            (),
            Path("public-report.xml"),
        )
        root = ElementTree.fromstring(sanitized)
        property_element = root.find("./testsuite/properties/property")
        assert property_element is not None
        self.assertEqual(
            property_element.attrib["value"],
            _encoded(_statistics_text("public")),
        )

    def test_encoded_values_are_not_blindly_replaced_before_decoding(self) -> None:
        statistics = _statistics_text("public statistics")
        encoded = _encoded(statistics)
        token = encoded[3:10]
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            _statistics_xml(encoded),
            ((token, "PUBLIC_TOKEN"),),
            Path("public-report.xml"),
        )
        self.assertEqual(_published_statistics(sanitized), statistics)

    def test_coordinated_node_id_sanitization_must_remain_bound(self) -> None:
        node_id = "/known/project/tests/test_public.py::test_public"
        statistics = _statistics_text("public statistics", suffix=node_id)
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            _statistics_xml(_encoded(statistics), suffix=node_id),
            (("/known/project", "PROJECT_ROOT"),),
            Path("public-report.xml"),
        )
        root = ElementTree.fromstring(sanitized)
        property_element = root.find("./testsuite/properties/property")
        assert property_element is not None
        self.assertEqual(
            property_element.attrib["name"],
            "hypothesis-statistics-PROJECT_ROOT/tests/test_public.py::test_public",
        )
        self.assertEqual(
            _published_statistics(sanitized),
            "PROJECT_ROOT/tests/test_public.py::test_public:\npublic statistics",
        )

        escaped_node_id = (
            r"tests/test_css.py::test_css[.public{url(/**/public.png/**/)}-@\\69mport]"
        )
        escaped_statistics = _statistics_text("public", suffix=escaped_node_id)
        escaped = junit_report._sanitize(  # ruff: ignore[private-member-access]
            _statistics_xml(_encoded(escaped_statistics), suffix=escaped_node_id),
            (),
            Path("public-report.xml"),
        )
        self.assertEqual(_published_statistics(escaped), escaped_statistics)

        colliding_node_id = "PROJECT_ROOT/tests/test_public.py::test_public"
        second_property = (
            _statistics_xml(
                _encoded(_statistics_text("public", suffix=colliding_node_id)),
                suffix=colliding_node_id,
            )
            .split(b"<properties>", maxsplit=1)[1]
            .split(b"</properties>", maxsplit=1)[0]
        )
        collision = _statistics_xml(_encoded(statistics), suffix=node_id).replace(
            b"</properties>", second_property + b"</properties>"
        )
        with self.assertRaisesRegex(
            junit_report.JunitReportError,
            "duplicate sanitized Hypothesis statistics node ID",
        ):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                collision,
                (("/known/project", "PROJECT_ROOT"),),
                Path("public-report.xml"),
            )

    def test_element_tail_is_sanitized_and_output_is_revalidated(self) -> None:
        content = b"<testsuites><testsuite><testcase/>\n</testsuite></testsuites>"
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            content,
            (("\n", "\t"),),
            Path("public-report.xml"),
        )
        self.assertIn(b"\t", sanitized)

        parsed = ElementTree.fromstring(_xml("public"))
        with (
            patch.object(
                ElementTree,
                "fromstring",
                side_effect=(
                    parsed,
                    ElementTree.ParseError("revalidation failed"),
                ),
            ),
            self.assertRaisesRegex(
                junit_report.JunitReportError,
                "sanitized JUnit report is invalid XML",
            ),
        ):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                _xml("public"),
                (),
                Path("public-report.xml"),
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
