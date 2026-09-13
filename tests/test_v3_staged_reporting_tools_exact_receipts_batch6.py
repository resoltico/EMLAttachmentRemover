"""Exact parser and distribution-tool receipts for surviving mutation boundaries."""

from __future__ import annotations

import base64
from pathlib import Path

import pytest
from tools import (
    hatch_build,
    hypothesis_observation_safety,
    junit_report,
    smoke_distribution,
)

from eml_attachment_remover import cli_parser
from eml_attachment_remover.domain import AppError, ExitCode


def test_raw_parser_tracks_final_value_options_and_removed_short_spellings() -> None:
    """Preparse scanning keeps the final option value and rejects exact v2 spellings."""
    arguments = ["source.eml", "--output-format", "json"]
    assert cli_parser._consumed_option_values(  # ruff: ignore[private-member-access] - final value ownership.
        arguments
    ) == {2}
    assert cli_parser.raw_json_requested(arguments)

    with pytest.raises(AppError) as force:
        cli_parser.validate_raw_arguments(["-f", "source.eml"])
    assert force.value == AppError(ExitCode.USAGE, cli_parser.MIGRATION_EXISTING)

    with pytest.raises(AppError) as paths:
        cli_parser.validate_raw_arguments(["--output-format=paths", "source.eml"])
    assert paths.value == AppError(ExitCode.USAGE, cli_parser.MIGRATION_PATHS)


def test_raw_parser_preserves_both_json_forms_and_end_of_option_boundaries() -> None:
    """JSON selection is exact before, but never after, the literal-source marker."""
    assert cli_parser.raw_json_requested(["--output-format=json", "source.eml"])
    assert cli_parser.raw_json_requested(["--output-format", "json", "source.eml"])
    assert not cli_parser.raw_json_requested(["--", "--output-format=json"])
    assert cli_parser.raw_source_candidates([
        "--output-format",
        "json",
        "source.eml",
    ]) == ["source.eml"]


def test_public_statistics_passes_exact_labels_and_source_to_the_sanitizer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Encoded statistics distinguish property-name and body sanitization evidence."""
    source = tmp_path / "statistics.xml"
    encoded = base64.b64encode(b"case:\npublic body").decode("ascii")
    calls: list[tuple[str, Path, str, bool]] = []

    def public_text(
        value: str,
        _replacements: tuple[tuple[str, str], ...],
        observed_source: Path,
        label: str,
        *,
        pytest_node_id: bool = False,
    ) -> str:
        calls.append((value, observed_source, label, pytest_node_id))
        return value

    monkeypatch.setattr(junit_report, "_public_text", public_text)
    node_id, public = junit_report._public_statistics(  # ruff: ignore[private-member-access] - statistics privacy boundary.
        encoded, "case", (), source
    )
    assert node_id == "case"
    assert base64.b64decode(public) == b"case:\npublic body"
    assert calls == [
        ("case", source, "Hypothesis statistics property name", True),
        ("public body", source, "encoded Hypothesis statistics", False),
    ]


def test_junit_node_id_path_check_collapses_doubled_backslashes_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Node IDs normalize escaped separators before the absolute-path safety check."""
    source = tmp_path / "report.xml"
    checked: list[str] = []
    monkeypatch.setattr(
        hypothesis_observation_safety, "public_text", lambda value, _replacements: value
    )
    monkeypatch.setattr(
        hypothesis_observation_safety,
        "private_prefix_remains",
        lambda _value, _replacements: False,
    )
    monkeypatch.setattr(
        junit_report.policy, "public_content_messages", lambda _value: ()
    )
    monkeypatch.setattr(junit_report.node_id, "redact", lambda value: value)

    def contains_absolute_path(value: str) -> bool:
        checked.append(value)
        return False

    monkeypatch.setattr(
        hypothesis_observation_safety,
        "contains_absolute_path",
        contains_absolute_path,
    )

    assert (
        junit_report._public_text(  # ruff: ignore[private-member-access] - escaped-node path safety.
            r"case\\name", (), source, "node", pytest_node_id=True
        )
        == r"case\\name"
    )
    assert checked == [r"case\name"]


