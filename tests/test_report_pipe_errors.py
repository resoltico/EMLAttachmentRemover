"""Windows pipe normalization preserves unrelated channel and staging errors."""

from __future__ import annotations

import errno
import os
from io import StringIO
from types import SimpleNamespace
from typing import TYPE_CHECKING, override

import pytest

from eml_attachment_remover import report_delivery
from eml_attachment_remover.cancellation import DeliveryGuard

if TYPE_CHECKING:
    from pathlib import Path


class _FailingChannel(StringIO):
    """Inject an output failure while retaining a real endpoint descriptor."""

    def __init__(self, descriptor: int, operation: str, failure: OSError) -> None:
        super().__init__()
        self.descriptor = descriptor
        self.operation = operation
        self.failure = failure

    @override
    def fileno(self) -> int:
        return self.descriptor

    @override
    def write(self, value: str, /) -> int:
        if self.operation == "write":
            raise self.failure
        return super().write(value)

    @override
    def flush(self) -> None:
        if self.operation == "flush":
            raise self.failure
        super().flush()


@pytest.mark.parametrize("operation", ["write", "flush"])
@pytest.mark.parametrize(
    "case",
    [
        ("win32", "pipe", errno.EINVAL, True),
        ("linux", "pipe", errno.EINVAL, False),
        ("win32", "file", errno.EINVAL, False),
        ("win32", "invalid", errno.EINVAL, False),
        ("win32", "pipe", errno.EPERM, False),
        ("win32", "pipe", errno.EPIPE, False),
    ],
)
def test_output_errors_identify_windows_pipes_before_normalizing(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    operation: str,
    case: tuple[str, str, int, bool],
) -> None:
    platform, endpoint, number, normalized = case
    read_descriptor, write_descriptor = os.pipe()
    try:
        with (tmp_path / "output").open("wb") as regular:
            descriptors = {
                "pipe": write_descriptor,
                "file": regular.fileno(),
                "invalid": -1,
            }
            failure = OSError(number, "injected channel failure")
            channel = _FailingChannel(descriptors[endpoint], operation, failure)
            monkeypatch.setattr(
                report_delivery,
                "sys",
                SimpleNamespace(platform=platform, stderr=channel),
            )
            with pytest.raises(
                OSError, match=r"pipe reader closed|injected channel failure"
            ) as raised:
                report_delivery.write_note("diagnostic\n", DeliveryGuard())
            if normalized:
                assert isinstance(raised.value, BrokenPipeError)
                assert raised.value.errno == errno.EPIPE
                assert str(raised.value) == "[Errno 32] report pipe reader closed"
                assert raised.value.__cause__ is failure
            else:
                assert raised.value is failure
    finally:
        os.close(read_descriptor)
        os.close(write_descriptor)
