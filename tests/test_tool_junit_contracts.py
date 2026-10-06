"""Behavioral contracts for private JUnit sanitization and publication."""

from __future__ import annotations

import base64
import html
import os
import socket
import sys
from pathlib import Path

import pytest
from defusedxml import ElementTree
from tools import junit_report

PUBLICATION_FAILURE = "publication failure"


def _xml(body: str = "public") -> bytes:
    return (
        "<testsuites><testsuite name='public'><testcase name='public'>"
        f"<failure>{html.escape(body)}</failure>"
        "</testcase></testsuite></testsuites>"
    ).encode()


def _statistics_xml(
    value: str, suffix: str = "tests/test_public.py::test_public"
) -> bytes:
    encoded = html.escape(value, quote=True)
    name = html.escape(f"hypothesis-statistics-{suffix}", quote=True)
    return (
        "<testsuites><testsuite name='suite'><properties>"
        f"<property name='{name}' value='{encoded}'/>"
        "</properties><testcase name='public'/></testsuite></testsuites>"
    ).encode()


def test_publish_replaces_known_machine_prefixes_and_preserves_public_email(
    tmp_path: Path,
) -> None:
    isolated = tmp_path / "isolated"
    project = tmp_path / "project"
    isolated.mkdir()
    project.mkdir()
    source = isolated / junit_report.REPORT_NAME
    destination = project / "build" / "report.xml"
    private = " ".join((
        str(isolated / "case.py"),
        str(project / "tests" / "test_public.py"),
        str(Path.home() / "private"),
        str(Path(sys.prefix) / "library"),
        socket.gethostname(),
        "public@example.test",
    ))
    source.write_bytes(_xml(private))
    junit_report.publish(source, destination, project)
    public = destination.read_text(encoding="utf-8")
    ElementTree.fromstring(public)
    assert str(isolated) not in public
    assert str(project) not in public
    assert "ISOLATED_TEST_ROOT" in public
    assert "PROJECT_ROOT" in public
    assert "USER_HOME" in public
    assert "MACHINE_HOST" in public
    assert "public@example.test" in public


@pytest.mark.parametrize(
    "content",
    [
        b"\xff",
        b"<testsuites>",
        b"<testsuites><testsuite><unknown/></testsuite></testsuites>",
    ],
)
def test_publish_rejects_malformed_or_non_xunit_sources(
    tmp_path: Path, content: bytes
) -> None:
    source = tmp_path / "source.xml"
    source.write_bytes(content)
    with pytest.raises(junit_report.JunitReportError):
        junit_report.publish(source, tmp_path / "build" / "report.xml", tmp_path)


def test_publish_rejects_missing_directory_and_symbolic_sources(tmp_path: Path) -> None:
    destination = tmp_path / "build" / "report.xml"
    directory = tmp_path / "directory.xml"
    directory.mkdir()
    target = tmp_path / "target.xml"
    target.write_bytes(_xml())
    symbolic = tmp_path / "symbolic.xml"
    symbolic.symlink_to(target)
    for source in (tmp_path / "missing.xml", directory, symbolic):
        with pytest.raises(junit_report.JunitReportError):
            junit_report.publish(source, destination, tmp_path)


@pytest.mark.parametrize(
    "unsafe",
    [
        "/unknown/private/report",
        r"C:\private\report",
        r"\\private-server\share\report",
        "file:///unknown/private/report",
    ],
)
def test_sanitize_rejects_residual_absolute_text(unsafe: str) -> None:
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(_xml(unsafe), (), Path("public.xml"))


def test_statistics_are_decoded_sanitized_and_reencoded(tmp_path: Path) -> None:
    node_id = "tests/test_public.py::test_public"
    isolated = tmp_path / "isolated"
    project = tmp_path / "project"
    isolated.mkdir()
    project.mkdir()
    statistics = f"{node_id}:\nsource={project / 'case.py'}\npublic"
    encoded = base64.b64encode(statistics.encode()).decode()
    source = isolated / "source.xml"
    destination = tmp_path / "destination.xml"
    source.write_bytes(_statistics_xml(encoded, node_id))
    junit_report.publish(source, destination, project)
    property_element = ElementTree.fromstring(destination.read_bytes()).find(
        "./testsuite/properties/property"
    )
    assert property_element is not None
    decoded = base64.b64decode(property_element.attrib["value"], validate=True).decode()
    assert decoded.startswith(f"{node_id}:\nsource=PROJECT_ROOT/case.py")


@pytest.mark.parametrize("encoded", ["%%", "AB==", "/w=="])
def test_statistics_reject_invalid_or_noncanonical_encodings(encoded: str) -> None:
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(_statistics_xml(encoded), (), Path("public.xml"))


def test_publish_failure_removes_the_reserved_temporary_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "source.xml"
    destination = tmp_path / "build" / "report.xml"
    source.write_bytes(_xml())

    def fail_publish(_source: Path, _destination: Path) -> None:
        raise OSError(PUBLICATION_FAILURE)

    monkeypatch.setattr(Path, "replace", fail_publish)
    with pytest.raises(junit_report.JunitReportError):
        junit_report.publish(source, destination, tmp_path)
    assert not list(destination.parent.glob(".report.xml.*.tmp"))


