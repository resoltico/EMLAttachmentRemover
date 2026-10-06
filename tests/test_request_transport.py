"""Native request framing, admission limits and real pipe-based CLI integration."""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import request_transport
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.request_transport import decode

if TYPE_CHECKING:
    from pathlib import Path


def _payload(paths: list[str]) -> bytes:
    encoding = "windows-utf16le" if os.name == "nt" else "posix-bytes"
    native = [
        path.encode("utf-16-le", "surrogatepass")
        if os.name == "nt"
        else os.fsencode(path)
        for path in paths
    ]
    return json.dumps({
        "encoding": encoding,
        "paths": [base64.b64encode(path).decode("ascii") for path in native],
    }).encode("utf-8")


def _frame(payload: bytes) -> bytes:
    return len(payload).to_bytes(4, "big") + payload


def test_native_paths_round_trip_without_option_interpretation() -> None:
    """Path order, duplicates and unusual Unicode are data, never CLI flags."""
    paths = ["--output=other", "a b.eml", "line\n.eml", "π-😀.eml", "a b.eml"]
    assert decode(_payload(paths)) == paths
    if os.name != "nt":
        path = os.fsdecode(b"raw-\xff.eml")
        assert os.fsencode(decode(_payload([path]))[0]) == b"raw-\xff.eml"


@pytest.mark.parametrize(
    ("payload", "message"),
    [
        (b"", "request frame exceeds its byte limit"),
        (b"not JSON", "request is not valid UTF-8 JSON"),
        (b"\xff", "request is not valid UTF-8 JSON"),
        (b"{}", "invalid native-path request fields"),
        (b"[]", "invalid native-path request fields"),
        (b'{"encoding":0,"paths":[]}', "invalid native-path request fields"),
        (
            b'{"encoding":"wrong","paths":["YQ=="]}',
            "invalid native-path request fields",
        ),
        (
            b'{"encoding":"posix-bytes","encoding":"posix-bytes","paths":["YQ=="]}',
            "request is not valid UTF-8 JSON",
        ),
    ],
)
def test_invalid_request_objects_are_typed_usage_errors(
    payload: bytes, message: str
) -> None:
    """Malformed selection failures retain their actionable public explanation."""
    with pytest.raises(AppError) as failure:
        decode(payload)
    assert failure.value.code is ExitCode.USAGE
    assert failure.value.message == message


@pytest.mark.parametrize(
    ("paths", "message"),
    [
        ([], "request exceeds its input-count limit"),
        ([""], "request path is empty or contains NUL"),
        (["\0"], "request path is empty or contains NUL"),
        (["x"] * 4097, "request exceeds its input-count limit"),
        (["x" * (4 * 1024 * 1024 + 1)], "request exceeds its path-byte limit"),
    ],
)
def test_request_limits_and_invalid_addresses_are_refused(
    paths: list[str], message: str
) -> None:
    """Selection bounds and address failures name the specific remedy before work."""
    with pytest.raises(AppError) as failure:
        decode(_payload(paths))
    assert failure.value.code is ExitCode.USAGE
    assert failure.value.message == message


def _run(frame: bytes, *, arguments: tuple[str, ...] = ()) -> tuple[int, bytes, bytes]:
    command = [
        sys.executable,
        "-B",
        "-m",
        "eml_attachment_remover",
        "--request-stdin",
        "--output-format=json",
        *arguments,
    ]
    with (
        subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ) as process,
        ThreadPoolExecutor(max_workers=2) as readers,
    ):
        assert process.stdin is not None
        assert process.stdout is not None
        assert process.stderr is not None
        output = readers.submit(process.stdout.read)
        errors = readers.submit(process.stderr.read)
        try:
            process.stdin.write(frame)
            process.stdin.flush()
            status = process.wait(timeout=20)
            return status, output.result(timeout=2), errors.result(timeout=2)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)


def test_one_byte_json_frame_reports_invalid_fields_instead_of_a_size_error() -> None:
    status, output, errors = _run(_frame(b"0"))
    assert status == 2, errors
    report = json.loads(output)
    assert report["batch_error"]["code"] == "USAGE"
    assert report["batch_error"]["message"] == "invalid native-path request fields"
    assert report["items"] == []


def test_framed_cli_creates_the_same_copy_without_named_request_storage(
    tmp_path: Path,
) -> None:
    """A held-open pipe retains the ordinary real-CLI report contract."""
    source = tmp_path / "space -- message.eml"
    raw = b"Content-Type: text/plain\r\n\r\nretained\r\n"
    source.write_bytes(raw)
    status, output, errors = _run(_frame(_payload([str(source)])))
    assert status == 0, errors
    report = json.loads(output)
    assert report["schema_version"] == 3
    assert report["items"][0]["status"] == "created"
    assert source.with_suffix(".mime-pruned.eml").read_bytes() == raw
    assert source.read_bytes() == raw
    assert sorted(path.name for path in tmp_path.iterdir()) == [
        "space -- message.eml",
        "space -- message.mime-pruned.eml",
    ]


