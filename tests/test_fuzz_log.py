"""Verify bounded diagnostics preserve failures and do not truncate silently."""

from __future__ import annotations

import io
import re
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING, override

import pytest
from tools import fuzz_log

if TYPE_CHECKING:
    from collections.abc import Buffer


@pytest.mark.parametrize(
    "content",
    [b"", b"diagnostic", b"x" * 65536],
    ids=["empty", "diagnostic", "exact-chunk"],
)
def test_log_preserves_content_through_exact_limit(
    tmp_path: Path, content: bytes
) -> None:
    destination = tmp_path / "stage.log"
    fuzz_log.write_log(io.BytesIO(content), destination, max(1, len(content)))
    assert destination.read_bytes() == content


def test_log_overflow_fails_with_only_bounded_prefix(tmp_path: Path) -> None:
    destination = tmp_path / "stage.log"
    message = f"fuzz diagnostics exceed 16 bytes: {destination}"
    with pytest.raises(ValueError, match=f"^{re.escape(message)}$"):
        fuzz_log.write_log(io.BytesIO(b"a" * 17), destination, 16)
    assert destination.read_bytes() == b"a" * 16


@pytest.mark.parametrize("limit", [0, -1])
def test_invalid_log_limit_is_rejected_before_creating_a_file(
    tmp_path: Path,
    limit: int,
) -> None:
    destination = tmp_path / "stage.log"
    with pytest.raises(ValueError, match=r"^fuzz log byte limit must be positive$"):
        fuzz_log.write_log(io.BytesIO(b"diagnostic"), destination, limit)
    assert not destination.exists()


def test_existing_diagnostics_are_not_overwritten(tmp_path: Path) -> None:
    destination = tmp_path / "stage.log"
    destination.write_bytes(b"previous failure")
    with pytest.raises(FileExistsError):
        fuzz_log.write_log(io.BytesIO(b"replacement"), destination, 16)
    assert destination.read_bytes() == b"previous failure"


def test_multichunk_log_has_one_exact_budget_and_one_overflow_sentinel(
    tmp_path: Path,
) -> None:
    class ObservedSource(io.BytesIO):
        requests: list[int | None]

        def __init__(self, content: bytes) -> None:
            super().__init__(content)
            self.requests = []

        def read(self, size: int | None = -1) -> bytes:
            self.requests.append(size)
            return super().read(size)

    limit = 65537
    destination = tmp_path / "stage.log"
    source = ObservedSource(b"x" * (limit + 100))
    with pytest.raises(ValueError, match="exceed 65537 bytes"):
        fuzz_log.write_log(source, destination, limit)
    assert destination.read_bytes() == b"x" * limit
    assert source.tell() == limit + 1
    assert source.requests == [65536, 2]


def test_source_error_is_propagated_and_received_diagnostics_survive(
    tmp_path: Path,
) -> None:
    class InterruptedSource(io.BytesIO):
        def read(self, size: int | None = -1) -> bytes:
            if self.tell():
                message = "diagnostic source failed"
                raise OSError(message)
            return super().read(size)

    destination = tmp_path / "stage.log"
    with pytest.raises(OSError, match=r"^diagnostic source failed$"):
        fuzz_log.write_log(InterruptedSource(b"prefix"), destination, 16)
    assert destination.read_bytes() == b"prefix"


@pytest.mark.parametrize("write_error", [False, True])
def test_partial_raw_writes_are_completed_and_flushed_or_fail(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    write_error: bool,
) -> None:
    class ShortSink(io.RawIOBase):
        def __init__(self) -> None:
            super().__init__()
            self.contents = bytearray()

        @override
        def writable(self) -> bool:
            return True

        def write(self, data: Buffer, /) -> int:
            if write_error and self.contents:
                message = "diagnostic destination failed"
                raise OSError(message)
            payload = memoryview(data)
            count = max(1, len(payload) // 2)
            self.contents.extend(payload[:count])
            return count

    sink = ShortSink()
    content = b"x" * 65537

    class VisibleSource(io.BytesIO):
        def read(self, size: int | None = -1) -> bytes:
            # A later read must see the preceding chunk in the underlying file,
            # so live diagnostics remain available before the stage finishes.
            assert sink.contents == content[: self.tell()]
            return super().read(size)

    def open_sink(
        _path: Path,
        mode: str,
        *,
        buffering: int = -1,
    ) -> ShortSink | io.BufferedWriter:
        assert mode == "xb"
        return sink if buffering == 0 else io.BufferedWriter(sink)

    with monkeypatch.context() as fault:
        fault.setattr(Path, "open", open_sink)
        if write_error:
            with pytest.raises(OSError, match=r"^diagnostic destination failed$"):
                fuzz_log.write_log(
                    VisibleSource(content), tmp_path / "stage.log", len(content)
                )
            assert 0 < len(sink.contents) < len(content)
        else:
            fuzz_log.write_log(
                VisibleSource(content), tmp_path / "stage.log", len(content)
            )
            assert sink.contents == content


@pytest.mark.parametrize("overflow", [False, True], ids=("exact-limit", "overflow"))
def test_cli_preserves_binary_stdin_and_enforces_the_log_limit(
    tmp_path: Path, *, overflow: bool
) -> None:
    destination = tmp_path / "stage.log"
    expected = b"diagnostic\r\n\x00\xff"
    result = subprocess.run(
        [sys.executable, "-B", fuzz_log.__file__, str(destination), str(len(expected))],
        input=expected + (b"excess" if overflow else b""),
        capture_output=True,
        check=False,
        timeout=5,
    )
    assert result.returncode == (1 if overflow else 0)
    assert destination.read_bytes() == expected
    assert result.stdout == b""
    if overflow:
        assert (
            f"fuzz diagnostics exceed {len(expected)} bytes".encode() in result.stderr
        )
    else:
        assert result.stderr == b""


@pytest.mark.skipif(sys.platform == "win32", reason="requires POSIX shell pipeline")
def test_pipeline_propagates_logging_and_command_failures(tmp_path: Path) -> None:
    logger = Path(fuzz_log.__file__).resolve()
    pipeline = '"$1" -c "$2" 2>&1 | "$1" -B "$3" "$4" 16'
    for index, program in enumerate([
        "print('x' * 17)",
        "print('diagnostic'); raise SystemExit(7)",
    ]):
        destination = tmp_path / f"stage-{index}.log"
        result = subprocess.run(
            [
                "/bin/bash",
                "-o",
                "pipefail",
                "-c",
                pipeline,
                "fuzz-stage",
                sys.executable,
                program,
                str(logger),
                str(destination),
            ],
            capture_output=True,
            check=False,
            timeout=5,
        )
        assert result.returncode != 0
        assert destination.stat().st_size <= 16
