"""Exact user-safe native-path validation and display contracts."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_values
from eml_attachment_remover.domain import AppError, ExitCode

if TYPE_CHECKING:
    from collections.abc import Iterator


def _invalid_posix() -> Iterator[tuple[str, AppError]]:
    yield (
        "first/second/.",
        AppError(ExitCode.INPUT_ERROR, "path basename may not be . or .."),
    )
    yield (
        "first/second/..",
        AppError(ExitCode.INPUT_ERROR, "path basename may not be . or .."),
    )
    yield "first/second/", AppError(ExitCode.INPUT_ERROR, "path has an empty basename")
    yield (
        "first\x00second",
        AppError(ExitCode.INPUT_ERROR, "path has an empty basename"),
    )


def test_native_display_escapes_every_unsafe_unicode_category_exactly() -> None:
    """Public diagnostics retain visible text and escape every unsafe control form."""
    value = "A\x1f\u200e\ud800\u2028\u2029\u00a0Z"
    assert native_values._display(value) == (  # ruff: ignore[private-member-access] - public diagnostic rendering contract.
        "A\\u001f\\u200e\\ud800\\u2028\\u2029\u00a0Z"
    )


def test_posix_validation_preserves_full_terminal_component_and_errors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reject every unsafe terminal form with its stable input-error receipt."""
    native_values._validate_posix("first/second/leaf")  # ruff: ignore[private-member-access] - nested safe POSIX path.
    for value, expected in _invalid_posix():
        with pytest.raises(AppError) as captured:
            native_values._validate_posix(value)  # ruff: ignore[private-member-access] - native terminal-component contract.
        assert captured.value == expected
    with monkeypatch.context() as context:
        context.setattr(native_values, "MAX_PATH_BYTES", 4)
        native_values._validate_posix("four")  # ruff: ignore[private-member-access] - inclusive POSIX path budget.
        with pytest.raises(AppError) as captured:
            native_values._validate_posix("fives")  # ruff: ignore[private-member-access] - over-budget POSIX path.
        assert captured.value == AppError(
            ExitCode.INPUT_ERROR, "path exceeds the 32 KiB native-path limit"
        )


def test_windows_component_validation_rejects_reserved_and_ambiguous_names() -> None:
    """Reject every reserved DOS-name and trailing-character form consistently."""
    native_values._validate_windows_components(  # ruff: ignore[private-member-access] - ordinary Windows components.
        "safe\\file.eml"
    )
    for value, expected in (
        ("safe\\leaf.", "path has a trailing dot or space"),
        ("safe\\leaf ", "path has a trailing dot or space"),
        ("safe\\CON", "path contains a reserved DOS name"),
        ("safe\\com1.eml", "path contains a reserved DOS name"),
        ("safe/lpt9.data", "path contains a reserved DOS name"),
    ):
        with pytest.raises(AppError) as captured:
            native_values._validate_windows_components(value)  # ruff: ignore[private-member-access] - closed Windows component grammar.
        assert captured.value == AppError(ExitCode.INPUT_ERROR, expected)


def test_path_values_have_one_lossless_native_channel_per_platform(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Keep raw POSIX and UTF-16 Windows evidence mutually exclusive and exact."""
    value = "safe\nπ"
    posix = native_values.path_value(value)
    assert posix.text == value
    assert posix.display == "safe\\u000aπ"
    if os.name == "nt":
        assert posix.native_base64 is None
        assert posix.native_utf16le_base64 is not None
    else:
        assert posix.native_base64 is not None
        assert posix.native_utf16le_base64 is None

    monkeypatch.setattr(native_values.__dict__["os"], "name", "nt")
    windows = native_values.path_value(value)
    assert windows.text == value
    assert windows.display == "safe\\u000aπ"
    assert windows.native_base64 is None
    assert windows.native_utf16le_base64 is not None
