"""Contracts for private temporary JUnit report publication."""

from __future__ import annotations

import html
import os
import socket
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from defusedxml import ElementTree
from tools import junit_report


def _xml(body: str = "public") -> bytes:
    """Return one valid public JUnit document.

    Returns:
        UTF-8 XML bytes.

    """
    return (
        "<?xml version='1.0' encoding='utf-8'?>"
        "<testsuites><testsuite name='public'><testcase name='public'>"
        f"<failure>{html.escape(body)}</failure>"
        "</testcase></testsuite></testsuites>"
    ).encode()


class JunitReportPublicationTests(unittest.TestCase):
    """Require sanitized, valid, atomic machine-readable test evidence."""

    def test_temporary_report_is_beside_hypothesis_storage(self) -> None:
        storage = Path("public-root") / ".hypothesis"
        self.assertEqual(
            junit_report.temporary_report_path(storage),
            Path("public-root") / junit_report.REPORT_NAME,
        )

    def test_publish_replaces_every_known_private_prefix(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            isolated = root / "isolated"
            project = root / "project"
            isolated.mkdir()
            project.mkdir()
            source = isolated / junit_report.REPORT_NAME
            destination = project / "build" / "public.xml"
            private = " ".join((
                str(isolated / "case.py"),
                str(project / "tests" / "test_public.py"),
                str(Path.home() / "private"),
                str(Path(sys.prefix) / "library"),
                str(Path(sys.base_prefix) / "base-library"),
                socket.gethostname(),
                "public@example.test",
            ))
            source.write_bytes(_xml(private))

            published = junit_report.publish(source, destination, project)

            content = published.read_text(encoding="utf-8")
        ElementTree.fromstring(content)
        self.assertNotIn(str(isolated), content)
        self.assertNotIn(str(project), content)
        self.assertNotIn(str(Path.home()), content)
        self.assertNotIn(socket.gethostname(), content)
        self.assertIn("ISOLATED_TEST_ROOT", content)
        self.assertIn("PROJECT_ROOT", content)
        self.assertIn("USER_HOME", content)
        self.assertIn("MACHINE_HOST", content)
        self.assertIn("public@example.test", content)

    def test_publish_replaces_an_existing_regular_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            destination = root / "build" / "report.xml"
            destination.parent.mkdir()
            source.write_bytes(_xml("new public evidence"))
            destination.write_bytes(_xml("old public evidence"))

            junit_report.publish(source, destination, root)

            self.assertIn(
                "new public evidence", destination.read_text(encoding="utf-8")
            )
            self.assertFalse(tuple(destination.parent.glob(".report.xml.*.tmp")))

    def test_source_must_be_readable_regular_utf8_xml(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            destination = root / "build" / "report.xml"
            missing = root / "missing.xml"
            directory_source = root / "directory.xml"
            directory_source.mkdir()
            invalid_utf8 = root / "invalid-utf8.xml"
            invalid_utf8.write_bytes(b"\xff")
            malformed = root / "malformed.xml"
            malformed.write_text("<testsuites>", encoding="utf-8")
            target = root / "target.xml"
            target.write_bytes(_xml())
            symbolic = root / "symbolic.xml"
            symbolic.symlink_to(target)
            cases = (missing, directory_source, invalid_utf8, malformed, symbolic)

            for source in cases:
                with (
                    self.subTest(source=source.name),
                    self.assertRaises(junit_report.JunitReportError),
                ):
                    junit_report.publish(source, destination, root)

    def test_source_read_failure_is_contextual(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            source.write_bytes(_xml())
            with (
                patch.object(Path, "read_bytes", side_effect=OSError("read failed")),
                self.assertRaisesRegex(junit_report.JunitReportError, "cannot read"),
            ):
                junit_report.publish(source, root / "build" / "report.xml", root)

    def test_nonreserved_identity_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            source.write_bytes(_xml("private@" + "example.com"))
            with self.assertRaisesRegex(
                junit_report.JunitReportError,
                "private content",
            ):
                junit_report.publish(source, root / "build" / "report.xml", root)

    def test_nonencoded_values_reject_paths_but_allow_public_relative_text(
        self,
    ) -> None:
        for unsafe in (
            "/unknown/private/report/path",
            r"C:\private\report",
            r"\\private-server\private-share\report",
            "file:///unknown/private/report",
        ):
            with (
                self.subTest(value=unsafe),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "machine-specific absolute path remains in JUnit XML text",
                ),
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    _xml(unsafe),
                    (),
                    Path("public-report.xml"),
                )
        public = (
            "tests/test_public.py::test_public "
            "PROJECT_ROOT/tests/test_public.py "
            "https://example.test/path/to/report?next=/relative/value"
        )
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            _xml(public),
            (),
            Path("public-report.xml"),
        )
        self.assertIn(public, sanitized.decode())

        pytest_name = r"test_css[.public{url(/**/commented.png/**/)}-@\\69mport]"
        named_xml = _xml().replace(
            b"<testcase name='public'>",
            f"<testcase name='{html.escape(pytest_name, quote=True)}'>".encode(),
        )
        named = junit_report._sanitize(  # ruff: ignore[private-member-access]
            named_xml,
            (),
            Path("public-report.xml"),
        )
        testcase = ElementTree.fromstring(named).find("./testsuite/testcase")
        assert testcase is not None
        self.assertEqual(testcase.attrib["name"], pytest_name)

    def test_xml_names_cannot_retain_known_private_namespace_paths(self) -> None:
        cases = (
            (
                b"<testsuites xmlns:p='/known/project/schema'><testsuite>"
                b"<p:item/></testsuite></testsuites>"
            ),
            (
                b"<testsuites xmlns:p='/known/project/schema'>"
                b"<testsuite p:item='public'/></testsuites>"
            ),
        )
        for content in cases:
            with (
                self.subTest(content=content),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "private prefix appears in JUnit XML .* name",
                ),
            ):
                junit_report._sanitize(  # ruff: ignore[private-member-access]
                    content,
                    (("/known/project", "PROJECT_ROOT"),),
                    Path("public-report.xml"),
                )

        public_namespace = (
            b"<testsuites xmlns:p='https://example.test/schema'>"
            b"<testsuite p:item='PROJECT_ROOT/tests/test_public.py'/></testsuites>"
        )
        sanitized = junit_report._sanitize(  # ruff: ignore[private-member-access]
            public_namespace,
            (),
            Path("public-report.xml"),
        )
        ElementTree.fromstring(sanitized)

    def test_sanitizer_backstops_residual_prefixes_and_invalid_replacements(
        self,
    ) -> None:
        source = Path("public.xml")
        with self.assertRaisesRegex(junit_report.JunitReportError, "private path"):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                _xml("private-prefix"),
                (("private-prefix", "private-prefix"),),
                source,
            )
        with self.assertRaisesRegex(junit_report.JunitReportError, "invalid XML"):
            junit_report._sanitize(  # ruff: ignore[private-member-access]
                _xml("public-value"),
                (("public-value", "<invalid"),),
                source,
            )

    def test_replacement_prefixes_never_include_the_filesystem_root(self) -> None:
        with patch("tools.junit_report.socket.gethostname", return_value=""):
            replacements = junit_report._replacements(  # ruff: ignore[private-member-access]
                Path("/report.xml"),
                Path("/"),
            )
        self.assertNotIn(os.sep, dict(replacements))

    def test_destination_and_parent_must_not_be_symbolic_or_nonregular(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            source.write_bytes(_xml())
            real_parent = root / "real"
            real_parent.mkdir()
            symbolic_parent = root / "symbolic-parent"
            symbolic_parent.symlink_to(real_parent, target_is_directory=True)
            real_target = real_parent / "real.xml"
            real_target.write_bytes(_xml())
            symbolic_target = real_parent / "symbolic.xml"
            symbolic_target.symlink_to(real_target)
            directory_target = real_parent / "directory.xml"
            directory_target.mkdir()
            cases = (
                symbolic_parent / "report.xml",
                symbolic_target,
                directory_target,
            )

            for destination in cases:
                with (
                    self.subTest(destination=destination),
                    self.assertRaises(junit_report.JunitReportError),
                ):
                    junit_report.publish(source, destination, root)

    def test_destination_inspection_failures_are_contextual(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            destination = root / "build" / "report.xml"
            source.write_bytes(_xml())
            destination.parent.mkdir()
            original_lstat = Path.lstat
            inspection_failure = "destination inspection failed"

            def fail_destination(path: Path) -> os.stat_result:
                if path == destination:
                    raise OSError(inspection_failure)
                return original_lstat(path)

            with (
                patch.object(
                    Path, "lstat", autospec=True, side_effect=fail_destination
                ),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "cannot inspect JUnit report destination",
                ),
            ):
                junit_report.publish(source, destination, root)
            with (
                patch.object(Path, "lstat", side_effect=OSError("parent failed")),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "cannot inspect JUnit report directory",
                ),
            ):
                junit_report._validate_destination(  # ruff: ignore[private-member-access]
                    destination
                )

    def test_publication_failure_removes_reserved_temporary(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            destination = root / "build" / "report.xml"
            source.write_bytes(_xml())
            with (
                patch.object(Path, "replace", side_effect=OSError("public failure")),
                self.assertRaisesRegex(junit_report.JunitReportError, "cannot publish"),
            ):
                junit_report.publish(source, destination, root)
            self.assertFalse(tuple(destination.parent.glob(".report.xml.*.tmp")))

    def test_reservation_failure_is_contextual(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.xml"
            source.write_bytes(_xml())
            with (
                patch(
                    "tools.junit_report.tempfile.mkstemp",
                    side_effect=OSError("public reservation failure"),
                ),
                self.assertRaisesRegex(
                    junit_report.JunitReportError,
                    "cannot reserve",
                ),
            ):
                junit_report.publish(source, root / "build" / "report.xml", root)


if __name__ == "__main__":
    unittest.main(verbosity=2)
