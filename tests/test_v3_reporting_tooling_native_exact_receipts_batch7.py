"""Exact native-value, cancellation, runtime, and archive-tool receipts."""

from __future__ import annotations

import platform
import signal
from typing import TYPE_CHECKING

import pytest
from tools import build_zipapp

from eml_attachment_remover import cancellation, native_values, runtime
from eml_attachment_remover.domain import AppError, ExitCode

if TYPE_CHECKING:
    from pathlib import Path


def test_posix_validation_rejects_a_trailing_separator_with_exact_receipt() -> None:
    """A POSIX path ending in a separator has no usable basename for native binding."""
    with pytest.raises(AppError) as raised:
        native_values._validate_posix("folder/")  # ruff: ignore[private-member-access] - trailing POSIX basename boundary.
    assert raised.value == AppError(ExitCode.INPUT_ERROR, "path has an empty basename")


@pytest.mark.parametrize("value", ["C:\\folder\\", "C:/folder/"])
def test_windows_validation_rejects_both_terminal_separator_spellings(
    value: str,
) -> None:
    """Windows separator grammar rejects either slash form after an ordinary name."""
    with pytest.raises(AppError) as raised:
        native_values.validate_windows_argument(value)
    assert raised.value == AppError(ExitCode.INPUT_ERROR, "path has an empty basename")


def test_default_destination_selects_the_active_path_grammar_exactly(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Native suffixing preserves a Windows parent expression."""
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:\\in\\Mail.EML") == (
            "C:\\in\\Mail.mime-pruned.eml"
        )
        assert native_values.default_destination("C:\\in\\Mail.txt") == (
            "C:\\in\\Mail.txt.mime-pruned.eml"
        )


def test_cancellation_receipts_keep_delivered_signal_number_and_stable_name() -> None:
    """Native signal delivery has exact known and unknown ledger-ready receipts."""
    number = int(signal.SIGTERM)
    with pytest.raises(cancellation.CancellationSignal) as raised:
        cancellation._raise_cancellation(number, None)  # ruff: ignore[private-member-access] - delivered signal conversion.
    assert (raised.value.number, raised.value.name, raised.value.args) == (
        number,
        "SIGTERM",
        (number, "SIGTERM"),
    )
    assert cancellation.cancellation_name(999) == "signal-999"


def test_runtime_unsupported_diagnostic_reports_the_actual_major_and_minor(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """The runtime guard exposes both observed interpreter version components."""
    monkeypatch.setattr(platform, "python_implementation", lambda: "PyPy")
    monkeypatch.setattr(runtime.__dict__["sys"], "version_info", (3, 15))
    assert runtime.main() == runtime.UNSUPPORTED_RUNTIME_STATUS
    assert capsys.readouterr().err == (
        "EML Attachment Remover requires CPython 3.14.x; found PyPy 3.15\n"
    )


def _metadata() -> build_zipapp.ProjectMetadata:
    """Build public stable metadata for direct archive-member inspection.

    Returns:
        A complete metadata record with no private project values.

    """
    return build_zipapp.ProjectMetadata(
        name="public-project",
        version="1.0.0",
        summary="public summary",
        requires_python=">=3.14,<3.15",
        license_expression="MIT",
        implementation="CPython",
    )


def test_zipapp_members_keep_the_schema_at_its_exact_public_archive_name() -> None:
    """The generated archive retains the report schema at its documented member path."""
    members = dict(build_zipapp._archive_members(_metadata()))  # ruff: ignore[private-member-access] - public archive layout.
    assert members["schema/report.schema.json"] == build_zipapp.SCHEMA_FILE.read_bytes()
    assert "LICENSE" in members
    assert "__main__.py" in members


def test_zipapp_member_build_names_missing_license_or_schema_exactly(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """Archive construction cannot advance without public legal/schema input."""
    missing = tmp_path / "missing"
    monkeypatch.setattr(build_zipapp, "LICENSE_FILE", missing)
    with pytest.raises(FileNotFoundError) as raised:
        build_zipapp._archive_members(_metadata())  # ruff: ignore[private-member-access] - required archive input.
    assert str(raised.value) == "zipapp license or schema file is unavailable"


def test_path_value_keeps_one_platform_native_evidence_channel(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A Windows path report uses UTF-16, never a POSIX byte, evidence channel."""
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        value = native_values.path_value("C:\\public\\mail.eml")
    assert value.native_base64 is None
    assert (
        value.native_utf16le_base64
        == "QwA6AFwAcAB1AGIAbABpAGMAXABtAGEAaQBsAC4AZQBtAGwA"
    )