def test_maximum_batch_count_crosses_no_native_argument_limit(tmp_path: Path) -> None:
    """All 4,096 requests reach the backend through a two-option process launch."""
    paths = [str(tmp_path / f"missing-{index}.eml") for index in range(4096)]
    status, output, errors = _run(_frame(_payload(paths)))
    assert status == 9, errors
    report = json.loads(output)
    assert len(report["items"]) == 4096
    assert report["summary"]["failed"] == 4096
    assert all(item["error"]["code"] == "INPUT_ERROR" for item in report["items"])


def test_oversized_header_is_refused_without_reading_a_payload() -> None:
    """A hostile length cannot make the receiver allocate or wait for its body."""
    status, output, errors = _run((0xFFFF_FFFF).to_bytes(4, "big"))
    assert status == 2, errors
    report = json.loads(output)
    assert report["batch_error"]["code"] == "USAGE"
    assert report["items"] == []


def test_framed_folders_reach_real_processing_as_unique_leaf_files(
    tmp_path: Path,
) -> None:
    """Folder expansion completes before the ordinary source pipeline creates copies."""
    nested = tmp_path / "nested"
    nested.mkdir()
    first = tmp_path / "first.eml"
    second = nested / "second.EML"
    raw = b"Content-Type: text/plain\r\n\r\nretained\r\n"
    first.write_bytes(raw)
    second.write_bytes(raw)
    ignored = tmp_path / "ignored.mime-pruned.eml"
    ignored.write_bytes(b"existing generated copy")
    status, output, errors = _run(
        _frame(_payload([str(tmp_path), str(nested), str(first)]))
    )
    assert status == 0, errors
    report = json.loads(output)
    assert report["summary"]["total"] == 2
    assert report["summary"]["created"] == 2
    assert first.with_suffix(".mime-pruned.eml").read_bytes() == raw
    assert second.with_suffix(".mime-pruned.eml").read_bytes() == raw
    assert first.read_bytes() == raw
    assert second.read_bytes() == raw
    assert ignored.read_bytes() == b"existing generated copy"
    assert not (tmp_path / "ignored.mime-pruned.mime-pruned.eml").exists()


def test_frame_byte_budget_accepts_equality_and_refuses_the_next_byte(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """The frame budget includes equality and rejects the next byte."""
    payload = _payload(["public.eml"])
    monkeypatch.setattr(request_transport, "MAX_FRAME_BYTES", len(payload))
    assert decode(payload) == ["public.eml"]
    with pytest.raises(AppError) as failure:
        decode(payload + b" ")
    assert failure.value.message == "request frame exceeds its byte limit"


def test_request_path_budget_includes_exact_native_byte_total(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    paths = ["first.eml", "second.eml"]
    monkeypatch.setattr(
        request_transport,
        "MAX_CUMULATIVE_REQUEST_PATH_BYTES",
        sum(len(os.fsencode(path)) for path in paths),
    )
    assert decode(_payload(paths)) == paths
    with pytest.raises(AppError) as caught:
        decode(_payload([*paths, "x"]))
    assert caught.value.code is ExitCode.USAGE
    assert caught.value.message == "request exceeds its path-byte limit"


def test_windows_request_encoding_is_checked_on_every_host(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(os, "name", "nt")
    path = "C:\\Inbox\\π-😀.eml"
    assert decode(_payload([path])) == [path]


@pytest.mark.parametrize("failure", [b"", OSError("synthetic pipe failure")])
def test_request_read_failure_keeps_typed_diagnostic(
    monkeypatch: pytest.MonkeyPatch,
    failure: bytes | OSError,
) -> None:
    def read(descriptor: int, size: int) -> bytes:
        assert descriptor == 199
        assert size == 4
        if isinstance(failure, OSError):
            raise failure
        return failure

    monkeypatch.setattr(os, "read", read)
    with pytest.raises(AppError) as caught:
        request_transport.read(199)
    assert caught.value.code is ExitCode.USAGE
    assert caught.value.message == (
        "could not read request pipe"
        if isinstance(failure, OSError)
        else "request pipe ended before a complete frame"
    )


def test_fragmented_frame_reads_only_the_remaining_bytes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    payload = _payload(["message.eml"])
    pending = bytearray(_frame(payload))
    sizes: list[int] = []

    def read(descriptor: int, size: int) -> bytes:
        assert descriptor == 199
        sizes.append(size)
        result = bytes(pending[:1])
        del pending[:1]
        return result

    monkeypatch.setattr(os, "read", read)
    monkeypatch.setattr(request_transport, "MAX_FRAME_BYTES", len(payload))
    assert request_transport.read(199) == ["message.eml"]
    assert sizes == [4, 3, 2, 1, *range(len(payload), 0, -1)]
    assert not pending
