"""Literal final-address native evidence contracts."""

from __future__ import annotations

import os
from types import SimpleNamespace

import pytest

from eml_attachment_remover import (
    native_binding,
    native_literal_address,
    native_windows_binding,
)
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExistingEntry,
    ExitCode,
    FileIdentity,
    PathValue,
)


def _identity() -> FileIdentity:
    return FileIdentity(11, 12, "regular", 13)


def _destination() -> BoundDestination:
    value = PathValue("out.eml", "out.eml", "b3V0LmVtbA==")
    return BoundDestination(value, value, b"out.eml", _identity())


def test_existing_read_round_trips_the_literal_final_address(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Existing verification accepts an address only after a literal reopen."""
    destination = _destination()
    directory = BoundDirectory(97, windows=False)
    identity = _identity()
    address = PathValue("/literal/out.eml", "literal", "L2xpdGVyYWwvb3V0LmVtbA==")
    calls: list[tuple[str, object]] = []
    closed: list[int] = []
    identities = iter((identity, identity, identity))

    def descriptor_identity(descriptor: int) -> FileIdentity:
        calls.append(("identity", descriptor))
        return next(identities)

    def read_all(descriptor: int) -> bytes:
        calls.append(("read", descriptor))
        return b"existing"

    def open_literal(observed: PathValue) -> int:
        calls.append(("literal", observed))
        return 99

    def close_directory(observed: BoundDirectory) -> None:
        closed.append(observed.descriptor)

    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(native_binding, "_open_child_nofollow", lambda *_values: 98)
    monkeypatch.setattr(native_binding, "_descriptor_identity", descriptor_identity)
    monkeypatch.setattr(native_binding, "_read_all", read_all)
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_values: identity)
    monkeypatch.setattr(native_binding, "_final_address", lambda _descriptor: address)
    monkeypatch.setattr(native_literal_address, "open_final_address", open_literal)
    monkeypatch.setattr(native_binding.__dict__["os"], "close", closed.append)
    monkeypatch.setattr(native_binding, "_close_bound_directory", close_directory)

    assert native_binding.read_existing(destination) == ExistingEntry(
        identity, b"existing", address
    )
    assert calls == [
        ("identity", 98),
        ("read", 98),
        ("identity", 98),
        ("literal", address),
        ("identity", 99),
        ("read", 99),
    ]
    assert closed == [99, 98, 97]


def test_existing_read_rejects_a_literal_address_for_a_different_object(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A handle-derived string is unusable when its literal reopen changes identity."""
    destination = _destination()
    directory = BoundDirectory(97, windows=False)
    original = _identity()
    changed = FileIdentity(11, 99, "regular", 13)
    address = PathValue("/literal/out.eml", "literal", "L2xpdGVyYWwvb3V0LmVtbA==")
    identities = iter((original, original, changed))
    closed: list[int] = []
    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(native_binding, "_open_child_nofollow", lambda *_values: 98)
    monkeypatch.setattr(
        native_binding, "_descriptor_identity", lambda _descriptor: next(identities)
    )
    monkeypatch.setattr(native_binding, "_read_all", lambda _descriptor: b"existing")
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_values: original)
    monkeypatch.setattr(native_binding, "_final_address", lambda _descriptor: address)
    monkeypatch.setattr(
        native_literal_address, "open_final_address", lambda _address: 99
    )
    monkeypatch.setattr(native_binding.__dict__["os"], "close", closed.append)
    monkeypatch.setattr(
        native_binding,
        "_close_bound_directory",
        lambda observed: closed.append(observed.descriptor),
    )

    with pytest.raises(AppError) as rejected:
        native_binding.read_existing(destination)
    assert rejected.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "final address did not resolve to verified output"
    )
    assert closed == [99, 98, 97]


