"""Exact mutation receipts for native values and cross-platform binding dispatch."""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import native_binding, native_values, native_windows_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExistingEntry,
    ExitCode,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_windows import WindowsFileInfo

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch


def _identity() -> FileIdentity:
    return FileIdentity(7, 8, "regular", 901)


@dataclass
class WindowsBackend:
    """Trace the opaque Windows-handle conversions used by the binding facade."""

    handled_descriptors: list[int | None] = field(default_factory=list)
    discarded: list[int] = field(default_factory=list)
    published: list[tuple[int, int, str]] = field(default_factory=list)
    opened: list[tuple[str | None, int | None]] = field(default_factory=list)

    def handle_from_descriptor(self, descriptor: int | None) -> int:
        self.handled_descriptors.append(descriptor)
        return 170

    @staticmethod
    def info(_handle: int) -> WindowsFileInfo:
        return WindowsFileInfo(
            volume_serial=7,
            file_id=8,
            change_time=901,
            last_write_time=902,
            size=3,
            directory=False,
            reparse=False,
        )

    def discard_private_stage(self, handle: int) -> None:
        self.discarded.append(handle)

    def publish_no_replace(self, stage: int, parent: int, name: str) -> None:
        self.published.append((stage, parent, name))

    def open_directory(self, path: str | None, root: int | None) -> int:
        self.opened.append((path, root))
        return 5


def _install_windows_backend(monkeypatch: MonkeyPatch, backend: WindowsBackend) -> None:
    monkeypatch.setitem(native_windows_binding.__dict__, "_api", lambda: backend)


def _destination() -> BoundDestination:
    return BoundDestination(
        PathValue("request", "request", None, None),
        PathValue("parent", "parent", None, None),
        "out.eml",
        FileIdentity(7, 9, "directory", 809),
    )


def test_native_values_reject_trailing_paths_and_reserved_multi_suffixes() -> None:
    with pytest.raises(AppError) as posix_error:
        native_values._validate_posix("one/two/")  # ruff: ignore[private-member-access] - trailing separator means no target basename.
    assert posix_error.value == AppError(
        ExitCode.INPUT_ERROR, "path has an empty basename"
    )

    with pytest.raises(AppError) as reserved_error:
        native_values._validate_windows_components("folder\\CON.one.two")  # ruff: ignore[private-member-access] - DOS reservation depends on the first suffix boundary.
    assert reserved_error.value == AppError(
        ExitCode.INPUT_ERROR, "path contains a reserved DOS name"
    )


def test_native_values_preserve_platform_path_evidence_and_backend_failure_context(
    monkeypatch: MonkeyPatch,
) -> None:
    if os.name != "nt":
        with monkeypatch.context() as context:
            context.setattr(native_values.__dict__["os"], "name", "posix")
            assert native_values.path_value("π.eml") == PathValue(
                "π.eml", "π.eml", "z4AuZW1s"
            )
            assert native_values.default_destination("folder/Mail.EML") == (
                "folder/Mail.mime-pruned.eml"
            )

    failure = OSError("unavailable")
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        context.setattr(
            native_values,
            "WindowsApi",
            lambda: (_ for _ in ()).throw(failure),
        )
        with pytest.raises(AppError) as captured:
            native_values.require_native_backend()
    assert captured.value == AppError(
        ExitCode.WRITE_ERROR, "Windows native handle backend is unavailable"
    )
    assert captured.value.__cause__ is failure


def test_native_binding_name_type_rejections_and_private_stage_entropy_are_exact(
    monkeypatch: MonkeyPatch,
) -> None:
    with pytest.raises(TypeError, match=r"^POSIX native name must be bytes$"):
        native_binding._byte_name("name")  # ruff: ignore[private-member-access] - POSIX API names have byte identity.
    with pytest.raises(TypeError, match=r"^Windows native name must be Unicode$"):
        native_binding._unicode_name(b"name")  # ruff: ignore[private-member-access] - Windows API names have Unicode identity.
    assert native_binding._byte_name(b"name") == b"name"  # ruff: ignore[private-member-access] - bytes pass through unchanged.
    assert native_binding._unicode_name("name") == "name"  # ruff: ignore[private-member-access] - text passes through unchanged.

    calls: list[int] = []

    def token_hex(size: int) -> str:
        calls.append(size)
        return "a" * (size * 2)

    monkeypatch.setattr(native_binding.__dict__["secrets"], "token_hex", token_hex)
    with monkeypatch.context() as context:
        context.setattr(native_binding.__dict__["os"], "name", "posix")
        # ruff: ignore[private-member-access] - private stage has exactly 128 bits of entropy.
        assert (
            native_binding._private_stage_name()
            == b".eml-remove-" + b"a" * 32 + b".tmp"
        )
    assert calls == [16]


