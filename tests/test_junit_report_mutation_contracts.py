"""Mutation-resistant contracts for public JUnit report publication."""

from __future__ import annotations

import os
import socket
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import junit_report

from tests.test_junit_report import _xml
from tests.test_junit_report_encoded_privacy import (
    _encoded,
    _statistics_text,
    _statistics_xml,
)


class JunitReportMutationContracts(unittest.TestCase):
    """Pin exact privacy tokens, diagnostics, and atomic staging options."""

    def test_replacement_map_uses_exact_xml_safe_public_tokens(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            isolated = base / "isolated"
            project = base / "project"
            home = base / "home"
            python_prefix = base / "python"
            base_prefix = base / "base-python"
            system_temp = base / "system-temp"
            for path in (
                isolated,
                project,
                home,
                python_prefix,
                base_prefix,
                system_temp,
            ):
                path.mkdir()
            source = isolated / "report.xml"
            with (
                patch.object(Path, "home", return_value=home),
                patch.object(sys, "prefix", str(python_prefix)),
                patch.object(sys, "base_prefix", str(base_prefix)),
                patch.object(tempfile, "gettempdir", return_value=str(system_temp)),
                patch.object(socket, "gethostname", return_value="public-machine"),
            ):
                replacements = dict(
                    junit_report._replacements(  # ruff: ignore[private-member-access]
                        source,
                        project,
                    )
                )
        self.assertEqual(replacements[str(isolated)], "ISOLATED_TEST_ROOT")
        self.assertEqual(replacements[str(project)], "PROJECT_ROOT")
        self.assertEqual(replacements[str(home)], "USER_HOME")
        self.assertEqual(replacements[str(python_prefix)], "PYTHON_PREFIX")
        self.assertEqual(replacements[str(base_prefix)], "PYTHON_BASE_PREFIX")
        self.assertEqual(replacements[str(system_temp)], "SYSTEM_TEMP")
        self.assertEqual(replacements["public-machine"], "MACHINE_HOST")
        self.assertNotIn(os.sep, replacements)

    def test_replacement_map_normalizes_windows_style_prefixes(self) -> None:
        source = Path(r"C:\isolated\report.xml")
        project = Path(r"C:\project")
        with (
            patch.object(Path, "resolve", autospec=True, side_effect=lambda path: path),
            patch.object(socket, "gethostname", return_value=""),
        ):
            replacement_items = junit_report._replacements(  # ruff: ignore[private-member-access]
                source,
                project,
            )
            replacements = dict(replacement_items)
        self.assertEqual(replacements[r"C:\project"], "PROJECT_ROOT")
        self.assertEqual(replacements["C:/project"], "PROJECT_ROOT")
        self.assertEqual(replacements[r"C:\\project"], "PROJECT_ROOT")
        project_replacements = tuple(
            item for item in replacement_items if item[1] == "PROJECT_ROOT"
        )
        self.assertEqual(
            junit_report._public_text(  # ruff: ignore[private-member-access]
                r"c:\\PROJECT\\tests\\test_public.py",
                project_replacements,
                Path("public-report.xml"),
                "JUnit XML text",
            ),
            "PROJECT_ROOT/tests/test_public.py",
        )

    def test_source_contract_has_exact_contextual_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            missing = root / "missing.xml"
            with self.assertRaises(junit_report.JunitReportError) as missing_error:
                junit_report._regular_source(  # ruff: ignore[private-member-access]
                    missing
                )
            self.assertTrue(
                str(missing_error.exception).startswith(
                    f"cannot inspect JUnit report {missing}: ",
                )
            )

            directory_source = root / "directory.xml"
            directory_source.mkdir()
            with self.assertRaises(junit_report.JunitReportError) as wrong_kind:
                junit_report._regular_source(  # ruff: ignore[private-member-access]
                    directory_source
                )
            self.assertEqual(
                str(wrong_kind.exception),
                f"JUnit report must be a regular non-symbolic file: {directory_source}",
            )

    def test_invalid_xml_diagnostic_retains_exact_source(self) -> None:
        source = Path("public-report.xml")
        with self.assertRaises(junit_report.JunitReportError) as raised:
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                b"<testsuites>",
                (),
                source,
            )
        self.assertTrue(
            str(raised.exception).startswith(
                f"JUnit report is not valid UTF-8 XML: {source}: ",
            )
        )

    def test_sanitizer_diagnostics_bind_exact_source_and_value_location(self) -> None:
        source = Path("exact-public-report.xml")
        public_statistics = _encoded(_statistics_text("public"))
        node_id = "/unknown/tests/test_public.py::test_public"
        node_statistics = _encoded(_statistics_text("public", suffix=node_id))
        errors = {
            "statistics-name": (
                "machine-specific absolute path remains in Hypothesis statistics "
                f"property name from {source}"
            ),
            "statistics-body": (
                "machine-specific absolute path remains in encoded Hypothesis "
                f"statistics from {source}"
            ),
            "text": (
                "machine-specific absolute path remains in JUnit XML text "
                f"from {source}"
            ),
            "element-name": (
                "machine-specific absolute path remains in JUnit XML element name "
                f"from {source}"
            ),
            "attribute-name": (
                "machine-specific absolute path remains in JUnit XML attribute name "
                f"from {source}"
            ),
            "attribute-value": (
                "machine-specific absolute path remains in JUnit XML attribute value "
                f"from {source}"
            ),
        }
        documents = {
            "tail": (
                b"<testsuites><testsuite><testcase/>/unknown/tail/path"
                b"</testsuite></testsuites>"
            ),
            "element-name": (
                b"<testsuites xmlns:p='/unknown/element/schema'><testsuite>"
                b"<p:testcase/></testsuite></testsuites>"
            ),
            "attribute-name": (
                b"<testsuites xmlns:p='/unknown/attribute/schema'>"
                b"<testsuite p:item='public'/></testsuites>"
            ),
            "attribute-value": (
                b"<testsuites><testsuite><testcase classname='/unknown/value/path'/>"
                b"</testsuite></testsuites>"
            ),
            "property-value": (
                b"<testsuites><testsuite><properties>"
                b"<property name='public' value='/unknown/property/path'/>"
                b"</properties></testsuite></testsuites>"
            ),
        }
        cases = (
            (
                _statistics_xml(
                    public_statistics,
                    extra_attributes=" extra='public'",
                ),
                f"invalid Hypothesis statistics property schema in {source}",
            ),
            (
                _statistics_xml("%%%"),
                f"invalid Hypothesis statistics base64 in {source}",
            ),
            (
                _statistics_xml(node_statistics, suffix=node_id),
                errors["statistics-name"],
            ),
            (
                _statistics_xml(_encoded(_statistics_text("/unknown/statistics/path"))),
                errors["statistics-body"],
            ),
            (_xml("/unknown/text/path"), errors["text"]),
            (documents["tail"], errors["text"]),
            (documents["element-name"], errors["element-name"]),
            (documents["attribute-name"], errors["attribute-name"]),
            (documents["attribute-value"], errors["attribute-value"]),
            (documents["property-value"], errors["attribute-value"]),
        )
        for content, expected in cases:
            with self.subTest(expected=expected):
                with self.assertRaises(junit_report.JunitReportError) as raised:
                    junit_report._sanitize(  # ruff: ignore[private-member-access]
                        content,
                        (),
                        source,
                    )
                self.assertEqual(str(raised.exception), expected)

    def test_pytest_node_id_normalization_is_narrow_and_unc_safe(self) -> None:
        source = Path("exact-public-report.xml")
        backslash = chr(92)
        escaped_unc = backslash * 4 + "public-server" + backslash * 2 + "share"
        raw_unc = backslash * 2 + "public-server" + backslash + "share"
        cases = (
            ("name", escaped_unc),
            ("classname", raw_unc),
        )
        for attribute, value in cases:
            content = (
                "<testsuites><testsuite><testcase "
                f"{attribute}='{value}'/></testsuite></testsuites>"
            ).encode()
            with (
                self.subTest(attribute=attribute),
                self.assertRaises(junit_report.JunitReportError) as raised,
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    content,
                    (),
                    source,
                )
            self.assertEqual(
                str(raised.exception),
                "machine-specific absolute path remains in JUnit XML attribute "
                f"value from {source}",
            )

    def test_serialization_is_exact_canonical_utf8(self) -> None:
        content = (
            '<testsuites><testsuite><testcase name="publïc"/></testsuite></testsuites>'
        ).encode()

        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            content,
            (),
            Path("public-report.xml"),
        )

        self.assertEqual(
            sanitized,
            b"<?xml version='1.0' encoding='utf-8'?>\n"
            b'<testsuites><testsuite><testcase name="publ\xc3\xafc" />'
            b"</testsuite></testsuites>",
        )

    def test_invalid_replacement_diagnostic_binds_exact_source(self) -> None:
        source = Path("exact-public-report.xml")
        with self.assertRaises(junit_report.JunitReportError) as raised:
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                _xml("public"),
                (("public", "<invalid"),),
                source,
            )
        self.assertEqual(
            str(raised.exception),
            f"sanitized JUnit report is invalid XML: {source}: unsafe replacement",
        )

    def test_publish_passes_source_to_sanitizer_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "malformed.xml"
            source.write_text("<testsuites>", encoding="utf-8")
            with self.assertRaises(junit_report.JunitReportError) as raised:
                junit_report.publish(source, root / "build/report.xml", root)
        self.assertIn(str(source), str(raised.exception))

    def test_multiple_privacy_issues_use_stable_diagnostic_separator(self) -> None:
        source = Path("public-report.xml")
        content = _xml("private@" + "example.com\n" + "/" + "Users/private/case")
        with self.assertRaises(junit_report.JunitReportError) as raised:
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                content,
                (),
                source,
            )
        self.assertIn(
            "; user-home path on line 2; replace it with a portable placeholder",
            str(raised.exception),
        )

    def test_destination_kind_diagnostics_are_exact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            real_parent = root / "real"
            real_parent.mkdir()
            linked_parent = root / "linked"
            linked_parent.symlink_to(real_parent, target_is_directory=True)
            linked_destination = linked_parent / "report.xml"
            with self.assertRaises(junit_report.JunitReportError) as parent_error:
                junit_report._validate_destination(  # ruff: ignore[private-member-access]
                    linked_destination
                )
            self.assertEqual(
                str(parent_error.exception),
                f"JUnit report directory must be real: {linked_parent}",
            )

            directory_destination = real_parent / "report.xml"
            directory_destination.mkdir()
            with self.assertRaises(junit_report.JunitReportError) as target_error:
                junit_report._validate_destination(  # ruff: ignore[private-member-access]
                    directory_destination
                )
            self.assertEqual(
                str(target_error.exception),
                f"JUnit report destination must be regular: {directory_destination}",
            )

    def test_atomic_publication_uses_destination_local_named_temporary(self) -> None:
        real_mkstemp = tempfile.mkstemp
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source = root / "source.xml"
            destination = root / "build" / "report.xml"
            source.write_bytes(_xml())
            with patch.object(
                tempfile,
                "mkstemp",
                wraps=real_mkstemp,
            ) as reserve:
                published = junit_report.publish(source, destination, root)
            self.assertEqual(published, destination)
            self.assertTrue(destination.is_file())
        reserve.assert_called_once_with(
            dir=destination.parent,
            prefix=".report.xml.",
            suffix=".tmp",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