def test_low_level_source_and_replacement_failures_are_contextual(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assert junit_report.temporary_report_path(tmp_path / ".hypothesis") == (
        tmp_path / junit_report.REPORT_NAME
    )
    with pytest.raises(junit_report.JunitReportError):
        junit_report._regular_source(tmp_path / "missing.xml")
    source = tmp_path / "source.xml"
    source.write_bytes(_xml())

    def fail_read(_path: Path) -> bytes:
        raise OSError(PUBLICATION_FAILURE)

    monkeypatch.setattr(Path, "read_bytes", fail_read)
    with pytest.raises(junit_report.JunitReportError):
        junit_report._regular_source(source)
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_replacements((("public", "<invalid"),), source)


def test_private_names_statistics_shapes_and_tails_fail_closed() -> None:
    source = Path("public.xml")
    with pytest.raises(junit_report.JunitReportError):
        junit_report._public_text(
            "private-prefix", (("private-prefix", "private-prefix"),), source, "text"
        )
    with pytest.raises(junit_report.JunitReportError):
        junit_report._public_text(
            "private@" + "example" + ".com",
            (),
            source,
            "text",
        )
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_xml_name(
            "/private-tag", (("/private", "PUBLIC"),), source, "name"
        )
    empty_statistics = _statistics_xml("")
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(empty_statistics, (), source)
    node_id = "tests/test_public.py::test_public"
    mismatched = base64.b64encode(b"different:\npublic").decode()
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(_statistics_xml(mismatched, node_id), (), source)
    duplicated = _statistics_xml(
        base64.b64encode(f"{node_id}:\npublic".encode()).decode()
    )
    duplicated = duplicated.replace(
        b"</properties>",
        duplicated.split(b"<properties>", 1)[1].split(b"</properties>", 1)[0]
        + b"</properties>",
    )
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(duplicated, (), source)
    sanitized = junit_report._sanitize(
        b"<testsuites><testsuite><testcase/>\n</testsuite></testsuites>",
        (("\n", "\t"),),
        source,
    )
    assert b"\t" in sanitized


def test_destination_and_reservation_failures_preserve_prior_report(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "source.xml"
    source.write_bytes(_xml())
    blocking_parent = tmp_path / "blocking"
    blocking_parent.write_bytes(b"not-directory")
    with pytest.raises(junit_report.JunitReportError):
        junit_report.publish(source, blocking_parent / "report.xml", tmp_path)
    target = tmp_path / "target.xml"
    target.write_bytes(_xml())
    symbolic = tmp_path / "symbolic.xml"
    symbolic.symlink_to(target)
    with pytest.raises(junit_report.JunitReportError):
        junit_report.publish(source, symbolic, tmp_path)

    def fail_reservation(*_arguments: object, **_keywords: object) -> int:
        raise OSError(PUBLICATION_FAILURE)

    monkeypatch.setattr("tools.junit_report.tempfile.mkstemp", fail_reservation)
    with pytest.raises(junit_report.JunitReportError):
        junit_report.publish(source, tmp_path / "build" / "report.xml", tmp_path)


def test_replacement_and_destination_inspection_edges(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(Path, "resolve", lambda _path: Path(os.sep))
    monkeypatch.setattr("tools.junit_report.socket.gethostname", lambda: "")
    assert junit_report._replacements(Path("report.xml"), Path("project")) == ()
    monkeypatch.undo()
    parent = tmp_path / "parent"
    parent.write_bytes(b"not-directory")
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_destination(parent / "report.xml")
    directory = tmp_path / "directory"
    directory.mkdir()
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_destination(directory)
    destination = tmp_path / "report.xml"
    original_lstat = Path.lstat

    def fail_destination(path: Path) -> os.stat_result:
        if path == destination:
            raise OSError(PUBLICATION_FAILURE)
        return original_lstat(path)

    monkeypatch.setattr(Path, "lstat", fail_destination)
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_destination(destination)
    monkeypatch.undo()
    destination.write_bytes(_xml())
    junit_report._validate_destination(destination)

    def fail_parent(path: Path) -> os.stat_result:
        if path == destination.parent:
            raise OSError(PUBLICATION_FAILURE)
        return original_lstat(path)

    monkeypatch.setattr(Path, "lstat", fail_parent)
    with pytest.raises(junit_report.JunitReportError):
        junit_report._validate_destination(destination)


def test_statistics_collision_and_xml_revalidation_fail_closed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    first = "/known/tests/test_public.py::test_public"
    second = "PROJECT_ROOT/tests/test_public.py::test_public"

    def property_bytes(node_id: str) -> bytes:
        encoded = base64.b64encode(f"{node_id}:\npublic".encode()).decode()
        return (
            f"<property name='hypothesis-statistics-{node_id}' value='{encoded}'/>"
        ).encode()

    collision = (
        b"<testsuites><testsuite><properties>"
        + property_bytes(first)
        + property_bytes(second)
        + b"</properties><testcase/></testsuite></testsuites>"
    )
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(
            collision, (("/known", "PROJECT_ROOT"),), Path("public.xml")
        )
    parsed = ElementTree.fromstring(_xml())
    calls = iter((parsed, ElementTree.ParseError(PUBLICATION_FAILURE)))

    def reparse_failure(_value: str | bytes) -> object:
        result = next(calls)
        if isinstance(result, Exception):
            raise result
        return result

    monkeypatch.setattr("tools.junit_report.ElementTree.fromstring", reparse_failure)
    with pytest.raises(junit_report.JunitReportError):
        junit_report._sanitize(_xml(), (), Path("public.xml"))
