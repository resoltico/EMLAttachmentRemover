"""Complete public receipts for native argument and destination values."""

from __future__ import annotations

import os
from base64 import b64encode
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_values
from eml_attachment_remover.domain import AppError, ExitCode, PathValue

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize(
    ("value", "message"),
    [
        ("safe\ud800", "path contains an unpaired surrogate"),
        (
            "\\\\?\\GLOBALROOT\\Device\\HarddiskVolume1",
            "extended path namespace is unsafe",
        ),
        ("", "path has an empty basename"),
        ("C:\\safe\\", "path has an empty basename"),
        (".", "path basename may not be . or .."),
        ("..", "path basename may not be . or .."),
        ("C:relative.eml", "drive-relative paths are unsafe"),
        ("C:\\safe\\mail.eml:metadata", "alternate data streams are unsafe"),
        ("C:\\safe\\leaf. ", "path has a trailing dot or space"),
        ("C:\\safe\\AUX.eml", "path contains a reserved DOS name"),
    ],
)
def test_windows_argument_validation_has_an_exact_receipt_per_unsafe_category(
    value: str, message: str
) -> None:
    """Keep each rejected Windows path form distinguishable to the caller."""
    with pytest.raises(AppError) as captured:
        native_values.validate_windows_argument(value)
    assert captured.value == AppError(ExitCode.INPUT_ERROR, message)


def test_windows_argument_validation_accepts_only_ordinary_and_file_namespaces() -> (
    None
):
    """Preserve drive, UNC, and documented extended-file path acceptance."""
    for value in (
        "C:\\safe\\mail.eml",
        "C:/safe/mail.eml",
        "\\\\server\\share\\mail.eml",
        "\\\\?\\C:\\safe\\mail.eml",
        "\\\\?\\UNC\\server\\share\\mail.eml",
    ):
        native_values.validate_windows_argument(value)


def test_windows_argument_validation_uses_an_inclusive_utf16_native_budget(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Measure Windows paths in native UTF-16LE bytes, including the boundary."""
    with monkeypatch.context() as context:
        context.setattr(native_values, "MAX_PATH_BYTES", 4)
        native_values.validate_windows_argument("xx")
        with pytest.raises(AppError) as captured:
            native_values.validate_windows_argument("xxx")
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "path exceeds the 32 KiB native-path limit"
    )


def test_default_destination_preserves_requested_expression_on_both_path_grammars(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Derive the v3 name without canonicalizing the user-requested parent."""
    if os.name != "nt":
        assert native_values.default_destination("parent/../inbox/Message.EML") == (
            "parent/../inbox/Message.mime-pruned.eml"
        )
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:\\in\\..\\Mail.EML") == (
            "C:\\in\\..\\Mail.mime-pruned.eml"
        )
        assert native_values.default_destination("C:\\in\\mail.emlx") == (
            "C:\\in\\mail.emlx.mime-pruned.eml"
        )


def test_path_value_windows_evidence_is_full_utf16_and_has_no_posix_channel(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Emit exactly one native evidence channel for a Windows argument."""
    source = "C:\\odd\nπ.eml"
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        value = native_values.path_value(source)
    assert value == PathValue(
        source,
        "C:\\odd\\u000aπ.eml",
        None,
        b64encode(source.encode("utf-16-le")).decode("ascii"),
    )


def test_validate_argument_expands_home_but_preserves_dotdot_expression(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """Expand a user home token without resolving traversal against the filesystem."""
    monkeypatch.setenv("HOME", os.fspath(tmp_path))
    monkeypatch.setenv("USERPROFILE", os.fspath(tmp_path))
    expected = os.fspath(tmp_path / "mail" / ".." / "message.eml")
    if os.name == "nt":
        with pytest.raises(AppError) as captured:
            native_values.validate_argument("~/mail/../message.eml")
        assert captured.value == AppError(
            ExitCode.INPUT_ERROR, "path has a trailing dot or space"
        )
    else:
        assert native_values.validate_argument("~/mail/../message.eml") == expected
