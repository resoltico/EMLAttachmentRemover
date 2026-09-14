"""Corrective v3.0.1 native argument and existing-entry contracts."""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    mime_validation,
    native_binding,
    native_posix,
    native_values,
)
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    BoundDirectory,
    ExitCode,
    FileIdentity,
    PathValue,
)

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.skipif(os.name == "nt", reason="POSIX FIFO descriptor contract")
def test_existing_verify_rejects_a_fifo_before_any_blocking_read(
    tmp_path: Path,
) -> None:
    """A named pipe is occupied but can never be an existing verified output."""
    fifo = tmp_path / "candidate.mime-pruned.eml"
    os.mkfifo(fifo)
    destination = native_posix.bind_destination(os.fspath(fifo), os.fspath(fifo))

    with pytest.raises(AppError) as rejected:
        native_binding.read_existing(destination)

    assert rejected.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination is not a regular file"
    )


@pytest.mark.skipif(os.name == "nt", reason="POSIX raw argv spelling contract")
@pytest.mark.parametrize(
    ("value", "message"),
    [
        ("mail.eml/", "path has an empty basename"),
        ("mail.eml//", "path has an empty basename"),
        ("mail.eml/.", "path basename may not be . or .."),
        ("mail.eml/..", "path basename may not be . or .."),
    ],
)
def test_raw_file_argument_rejects_terminal_directory_semantics(
    value: str, message: str
) -> None:
    """Validation precedes expansion so a file request cannot shed its suffix."""
    with pytest.raises(AppError) as rejected:
        native_values.validate_argument(value)
    assert rejected.value == AppError(ExitCode.INPUT_ERROR, message)


@pytest.mark.skipif(os.name == "nt", reason="POSIX descriptor cleanup contract")
def test_nofollow_open_closes_a_descriptor_if_its_metadata_read_fails(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A failed regular-file proof must not leak the opened descriptor."""
    closed: list[int] = []
    monkeypatch.setattr(native_posix.__dict__["os"], "open", lambda *_args, **_kw: 63)
    monkeypatch.setattr(
        native_posix.__dict__["os"],
        "fstat",
        lambda _descriptor: (_ for _ in ()).throw(OSError("metadata")),
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closed.append)

    with pytest.raises(OSError, match=r"^metadata$"):
        native_posix._open_child_nofollow(  # ruff: ignore[private-member-access] - native regularity proof.
            BoundDirectory(17, windows=False), b"out"
        )

    assert closed == [63]


def test_existing_output_failures_are_conflicts_and_nonregular_entries_are_rejected(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Verification treats expected native read faults as one item-level conflict."""
    destination = BoundDestination(
        PathValue("out", "out", None),
        PathValue(".", ".", None),
        b"out",
        FileIdentity(1, 2, "directory", 3),
    )
    directory = BoundDirectory(73, windows=False)
    monkeypatch.setattr(
        native_binding,
        "_open_bound_destination",
        lambda _destination: (_ for _ in ()).throw(OSError("parent")),
    )
    with pytest.raises(AppError) as opening:
        native_binding.read_existing(destination)
    assert opening.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "could not read existing output: parent"
    )

    closed: list[int] = []
    monkeypatch.setattr(
        native_binding, "_open_bound_destination", lambda _value: directory
    )
    monkeypatch.setattr(
        native_binding,
        "_close_bound_directory",
        lambda value: closed.append(value.descriptor),
    )
    monkeypatch.setattr(
        native_binding,
        "_read_existing_from_directory",
        lambda *_args: (_ for _ in ()).throw(OSError("child")),
    )
    with pytest.raises(AppError) as reading:
        native_binding.read_existing(destination)
    assert reading.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "could not read existing output: child"
    )
    assert closed == [73]

    monkeypatch.setattr(
        native_binding,
        "_descriptor_identity",
        lambda _descriptor: FileIdentity(1, 2, "fifo", 3),
    )
    with pytest.raises(AppError) as nonregular:
        native_binding._existing_entry(  # ruff: ignore[private-member-access] - defense-in-depth regularity proof.
            directory, destination, 74
        )
    assert nonregular.value == AppError(
        ExitCode.OUTPUT_CONFLICT, "destination is not a regular file"
    )


def test_final_address_dispatch_uses_the_active_native_backend(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A final address is descriptor evidence, not a destination request alias."""
    value = PathValue("final", "final", "ZmluYWw=")
    monkeypatch.setattr(native_binding.__dict__["os"], "name", "nt")
    windows_descriptors: list[int] = []

    def windows_final_address(descriptor: int) -> PathValue:
        windows_descriptors.append(descriptor)
        return value

    monkeypatch.setattr(
        native_binding.__dict__["_windows"],
        "final_address",
        windows_final_address,
    )
    assert native_binding.final_address(9) == value
    assert windows_descriptors == [9]
    monkeypatch.setattr(native_binding.__dict__["os"], "name", "posix")
    monkeypatch.setattr(
        native_binding.__dict__["_posix"], "final_address", lambda _fd: None
    )
    assert native_binding.final_address(9) is None


@pytest.mark.parametrize(
    "name",
    [b"filename*" + (b"9" * 5_000), b"filename*512", b"filename*512*"],
)
def test_rfc2231_continuation_index_is_bounded_before_integer_conversion(
    name: bytes,
) -> None:
    """An untrusted continuation label is a typed parse failure, never an abort."""
    with pytest.raises(AppError) as rejected:
        mime_validation._parameter_name(  # ruff: ignore[private-member-access] - finite RFC 2231 index grammar.
            name
        )
    assert rejected.value.code is ExitCode.PARSE_ERROR
