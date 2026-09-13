"""Exact receipts for behavior-distinct Windows native-path mutants."""

from __future__ import annotations

from dataclasses import dataclass, field

import pytest

from eml_attachment_remover import native_values, native_windows_binding
from eml_attachment_remover.domain import AppError, ExitCode


@dataclass
class _WindowsParentApi:
    """Record the root supplied when opening one Windows parent expression."""

    opened: list[tuple[str, int | None]] = field(default_factory=list)

    def open_directory(self, expression: str, root: int | None) -> int:
        """Retain the native addressing request and return one owned handle.

        Returns:
            A deterministic fixture handle for the opened directory.

        """
        self.opened.append((expression, root))
        return 41


def test_windows_default_destination_preserves_forward_slash_drive_grammar(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Windows suffixing keeps a requested slash spelling without mixed separators."""
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:/Note.EML") == (
            "C:/Note.mime-pruned.eml"
        )
        assert native_values.default_destination("C:/Note.txt") == (
            "C:/Note.txt.mime-pruned.eml"
        )


def test_windows_binding_opens_forward_slash_drive_parent_absolutely(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A rooted drive expression never resolves relative to captured startup CWD."""
    api = _WindowsParentApi()
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: api)
    monkeypatch.setattr(native_windows_binding, "_cwd", lambda: 77)

    assert native_windows_binding._parent("C:/Inbox/source.eml") == (  # ruff: ignore[private-member-access] - forward-slash drive paths are Windows-absolute at the handle boundary.
        41,
        "C:/Inbox",
        "source.eml",
    )
    assert api.opened == [("C:/Inbox", None)]


def test_windows_device_namespace_is_rejected_before_component_validation() -> None:
    """Win32 device objects cannot be used as ordinary handle-relative file inputs."""
    with pytest.raises(AppError) as raised:
        native_values.validate_windows_argument(r"\\.\pipe\x")
    assert raised.value == AppError(
        ExitCode.INPUT_ERROR, "device path namespace is unsafe"
    )
