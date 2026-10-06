"""Public JUnit evidence must redact synthetic absolute pytest node IDs."""

from __future__ import annotations

from typing import TYPE_CHECKING

from tools import junit_report

if TYPE_CHECKING:
    from pathlib import Path


def test_junit_publisher_redacts_escaped_windows_path_in_pytest_node_id(
    tmp_path: Path,
) -> None:
    source = tmp_path / "private.xml"
    destination = tmp_path / "public.xml"
    source.write_text(
        "<testsuites><testsuite name='suite'><testcase "
        "name='windows[C:\\\\safe\\\\final.]' /></testsuite></testsuites>",
        encoding="utf-8",
    )
    junit_report.publish(source, destination, tmp_path)
    public = destination.read_text(encoding="utf-8")
    assert "redacted-absolute-node-id-" in public
    assert "C:\\" not in public
