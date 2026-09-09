"""Focused native-path contracts for the v3 descriptor-bound path layer."""

from __future__ import annotations

import os
from base64 import b64decode
from types import SimpleNamespace
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path

from eml_attachment_remover import native_values
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.native_paths import (
    default_destination,
    read_source,
    validate_windows_argument,
)


def test_default_destination_replaces_only_a_terminal_eml_suffix() -> None:
    """Derived names retain their requested parent and use the v3 suffix."""
    separator = "\\" if os.name == "nt" else "/"
    assert default_destination("folder/Message.EML") == (
        f"folder{separator}Message.mime-pruned.eml"
    )
    assert default_destination("folder/message") == (
        f"folder{separator}message.mime-pruned.eml"
    )


def test_source_binding_keeps_kernel_symlink_dotdot_semantics(tmp_path: Path) -> None:
    """A literal intermediate-link/.. expression opens the kernel-selected file."""
    if os.name == "nt":
        source = tmp_path / "source.eml"
        source.write_bytes(b"ordinary-windows-source")
        snapshot = read_source(str(source))
        assert snapshot.raw == b"ordinary-windows-source"
        return
    left = tmp_path / "left"
    target = tmp_path / "target"
    nested = target / "nested"
    left.mkdir()
    nested.mkdir(parents=True)
    (left / "source.eml").write_bytes(b"left")
    selected = target / "source.eml"
    selected.write_bytes(b"target")
    (left / "link").symlink_to(nested, target_is_directory=True)
    request = str(left / "link" / ".." / "source.eml")
    snapshot = read_source(request)
    assert snapshot.raw == b"target"
    assert snapshot.request.text == request


@pytest.mark.parametrize(
    "path",
    [
        "C:relative.eml",
        "C:\\safe\\final. ",
        "C:\\safe\\COM1.eml",
        "C:\\safe\\mail.eml:stream",
        "C:\\safe\\broken\ud800.eml",
        "\\\\?\\GLOBALROOT\\Device\\HarddiskVolume1",
        "\\\\?\\Volume{00000000-0000-0000-0000-000000000000}\\mail.eml",
        "\\\\?\\pipe\\mail",
    ],
)
def test_windows_native_validation_rejects_unsafe_forms(
    path: str,
) -> None:
    """The Windows validator rejects unsafe forms before any native API invocation."""
    with pytest.raises(AppError):
        validate_windows_argument(path)


@pytest.mark.parametrize(
    "path",
    ["\\\\?\\C:\\long\\mail.eml", "\\\\?\\UNC\\server\\share\\mail.eml"],
)
def test_windows_native_validation_keeps_supported_extended_file_namespaces(
    path: str,
) -> None:
    """Permit only the documented drive and UNC extended-length forms."""
    validate_windows_argument(path)


def test_native_value_paths_cover_posix_and_handle_backend_boundaries(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    value = native_values.path_value("odd\nname.eml")
    assert (value.native_base64 is not None) is (os.name != "nt")
    assert "\\u000a" in value.display
    for unsafe in ("", ".", ".."):
        with pytest.raises(AppError):
            native_values.validate_argument(unsafe)
    monkeypatch.setattr(native_values.__dict__["os"], "name", "nt")
    assert (
        native_values.default_destination("C:\\in\\Mail.EML")
        == "C:\\in\\Mail.mime-pruned.eml"
    )

    class AvailableApi:
        def __init__(self) -> None:
            pass

    monkeypatch.setitem(native_values.__dict__, "WindowsApi", AvailableApi)
    native_values.require_native_backend()

    class MissingApi:
        def __init__(self) -> None:
            message = "missing"
            raise OSError(message)

    monkeypatch.setitem(native_values.__dict__, "WindowsApi", MissingApi)
    with pytest.raises(AppError):
        native_values.require_native_backend()


def test_path_value_and_native_length_limits_are_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Require lossless native evidence and exact inclusive path budgets."""
    value = native_values.path_value("odd\nπ")
    if os.name == "nt":
        assert value.native_utf16le_base64 is not None
        assert b64decode(value.native_utf16le_base64).decode("utf-16-le") == "odd\nπ"
    else:
        assert value.native_base64 is not None
        assert b64decode(value.native_base64) == os.fsencode("odd\nπ")
    assert value.display == "odd\\u000aπ"
    with monkeypatch.context() as context:
        context.setattr(native_values, "MAX_PATH_BYTES", 4)
        native_values._validate_posix("abcd")  # ruff: ignore[private-member-access] - exact POSIX byte ceiling.
        with pytest.raises(AppError) as captured:
            native_values._validate_posix("abcde")  # ruff: ignore[private-member-access] - over-limit POSIX byte ceiling.
        assert captured.value == AppError(
            ExitCode.INPUT_ERROR, "path exceeds the 32 KiB native-path limit"
        )
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        context.setattr(native_values, "MAX_PATH_BYTES", 8)
        windows = native_values.path_value("C:\\x")
        assert windows.native_utf16le_base64 is not None
        assert b64decode(windows.native_utf16le_base64).decode("utf-16-le") == "C:\\x"
        native_values.validate_windows_argument("C:\\x")
        with pytest.raises(AppError):
            native_values.validate_windows_argument("C:\\xy")


def test_native_value_validation_covers_platform_specific_boundary_forms(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    runtime_name = os.name
    observed: list[str] = []
    monkeypatch.setattr(native_values.__dict__["os"], "name", "nt")
    windows_value = native_values.path_value("C:\\value")
    assert windows_value.native_base64 is None
    assert windows_value.native_utf16le_base64 is not None
    monkeypatch.setattr(native_values, "validate_windows_argument", observed.append)
    assert native_values.validate_argument("value") == "value"
    assert observed == ["value"]

    monkeypatch.undo()
    for invalid in ("", "."):
        with pytest.raises(AppError):
            native_values.validate_windows_argument(invalid)
    monkeypatch.setattr(native_values, "MAX_PATH_BYTES", 1)
    with pytest.raises(AppError):
        native_values.validate_windows_argument("x")

    monkeypatch.setattr(native_values.__dict__["os"], "name", "posix")
    native_values.require_native_backend()
    with pytest.raises(AppError):
        native_values._validate_posix("")  # ruff: ignore[private-member-access] - POSIX native component boundary.
    monkeypatch.setattr(native_values, "MAX_PATH_BYTES", 1)
    with pytest.raises(AppError):
        native_values._validate_posix("xx")  # ruff: ignore[private-member-access] - POSIX native component boundary.
    native_values._validate_windows_components(  # ruff: ignore[private-member-access] - Windows component empty-segment boundary.
        ""
    )
    monkeypatch.setattr(native_values.__dict__["os"], "name", runtime_name)
    with monkeypatch.context() as context:
        context.setattr(
            native_values,
            "os",
            SimpleNamespace(
                name="posix",
                fsencode=os.fsencode,
                fspath=os.fspath,
                path=os.path,
            ),
        )
        context.setattr(native_values, "MAX_PATH_BYTES", 32 * 1024)
        assert native_values.validate_argument("ordinary") == "ordinary"
        assert native_values.default_destination("parent/message.eml")
    with pytest.raises(AppError):
        native_values._validate_posix(".")  # ruff: ignore[private-member-access] - POSIX dot-basename boundary.
