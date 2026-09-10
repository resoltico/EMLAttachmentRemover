"""Exact lifecycle and Windows-adapter receipts for surviving mutation targets."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass

import pytest

from eml_attachment_remover import native_windows, staged_output
from eml_attachment_remover.domain import (
    BoundDestination,
    BoundDirectory,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_windows import WindowsApi


def _destination() -> BoundDestination:
    return BoundDestination(
        PathValue("request", "request", None, None),
        PathValue("parent", "parent", None, None),
        "out.eml",
        FileIdentity(7, 8, "directory", 901),
    )


def test_windows_adapter_reports_exact_unavailable_close_and_sync_diagnostics(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with monkeypatch.context() as context:
        context.setattr(native_windows.__dict__["os"], "name", "posix")
        with pytest.raises(
            OSError, match=r"^Windows handle operations are unavailable"
        ):
            WindowsApi()

    messages: list[str] = []

    @dataclass
    class Kernel:
        @staticmethod
        # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
        def CloseHandle(_value: object) -> int:
            return 0

        @staticmethod
        # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
        def FlushFileBuffers(_value: object) -> int:
            return 0

    api = object.__new__(WindowsApi)
    api.__dict__["kernel32"] = Kernel()

    def raise_last(message: str) -> None:
        messages.append(message)
        raise OSError(message)

    monkeypatch.setattr(WindowsApi, "_raise_last", staticmethod(raise_last))
    with pytest.raises(OSError, match=r"^could not close handle$"):
        api.close(0x1234)
    with pytest.raises(OSError, match=r"^could not sync destination directory$"):
        api.sync_directory(0x5678)
    assert messages == [
        "could not close handle",
        "could not sync destination directory",
    ]


def test_windows_adapter_preserves_explicit_child_boolean_intent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[tuple[int, str, bool, bool, bool]] = []

    def create_relative(
        _self: WindowsApi,
        parent: int,
        name: str,
        *,
        directory: bool,
        no_follow: bool,
        create: bool,
    ) -> int:
        calls.append((parent, name, directory, no_follow, create))
        return 44

    api = object.__new__(WindowsApi)
    monkeypatch.setattr(WindowsApi, "_create_relative", create_relative)
    assert api.open_child(10, "old", no_follow=True) == 44
    assert api.create_child(11, "new") == 44
    assert calls == [
        (10, "old", False, True, False),
        (11, "new", False, False, True),
    ]


def test_stage_construction_preserves_primary_without_synthetic_cleanup_group(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    state = staged_output._PublicationState(  # ruff: ignore[private-member-access] - direct lifecycle construction boundary.
        _destination(), b"body", hashlib.sha256(b"body").hexdigest()
    )
    state.parent = BoundDirectory(41, windows=False)
    primary = RuntimeError("construct")
    monkeypatch.setattr(staged_output, "private_stage_name", lambda: b"stage")
    monkeypatch.setattr(staged_output, "create_private_stage", lambda *_args: 42)
    monkeypatch.setattr(
        staged_output,
        "_Stage",
        lambda *_args: (_ for _ in ()).throw(primary),
    )
    monkeypatch.setattr(staged_output, "discard_private_stage", lambda *_args: None)
    monkeypatch.setattr(staged_output, "_close_descriptor", lambda _fd: None)

    with pytest.raises(RuntimeError) as captured:
        staged_output._create_stage(state)  # ruff: ignore[private-member-access] - bare construction error remains its own cause.
    assert captured.value is primary