def test_native_binding_existing_read_closes_descriptor_zero_and_never_closes_sentinel(
    monkeypatch: MonkeyPatch,
) -> None:
    directory = BoundDirectory(41, windows=False)
    closed: list[int] = []
    directory_closed: list[BoundDirectory] = []
    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(native_binding, "_open_child_nofollow", lambda *_args: 0)
    monkeypatch.setattr(native_binding, "_descriptor_identity", lambda _fd: _identity())
    monkeypatch.setattr(native_binding, "_read_all", lambda _fd: b"body")
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_args: _identity())
    monkeypatch.setattr(
        native_binding, "_close_bound_directory", directory_closed.append
    )
    monkeypatch.setattr(native_binding.__dict__["os"], "close", closed.append)

    assert native_binding._read_existing(_destination()) == ExistingEntry(  # ruff: ignore[private-member-access] - existing output is read through descriptor zero too.
        _identity(), b"body"
    )
    assert closed == [0]
    assert directory_closed == [directory]

    closed.clear()
    directory_closed.clear()
    monkeypatch.setattr(
        native_binding,
        "_open_child_nofollow",
        lambda *_args: (_ for _ in ()).throw(FileNotFoundError()),
    )
    assert native_binding._read_existing(_destination()) is None  # ruff: ignore[private-member-access] - missing existing output owns no descriptor.
    assert closed == []
    assert directory_closed == [directory]


def test_native_binding_reads_one_mebibyte_and_dispatches_windows_descriptor_identity(
    monkeypatch: MonkeyPatch,
) -> None:
    requests: list[int] = []

    def empty_read(_descriptor: int, size: int) -> bytes:
        requests.append(size)
        return b""

    monkeypatch.setattr(native_binding, "MAX_RAW_BYTES", 2 * 1024 * 1024)
    monkeypatch.setattr(native_binding.__dict__["os"], "read", empty_read)
    assert native_binding._read_all(0) == b""  # ruff: ignore[private-member-access] - bounded native read request.
    assert requests == [1024 * 1024]

    calls: list[int] = []

    def descriptor_identity(descriptor: int) -> FileIdentity:
        calls.append(descriptor)
        return _identity()

    with monkeypatch.context() as context:
        context.setattr(native_binding.__dict__["os"], "name", "nt")
        context.setattr(
            native_binding.__dict__["_windows"],
            "descriptor_identity",
            descriptor_identity,
        )
        assert native_binding._descriptor_identity(73) == _identity()  # ruff: ignore[private-member-access] - Windows descriptor dispatch preserves the descriptor.
    assert calls == [73]


def test_windows_binding_converts_the_actual_stage_descriptor_for_all_operations(
    monkeypatch: MonkeyPatch,
) -> None:
    backend = WindowsBackend()
    _install_windows_backend(monkeypatch, backend)
    directory = BoundDirectory(31, windows=True)

    assert native_windows_binding._descriptor_identity(61) == _identity()  # ruff: ignore[private-member-access] - descriptor identity uses the supplied CRT descriptor.
    native_windows_binding._discard_private_stage(directory, 62, "stage")  # ruff: ignore[private-member-access] - private-stage discard converts the supplied CRT descriptor.
    assert (
        native_windows_binding._publish_stage_no_replace(  # ruff: ignore[private-member-access] - no-replace publication converts the supplied CRT descriptor.
            directory, 63, "stage", "out.eml"
        )
        is False
    )
    assert backend.handled_descriptors == [61, 62, 63]
    assert backend.discarded == [170]
    assert backend.published == [(170, 31, "out.eml")]


def test_windows_binding_captures_current_directory_and_exact_nonregular_diagnostic(
    monkeypatch: MonkeyPatch,
) -> None:
    backend = WindowsBackend()
    _install_windows_backend(monkeypatch, backend)
    current = os.fspath(Path.cwd())
    with monkeypatch.context() as context:
        context.setattr(native_windows_binding.__dict__["os"], "name", "nt")
        assert native_windows_binding._capture_start_cwd() == 5  # ruff: ignore[private-member-access] - Windows startup directory must be native-addressed.
    assert backend.opened == [(current, None)]

    backend_info = WindowsFileInfo(
        volume_serial=7,
        file_id=8,
        change_time=901,
        last_write_time=902,
        size=3,
        directory=False,
        reparse=True,
    )
    monkeypatch.setattr(backend, "info", lambda _handle: backend_info)
    with pytest.raises(AppError) as captured:
        native_windows_binding._require_regular(170)  # ruff: ignore[private-member-access] - reparse destinations are never ordinary output files.
    assert captured.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination is not a regular file"
    )
