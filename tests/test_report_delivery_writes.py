"""Delivery accounts for bytes accepted by real pipes and nonblocking streams."""

from __future__ import annotations

import os
import threading
from io import BytesIO
from typing import override

import pytest

from eml_attachment_remover import report_delivery
from eml_attachment_remover.cancellation import DeliveryGuard
from tests.deadline_support import finite_operation


class _Partial(BytesIO):
    """A sink alternating no progress, partial acceptance, and blocked flushes."""

    calls = 0
    flushes = 0

    def __init__(self, expected: bytes) -> None:
        super().__init__()
        self.expected = expected

    @override
    def write(self, payload: object) -> int | None:  # type: ignore[override]
        view = memoryview(payload)  # type: ignore[arg-type]
        assert view.nbytes
        self.calls += 1
        if self.calls == 1:
            return None
        if self.calls == 2:
            return 0
        if self.calls == 3:
            raise BlockingIOError
        accepted = super().write(view[:3])
        assert self.tell() <= len(self.expected)
        assert self.getvalue() == self.expected[: self.tell()]
        if self.calls == 4:
            raise BlockingIOError(11, "partially accepted", accepted)
        return accepted

    @override
    def flush(self) -> None:
        self.flushes += 1
        if self.flushes == 1:
            raise BlockingIOError


def test_delivery_keeps_every_unaccepted_byte(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(report_delivery, "POLL_SECONDS", 0.001)
    payload = b"a complete report across short writes"
    destination = _Partial(payload)
    guard = DeliveryGuard()
    with finite_operation():
        report_delivery._copy_chunks(BytesIO(payload), destination, guard)  # ruff: ignore[private-member-access] - delivery contract.
    assert destination.getvalue() == payload
    assert destination.flushes == 2


@pytest.mark.parametrize("report", [False, True])
def test_progress_receipts_match_physically_accepted_output(
    monkeypatch: pytest.MonkeyPatch, *, report: bool
) -> None:
    monkeypatch.setattr(report_delivery, "POLL_SECONDS", 0.001)
    payload = b"public output across short writes"
    destination = _Partial(payload)
    guard = DeliveryGuard()
    with finite_operation():
        report_delivery._copy_chunks(  # ruff: ignore[private-member-access] - accepted-output receipt totals.
            BytesIO(payload), destination, guard, report=report
        )
    assert destination.getvalue() == payload
    assert guard.report_units == (len(destination.getvalue()) if report else 0)
    assert guard.diagnostic_units == (0 if report else len(destination.getvalue()))


@pytest.mark.parametrize("accepted", [-1, 4])
def test_delivery_rejects_impossible_counts(accepted: int) -> None:
    class Invalid(BytesIO):
        @override
        def write(self, _payload: object) -> int:
            return accepted

    with pytest.raises(
        OSError, match=r"^report channel returned an invalid write count$"
    ):
        report_delivery._copy_chunks(BytesIO(b"abc"), Invalid(), DeliveryGuard())  # ruff: ignore[private-member-access] - invalid channel contract.


def test_a_later_write_cannot_accept_more_than_the_remaining_payload() -> None:
    class Invalid(BytesIO):
        calls = 0

        @override
        def write(self, _payload: object) -> int:
            self.calls += 1
            return 3 if self.calls == 1 else 4

    with pytest.raises(OSError, match="invalid write count"):
        report_delivery._copy_chunks(BytesIO(b"abcdef"), Invalid(), DeliveryGuard())  # ruff: ignore[private-member-access] - shrinking unaccepted suffix.


@pytest.mark.skipif(os.name == "nt", reason="POSIX nonblocking pipe")
def test_real_nonblocking_raw_pipe_receives_the_entire_report() -> None:
    read_fd, write_fd = os.pipe()
    os.set_blocking(write_fd, False)
    received = bytearray()
    payload = bytes(range(256)) * 768

    def drain() -> None:
        with os.fdopen(read_fd, "rb", buffering=0) as source:
            while chunk := source.read(4096):
                received.extend(chunk)
                if len(received) > len(payload):
                    break

    reader = threading.Thread(target=drain)
    reader.start()
    try:
        with finite_operation(), os.fdopen(write_fd, "wb", buffering=0) as destination:
            report_delivery._copy_chunks(BytesIO(payload), destination, DeliveryGuard())  # ruff: ignore[private-member-access] - real raw output boundary.
    finally:
        reader.join(5)
    assert not reader.is_alive()
    assert bytes(received) == payload


@pytest.mark.parametrize(
    ("chunk", "newline"),
    [("notice\n", True), (b"notice\n", True), ("partial", False), (b"partial", False)],
)
def test_diagnostic_progress_preserves_text_and_binary_line_boundaries(
    chunk: str | bytes, *, newline: bool
) -> None:
    guard = DeliveryGuard()
    guard.accepted(chunk, report=False)
    assert guard.diagnostic_units == len(chunk)
    assert guard.diagnostic_newline is newline
    assert guard.report_units == 0