def test_junit_publication_names_parent_creation_failure_exactly(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Publication cannot conceal the parent directory and its concrete OS failure."""
    source = tmp_path / "private.xml"
    destination = tmp_path / "build" / "report.xml"
    source.write_bytes(
        b"<testsuites><testsuite><testcase name='public'/></testsuite></testsuites>"
    )
    failure = OSError("read-only")

    def mkdir(_path: Path, **_keywords: object) -> None:
        raise failure

    monkeypatch.setattr(Path, "mkdir", mkdir)
    with pytest.raises(junit_report.JunitReportError) as raised:
        junit_report.publish(source, destination, tmp_path)
    assert str(raised.value) == (
        f"cannot prepare JUnit report directory {destination.parent}: read-only"
    )
    assert raised.value.__cause__ is failure


def _project(path: Path, *, requires: str, implementation: str = "CPython") -> Path:
    """Write minimal canonical wheel-tag metadata.

    Returns:
        The configuration path consumed by the Hatch hook.

    """
    config = path / "pyproject.toml"
    config.write_text(
        "\n".join((
            "[project]",
            f'requires-python = "{requires}"',
            "[tool.eml-attachment-remover.runtime]",
            f'implementation = "{implementation}"',
        )),
        encoding="utf-8",
    )
    return config


def test_wheel_tag_rejects_non_cpython_and_multiple_minor_bounds_exactly(
    tmp_path: Path,
) -> None:
    """The build hook gives each unsafe metadata shape a stable public message."""
    for requires, implementation, message in (
        (
            ">=3.14",
            "CPython",
            "wheel tag requires exact CPython major/minor project bounds",
        ),
        (
            ">=3.14,<3.15",
            "PyPy",
            "wheel tag requires exact CPython major/minor project bounds",
        ),
        (
            ">=3.14,<3.16",
            "CPython",
            "wheel tag requires one supported CPython minor version",
        ),
    ):
        with pytest.raises(ValueError, match=message) as raised:
            hatch_build.wheel_tag(
                _project(tmp_path, requires=requires, implementation=implementation)
            )
        assert str(raised.value) == message


def _semantic_message(*, attachment: bool, resource: bool) -> bytes:
    """Build a minimal parser-clean output with one selectable forbidden semantic.

    Returns:
        A serialized multipart EML with exactly the expected two leaf types.

    """
    extra_headers = b""
    if attachment:
        extra_headers += b"Content-Disposition: attachment\r\n"
    if resource:
        extra_headers += b"Content-ID: <public-image@example.test>\r\n"
    return (
        b"Subject: Installed distribution smoke test\r\n"
        b"Content-Type: multipart/alternative; boundary=public\r\n\r\n"
        b"--public\r\nContent-Type: text/plain\r\n\r\nPublic body\r\n"
        b"--public\r\nContent-Type: text/html\r\n"
        + extra_headers
        + b"\r\n<html><body>Public HTML</body></html>\r\n--public--\r\n"
    )


@pytest.mark.parametrize(("attachment", "resource"), [(True, False), (False, True)])
def test_smoke_verification_rejects_each_remaining_forbidden_semantic(
    tmp_path: Path, *, attachment: bool, resource: bool
) -> None:
    """A retained attachment or related Content-ID cannot pass artifact smoke proof."""
    output = _semantic_message(attachment=attachment, resource=resource)
    source = tmp_path / "source.eml"
    destination = tmp_path / "output.eml"
    source.write_bytes(output)
    destination.write_bytes(output)

    with pytest.raises(RuntimeError) as raised:
        smoke_distribution._verify_output(  # ruff: ignore[private-member-access] - installed artifact semantic gate.
            source, destination, output
        )
    assert (
        str(raised.value)
        == "installed command did not produce the expected MIME-pruned EML"
    )
