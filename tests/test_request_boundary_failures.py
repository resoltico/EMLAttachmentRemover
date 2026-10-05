"""Failure controls for request reading, resource ownership and folder observation."""

from __future__ import annotations

import argparse
import base64
import io
import json
import os
import signal
import threading
from contextlib import ExitStack
from pathlib import Path
from typing import override

import pytest

from eml_attachment_remover import (
    cli_parser,
    request_owner,
    request_transport,
    selection_collection,
)
from eml_attachment_remover.cancellation import GRACE_SECONDS, POLL_SECONDS
from eml_attachment_remover.domain import AppError, ExitCode


def _frame(encoding: str, entries: list[object]) -> bytes:
    body = json.dumps({"encoding": encoding, "paths": entries}).encode()
    return len(body).to_bytes(4, "big") + body


def test_empty_preconfigured_namespace_is_a_usage_error() -> None:
    """Callers supplying a namespace cannot bypass the source requirement."""
    with pytest.raises(AppError, match="required: source") as failure:
        cli_parser.validate_arguments(argparse.Namespace(source=[]))
    assert failure.value.code is ExitCode.USAGE
    assert failure.value.message == "the following arguments are required: source"


def test_non_descriptor_input_has_a_typed_request_error() -> None:
    """A text-only stream never creates a monitor or acquires a descriptor."""
    with (
        ExitStack() as storage,
        pytest.raises(AppError, match="readable pipe") as failure,
    ):
        storage.enter_context(request_owner.sources(io.StringIO("request")))
    assert failure.value.code is ExitCode.USAGE
    assert isinstance(failure.value.__cause__, OSError)


@pytest.mark.parametrize(
    ("entries", "message"),
    [
        ([1], "request path is not Base64 text"),
        (["not-valid-base64!"], "invalid native path encoding"),
        (["@@@YQ=="], "invalid native path encoding"),
    ],
)
def test_invalid_address_values_are_refused(
    entries: list[object], message: str
) -> None:
    """Malformed address values cannot reach source binding."""
    encoding = "windows-utf16le" if os.name == "nt" else "posix-bytes"
    data = _frame(encoding, entries)
    reader, writer = os.pipe()
    try:
        os.write(writer, data)
        with pytest.raises(AppError) as failure:
            request_transport.read(reader)
        assert failure.value.code is ExitCode.USAGE
        assert failure.value.message == message
    finally:
        os.close(reader)
        os.close(writer)


def test_windows_native_address_decoding_and_truncated_code_units() -> None:
    """UTF-16 roundtrips native units and refuses incomplete code units."""
    decode = request_transport.__dict__["_path"]
    address = "folder/\ud800.eml"
    encoded = base64.b64encode(address.encode("utf-16-le", "surrogatepass")).decode()
    assert decode(encoded, "windows-utf16le") == address
    with pytest.raises(AppError, match="native path encoding"):
        decode(base64.b64encode(b"x").decode(), "windows-utf16le")


def test_closed_descriptor_and_partial_frame_are_typed_failures() -> None:
    """Read errors and early EOF are distinct from an empty source selection."""
    with pytest.raises(AppError, match="could not read request pipe"):
        request_transport.read(-1)
    reader, writer = os.pipe()
    try:
        os.write(writer, b"\0\0")
        os.close(writer)
        writer = -1
        with pytest.raises(AppError, match="before a complete frame"):
            request_transport.read(reader)
    finally:
        os.close(reader)
        if writer != -1:
            os.close(writer)


def test_nonblocking_request_waits_for_real_delayed_input() -> None:
    """Temporary empty-pipe reads do not become premature EOF or busy spinning."""
    reader, writer = os.pipe()
    os.set_blocking(reader, False)
    encoding = "windows-utf16le" if os.name == "nt" else "posix-bytes"
    native = "source.eml".encode("utf-16-le") if os.name == "nt" else b"source.eml"
    frame = _frame(encoding, [base64.b64encode(native).decode()])
    worker = threading.Timer(0.05, os.write, args=(writer, frame))
    try:
        worker.start()
        assert request_transport.read(reader) == ["source.eml"]
    finally:
        worker.join(timeout=5)
        os.close(reader)
        os.close(writer)
    assert not worker.is_alive()


def test_monitor_read_failure_requests_one_bounded_interruption(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A broken owner descriptor requests cancellation and retains a hard deadline."""
    waits: list[float | None] = []
    signals: list[int] = []
    exits: list[int] = []

    class Deadline(threading.Event):
        @override
        def wait(self, timeout: float | None = None) -> bool:
            waits.append(timeout)
            return False

    def read_failure(_descriptor: int, _size: int) -> bytes:
        message = "owner descriptor lost"
        raise OSError(message)

    monkeypatch.setattr(os, "read", read_failure)
    monkeypatch.setattr(request_owner, "interrupt_main", signals.append)
    monkeypatch.setattr(request_owner, "hard_exit", exits.append)
    request_owner.__dict__["_watch"](17, Deadline())
    assert signals == [signal.SIGINT]
    assert exits == [130]
    assert waits == [POLL_SECONDS, GRACE_SECONDS]


def test_unretired_monitor_does_not_prevent_descriptor_cleanup(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Join failure remains visible while borrowed flags and ownership are restored."""

    class UnretiredWorker:
        ident = 1
        alive = True

        def start(self) -> None:
            pass

        @staticmethod
        def join(_timeout: float) -> None:
            assert _timeout == GRACE_SECONDS

        def is_alive(self) -> bool:
            return self.alive

    monkeypatch.setattr(threading, "Thread", lambda **_kw: UnretiredWorker())
    reader, writer = os.pipe()
    encoding = "windows-utf16le" if os.name == "nt" else "posix-bytes"
    native = "source.eml".encode("utf-16-le") if os.name == "nt" else b"source.eml"
    os.write(writer, _frame(encoding, [base64.b64encode(native).decode()]))
    try:
        with os.fdopen(reader, "r") as stream:
            with (
                pytest.raises(
                    RuntimeError, match=r"^request owner monitor did not stop$"
                ),
                request_owner.sources(stream),
            ):
                pass
            assert os.get_blocking(reader)
            assert os.fstat(reader)
    finally:
        os.close(writer)


def test_absent_monitor_cleanup_does_not_attempt_a_join() -> None:
    request_owner.__dict__["_join"](None)


def test_folder_replacement_before_enumeration_is_refused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A folder becoming an ordinary file cannot silently become an empty scan."""
    root = tmp_path / "emails"
    root.mkdir()
    source = root / "message.eml"
    source.write_bytes(b"public body")
    file_metadata = source.stat()
    original = Path.stat
    observations = 0

    def observe(path: Path, *, follow_symlinks: bool = True) -> os.stat_result:
        nonlocal observations
        if path == root and not follow_symlinks:
            observations += 1
            if observations == 2:
                return file_metadata
        return original(path, follow_symlinks=follow_symlinks)

    monkeypatch.setattr(Path, "stat", observe)
    with pytest.raises(AppError, match="selected folder changed"):
        selection_collection.collect([str(root)])
    assert source.read_bytes() == b"public body"
