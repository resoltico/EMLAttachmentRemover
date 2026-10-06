"""Independent literal final-receipt evidence contracts."""

from __future__ import annotations

import hashlib
from types import SimpleNamespace

import pytest

from eml_attachment_remover import staged_receipt
from eml_attachment_remover.domain import (
    AppError,
    BoundDestination,
    FileIdentity,
    PathValue,
)
from eml_attachment_remover.native_paths import BoundDirectoryHandle


@pytest.mark.parametrize(
    ("identity", "payload", "entry", "expected_result"),
    [
        (
            FileIdentity(1, 2, "regular", 3),
            b"candidate",
            FileIdentity(1, 2, "regular", 3),
            True,
        ),
        (
            FileIdentity(1, 2, "directory", 3),
            b"candidate",
            FileIdentity(1, 2, "regular", 3),
            False,
        ),
        (
            FileIdentity(1, 4, "regular", 3),
            b"candidate",
            FileIdentity(1, 2, "regular", 3),
            False,
        ),
        (
            FileIdentity(1, 2, "regular", 3),
            b"changed",
            FileIdentity(1, 2, "regular", 3),
            False,
        ),
        (
            FileIdentity(1, 2, "regular", 3),
            b"candidate",
            FileIdentity(1, 4, "regular", 3),
            False,
        ),
    ],
)
def test_literal_match_requires_every_evidence_fact(
    identity: FileIdentity,
    payload: bytes,
    entry: FileIdentity,
    *,
    expected_result: bool,
) -> None:
    """No literal path can be accepted after any one evidence fact changes."""
    expected = FileIdentity(1, 2, "regular", 3)
    value = PathValue("out", "out", "b3V0")
    destination = BoundDestination(
        value, value, b"out", FileIdentity(1, 2, "directory", 3)
    )
    operations = staged_receipt.ReceiptOperations(
        descriptor_identity=lambda _fd: identity,
        child_lstat=lambda *_args: entry,
        open_child_nofollow=lambda *_args: 0,
        final_address=lambda _fd: value,
        open_final_address=lambda _address: 0,
        read_all=lambda _fd: payload,
        close_descriptor=lambda _fd: None,
    )
    context = staged_receipt._ReceiptContext(  # ruff: ignore[private-member-access] - exact literal evidence context.
        BoundDirectoryHandle(7, windows=False),
        destination,
        expected,
        hashlib.sha256(b"candidate").hexdigest(),
        operations,
    )
    assert staged_receipt._literal_matches(9, context) is expected_result  # ruff: ignore[private-member-access] - literal evidence conjunction.


def test_combined_cleanup_receipts_preserve_single_and_multiple_errors() -> None:
    """Cleanup aggregation returns the sole original or a group with every member."""
    first = OSError("first")
    second = OSError("second")
    assert staged_receipt._combined([first]) is first  # ruff: ignore[private-member-access] - single cleanup identity.
    group = staged_receipt._combined([first, second])  # ruff: ignore[private-member-access] - multiple cleanup identity.
    assert isinstance(group, BaseExceptionGroup)
    assert group.message == "multiple staging cleanup failures"
    assert group.exceptions == (first, second)


def test_literal_failure_closes_the_exact_opened_descriptor() -> None:
    """A failed literal proof never closes a sentinel instead of its live handle."""
    value = PathValue("out", "out", "b3V0")
    expected = FileIdentity(1, 2, "regular", 3)
    destination = BoundDestination(
        value, value, b"out", FileIdentity(1, 2, "directory", 3)
    )
    closed: list[int] = []

    def close(descriptor: int) -> None:
        closed.append(descriptor)

    operations = staged_receipt.ReceiptOperations(
        descriptor_identity=lambda _fd: FileIdentity(1, 4, "regular", 3),
        child_lstat=lambda *_args: expected,
        open_child_nofollow=lambda *_args: 0,
        final_address=lambda _fd: value,
        open_final_address=lambda _address: 27,
        read_all=lambda _fd: b"candidate",
        close_descriptor=close,
    )
    context = staged_receipt._ReceiptContext(  # ruff: ignore[private-member-access] - exact close target context.
        BoundDirectoryHandle(7, windows=False),
        destination,
        expected,
        hashlib.sha256(b"candidate").hexdigest(),
        operations,
    )
    with pytest.raises(AppError, match="did not resolve"):
        staged_receipt._verify_literal_address(value, context)  # ruff: ignore[private-member-access] - failed literal proof.
    assert closed == [27]


def test_literal_open_failure_closes_the_unopened_descriptor_sentinel() -> None:
    """A native open failure is paired with the documented unopened sentinel."""
    value = PathValue("out", "out", "b3V0")
    expected = FileIdentity(1, 2, "regular", 3)
    destination = BoundDestination(
        value, value, b"out", FileIdentity(1, 2, "directory", 3)
    )
    closed: list[int] = []

    def close(descriptor: int) -> None:
        closed.append(descriptor)

    operations = staged_receipt.ReceiptOperations(
        descriptor_identity=lambda _fd: expected,
        child_lstat=lambda *_args: expected,
        open_child_nofollow=lambda *_args: 0,
        final_address=lambda _fd: value,
        open_final_address=lambda _address: (_ for _ in ()).throw(OSError("open")),
        read_all=lambda _fd: b"candidate",
        close_descriptor=close,
    )
    context = staged_receipt._ReceiptContext(  # ruff: ignore[private-member-access] - unopened literal context.
        BoundDirectoryHandle(7, windows=False),
        destination,
        expected,
        hashlib.sha256(b"candidate").hexdigest(),
        operations,
    )
    with pytest.raises(OSError, match="open"):
        staged_receipt._verify_literal_address(value, context)  # ruff: ignore[private-member-access] - native-open failure.
    assert closed == [-1]


def test_final_receipt_closes_a_zero_descriptor_after_successful_proof() -> None:
    """Descriptor zero is owned and must be closed just like every positive fd."""
    value = PathValue("out", "out", "b3V0")
    expected = FileIdentity(1, 2, "regular", 3)
    destination = BoundDestination(
        value, value, b"out", FileIdentity(1, 2, "directory", 3)
    )
    state = SimpleNamespace(
        destination=destination,
        digest=hashlib.sha256(b"candidate").hexdigest(),
        parent=BoundDirectoryHandle(7, windows=False),
        stage=SimpleNamespace(descriptor=0),
    )
    closed: list[int] = []

    def close(descriptor: int) -> None:
        closed.append(descriptor)

    operations = staged_receipt.ReceiptOperations(
        descriptor_identity=lambda _fd: expected,
        child_lstat=lambda *_args: expected,
        open_child_nofollow=lambda *_args: 0,
        final_address=lambda _fd: value,
        open_final_address=lambda _address: 1,
        read_all=lambda _fd: b"candidate",
        close_descriptor=close,
    )
    receipt = staged_receipt.read_final_receipt(state, operations)
    assert receipt.final_address == value
    assert closed == [1, 0]