def test_existing_read_rejects_a_literal_path_when_the_bound_entry_rebinds(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """An equal literal digest is insufficient when the bound entry changes."""
    destination = _destination()
    directory = BoundDirectory(97, windows=False)
    original = _identity()
    changed = FileIdentity(11, 13, "regular", 13)
    address = PathValue("/literal/out.eml", "literal", "L2xpdGVyYWwvb3V0LmVtbA==")
    identities = iter((original, original, original))
    entries = iter((original, changed))
    closed: list[int] = []
    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(native_binding, "_open_child_nofollow", lambda *_values: 98)
    monkeypatch.setattr(
        native_binding,
        "_descriptor_identity",
        lambda _descriptor: next(identities),
    )
    monkeypatch.setattr(native_binding, "_read_all", lambda _descriptor: b"existing")
    monkeypatch.setattr(native_binding, "_child_lstat", lambda *_values: next(entries))
    monkeypatch.setattr(native_binding, "_final_address", lambda _descriptor: address)
    monkeypatch.setattr(
        native_literal_address, "open_final_address", lambda _address: 99
    )
    monkeypatch.setattr(native_binding.__dict__["os"], "close", closed.append)
    monkeypatch.setattr(
        native_binding,
        "_close_bound_directory",
        lambda observed: closed.append(observed.descriptor),
    )
    with pytest.raises(AppError, match="final address did not resolve"):
        native_binding.read_existing(destination)
    assert closed == [99, 98, 97]


def test_literal_address_rejects_missing_text_and_dispatches_to_windows(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The literal dispatcher never invents text and delegates Windows paths."""
    missing = PathValue(None, "missing", None)
    with pytest.raises(OSError, match="no native text"):
        native_literal_address.open_final_address(missing)

    calls: list[str] = []

    def open_windows(address: str) -> int:
        calls.append(address)
        return 44

    with monkeypatch.context() as context:
        context.setattr(native_literal_address.__dict__["os"], "name", "nt")
        context.setattr(
            native_literal_address.__dict__["_windows"],
            "open_final_address",
            open_windows,
        )
        assert (
            native_literal_address.open_final_address(
                PathValue("C:\\mail.eml", "mail", None, "YwA=")
            )
            == 44
        )
    assert calls == ["C:\\mail.eml"]


def test_literal_address_dispatches_to_posix_without_host_platform_dependence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The POSIX literal boundary remains covered even on a Windows test host."""
    calls: list[tuple[bytes, int]] = []

    def open_posix(path: bytes, flags: int) -> int:
        calls.append((path, flags))
        return 45

    with monkeypatch.context() as context:
        module_os = native_literal_address.__dict__["os"]
        context.setattr(module_os, "name", "posix")
        context.setattr(module_os, "open", open_posix)
        assert (
            native_literal_address.open_final_address(
                PathValue("/mail.eml", "mail", None)
            )
            == 45
        )
    assert calls == [
        (
            b"/mail.eml",
            os.O_RDONLY
            | getattr(os, "O_NONBLOCK", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
        )
    ]


def test_windows_literal_address_reopens_parent_or_rejects_an_absent_basename(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Windows literal paths reopen through one no-follow parent/child receipt chain."""
    calls: list[tuple[str, object]] = []

    class Api:
        @staticmethod
        def open_child(parent: int, name: str, *, no_follow: bool) -> int:
            calls.append(("open", (parent, name, no_follow)))
            return 42

        @staticmethod
        def info(handle: int) -> SimpleNamespace:
            calls.append(("info", handle))
            return SimpleNamespace(directory=False, reparse=False)

        @staticmethod
        def descriptor_from_handle(handle: int, *, read_only: bool) -> int:
            calls.append(("descriptor", (handle, read_only)))
            return 43

        @staticmethod
        def close(handle: int) -> None:
            calls.append(("close", handle))

    def api() -> Api:
        return Api()

    parents: list[str] = []

    def parent(address: str) -> tuple[int, str, str]:
        parents.append(address)
        return 41, "parent", "out.eml"

    monkeypatch.setattr(
        native_windows_binding,
        "_parent",
        parent,
    )
    monkeypatch.setattr(native_windows_binding, "_api", api)
    assert native_windows_binding.open_final_address("C:\\out.eml") == 43
    assert calls == [
        ("open", (41, "out.eml", True)),
        ("info", 42),
        ("descriptor", (42, True)),
        ("close", 41),
    ]
    assert parents == ["C:\\out.eml"]
    with pytest.raises(OSError, match="no basename"):
        native_windows_binding._open_child_nofollow(  # ruff: ignore[private-member-access] - impossible literal receipt.
            BoundDirectory(1, windows=True)
        )
    with pytest.raises(
        OSError, match=r"^Windows final address has no basename$"
    ) as missing:
        native_windows_binding._open_child_nofollow(  # ruff: ignore[private-member-access] - exact missing-basename diagnostic.
            BoundDirectory(1, windows=True)
        )
    assert str(missing.value) == "Windows final address has no basename"
