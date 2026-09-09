"""Cross-platform binding dispatch and exact native-name contracts."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import native_binding
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    FileIdentity,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from collections.abc import Callable


def _identity() -> FileIdentity:
    return FileIdentity(1, 2, "regular", 3)


@pytest.mark.parametrize(
    ("file_type", "expected"),
    [("regular", True), ("-rw-------", True), ("directory", False)],
)
def test_file_identity_recognizes_portable_regular_file_labels(
    file_type: str, *, expected: bool
) -> None:
    """Require final-entry proof to accept both platform regular-file labels."""
    assert FileIdentity(0, 0, file_type, 0).is_regular() is expected


def _destination() -> BoundDestination:
    return BoundDestination(
        path_value("out.eml"), path_value("."), b"out.eml", _identity()
    )


def _object(value: object) -> object:
    return value


def _module_value(name: str) -> object:
    return cast("object", native_binding.__dict__[name])


def test_cross_platform_dispatch_selects_the_exact_bound_backend(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[str] = []

    def fake(label: str) -> Callable[..., str]:
        def invoke(*_arguments: object, **_keywords: object) -> str:
            calls.append(label)
            return label

        return invoke

    methods = (
        "bind_destination",
        "inspect_source_identity",
        "read_source",
        "open_bound_destination",
        "close_bound_directory",
        "descriptor_identity",
        "create_private_stage",
        "publish_stage_no_replace",
        "child_lstat",
        "open_child_nofollow",
        "discard_private_stage",
        "sync_bound_directory",
    )
    for backend, label in (
        (native_binding.__dict__["_posix"], "posix"),
        (native_binding.__dict__["_windows"], "windows"),
    ):
        for method in methods:
            monkeypatch.setattr(backend, method, fake(f"{label}:{method}"))
    monkeypatch.setattr(
        native_binding, "validate_argument", lambda value: f"expanded:{value}"
    )

    for platform, windows in (("posix", False), ("nt", True)):
        monkeypatch.setattr(_module_value("os"), "name", platform)
        directory = BoundDirectory(9, windows=windows)
        name = "stage" if windows else b"stage"
        destination = _destination()
        prefix = "windows" if windows else "posix"
        assert (
            _object(native_binding.bind_destination("request"))
            == f"{prefix}:bind_destination"
        )
        assert (
            _object(native_binding.inspect_source_identity("request"))
            == f"{prefix}:inspect_source_identity"
        )
        assert _object(native_binding.read_source("request")) == f"{prefix}:read_source"
        assert (
            _object(native_binding.open_bound_destination(destination))
            == f"{prefix}:open_bound_destination"
        )
        native_binding.close_bound_directory(directory)
        assert calls[-1] == f"{prefix}:close_bound_directory"
        assert (
            _object(native_binding.descriptor_identity(3))
            == f"{prefix}:descriptor_identity"
        )
        assert (
            _object(native_binding.create_private_stage(directory, name))
            == f"{prefix}:create_private_stage"
        )
        assert (
            _object(native_binding.publish_stage_no_replace(directory, 4, name, name))
            == f"{prefix}:publish_stage_no_replace"
        )
        assert (
            _object(native_binding.child_lstat(directory, name))
            == f"{prefix}:child_lstat"
        )
        assert (
            _object(native_binding.open_child_nofollow(directory, name))
            == f"{prefix}:open_child_nofollow"
        )
        native_binding.discard_private_stage(directory, 4, name)
        assert calls[-1] == f"{prefix}:discard_private_stage"
        assert (
            _object(native_binding.sync_bound_directory(directory))
            == f"{prefix}:sync_bound_directory"
        )
    assert len(calls) == 24


def test_native_name_and_stage_helpers_are_platform_exact(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(_module_value("secrets"), "token_hex", lambda _count: "token")
    monkeypatch.setattr(_module_value("os"), "name", "posix")
    assert native_binding.private_stage_name() == b".eml-remove-token.tmp"
    assert native_binding._byte_name(b"bytes") == b"bytes"  # ruff: ignore[private-member-access] - POSIX name boundary.
    with pytest.raises(TypeError):
        native_binding._byte_name("text")  # ruff: ignore[private-member-access] - POSIX name boundary.
    monkeypatch.setattr(_module_value("os"), "name", "nt")
    assert native_binding.private_stage_name() == ".eml-remove-token.tmp"
    assert native_binding._unicode_name("text") == "text"  # ruff: ignore[private-member-access] - Windows name boundary.
    with pytest.raises(TypeError):
        native_binding._unicode_name(b"bytes")  # ruff: ignore[private-member-access] - Windows name boundary.


def test_existing_entry_handles_missing_changes_and_cleanup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    destination = _destination()
    directory = BoundDirectory(9, windows=False)
    closed: list[int] = []
    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(
        native_binding,
        "_close_bound_directory",
        lambda value: closed.append(value.descriptor),
    )
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_args: _identity())
    assert native_binding.existing_identity(destination) == _identity()

    def absent(*_args: object) -> int:
        raise FileNotFoundError

    monkeypatch.setattr(native_binding, "_open_child_nofollow", absent)
    assert native_binding.read_existing(destination) is None
    assert closed == [9, 9]

    monkeypatch.setattr(native_binding, "_open_child_nofollow", lambda *_args: 7)
    monkeypatch.setattr(native_binding, "_descriptor_identity", lambda _fd: _identity())
    monkeypatch.setattr(native_binding, "_read_all", lambda _fd: b"candidate")
    monkeypatch.setattr(_module_value("os"), "close", _record_close(closed))
    existing = native_binding.read_existing(destination)
    assert existing is not None
    assert existing.raw == b"candidate"
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_args: None)
    with pytest.raises(AppError):
        native_binding.read_existing(destination)
    assert 7 in closed


def test_read_all_rejects_an_existing_file_that_exceeds_the_limit(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    complete = iter((b"complete", b""))
    monkeypatch.setattr(_module_value("os"), "read", lambda *_args: next(complete))
    assert native_binding._read_all(3) == b"complete"  # ruff: ignore[private-member-access] - existing-output read contract.
    reads = iter((b"first", b"second"))
    monkeypatch.setattr(_module_value("os"), "read", lambda *_args: next(reads))
    monkeypatch.setattr(native_binding, "MAX_RAW_BYTES", 5)
    with pytest.raises(AppError):
        native_binding._read_all(3)  # ruff: ignore[private-member-access] - existing-output size bound.


def _record_close(closed: list[int]) -> Callable[[int], None]:
    def close(descriptor: int) -> None:
        closed.append(descriptor)

    return close
