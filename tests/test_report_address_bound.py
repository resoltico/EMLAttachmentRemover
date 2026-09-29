"""A destination whose final address the report cannot carry is refused first."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    native_values,
    report_stream,
)
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import (
    AppError,
    ExitCode,
    ItemStatus,
)
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from pathlib import Path

HEADERS = (
    b"From: a@example.test\r\nMIME-Version: 1.0\r\n"
    b"Content-Type: multipart/mixed; boundary=B\r\n\r\n"
)
ATTACHMENT = (
    b"--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment\r\n"
    b"\r\nX\r\n--B--\r\n"
)
OPTIONS = BatchOptions(
    dry_run=False, existing="error", fail_fast=False, output=None, output_dir=None
)


def _parts(count: int) -> bytes:
    text = b"".join(
        b"--B\r\nContent-Type: text/plain\r\n\r\npart " + b"%d" % index + b"\r\n"
        for index in range(count)
    )
    return HEADERS + text + ATTACHMENT


@pytest.mark.parametrize("units", [1, 2])
def test_binding_refuses_a_destination_whose_final_address_cannot_be_reported(
    units: int,
) -> None:
    """The bound is checked on parent plus separator plus basename, inclusively."""
    parent = path_value("d" * 5)
    exact = native_values.address_units(parent) + 1
    limit = exact + 2  # a two-unit basename fills the address exactly
    with pytest.MonkeyPatch.context() as patch:
        patch.setattr(native_values, "MAX_ADDRESS_UNITS", limit)
        name: bytes | str = b"n" * 2 if units == 1 else "n" * 3
        if units == 1:
            native_values.require_reportable_destination(parent, name)
        else:
            with pytest.raises(AppError) as raised:
                native_values.require_reportable_destination(parent, name)
            assert raised.value == AppError(
                ExitCode.WRITE_ERROR, "destination address exceeds the reportable limit"
            )


def test_binding_refuses_an_unresolvable_destination_directory() -> None:
    """No proven parent address means the receipt could not be completed later."""
    with pytest.raises(AppError) as raised:
        native_values.require_reportable_destination(None, b"a.eml")
    assert raised.value == AppError(
        ExitCode.WRITE_ERROR, "could not resolve the destination directory address"
    )


def test_windows_basenames_are_measured_in_utf16_units() -> None:
    """A supplementary character costs two units of the address budget."""
    parent = path_value("d")
    limit = native_values.address_units(parent) + 1 + 2
    with pytest.MonkeyPatch.context() as patch:
        patch.setattr(native_values, "MAX_ADDRESS_UNITS", limit)
        native_values.require_reportable_destination(parent, "\U0001f600")
        with pytest.raises(AppError):
            native_values.require_reportable_destination(parent, "\U0001f600x")


def test_a_real_destination_beyond_the_address_bound_fails_before_any_copy(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Admitted exactly at the bound, refused one unit below it, never published."""
    source = tmp_path / "a.eml"
    source.write_bytes(_parts(1))
    normal = execute([str(source)], OPTIONS)
    try:
        receipt = normal.items[0].publication
        assert receipt is not None
        assert receipt.final_address is not None
        needed = native_values.address_units(receipt.final_address)
    finally:
        report_stream.close(normal)
    (tmp_path / "a.mime-pruned.eml").unlink()
    monkeypatch.setattr(native_values, "MAX_ADDRESS_UNITS", needed - 1)
    refused = execute([str(source)], OPTIONS)
    try:
        assert refused.items[0].status is ItemStatus.FAILED
        assert refused.items[0].error is not None
        assert refused.items[0].error.code is ExitCode.WRITE_ERROR
    finally:
        report_stream.close(refused)
    assert not (tmp_path / "a.mime-pruned.eml").exists()
    monkeypatch.setattr(native_values, "MAX_ADDRESS_UNITS", needed)
    admitted = execute([str(source)], OPTIONS)
    try:
        assert admitted.items[0].status is ItemStatus.CREATED
    finally:
        report_stream.close(admitted)
