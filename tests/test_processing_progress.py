"""Progress channels cannot replace final reports or alter publication outcomes."""

from __future__ import annotations

import errno
import json
import os
from contextlib import ExitStack
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch, cli, progress_transport, report_stream
from eml_attachment_remover.domain import AppError, ExitCode

if TYPE_CHECKING:
    from pathlib import Path


def _sources(root: Path) -> list[str]:
    first = root / "first.eml"
    third = root / "third.eml"
    for source in (first, third):
        source.write_bytes(b"Subject: Public progress QA\r\n\r\nPublic body.\r\n")
    directory = root / "directory.eml"
    directory.mkdir()
    return [str(first), str(directory), str(third)]


@pytest.mark.parametrize("fail_fast", [False, True])
def test_attempt_counts_include_failures_but_not_unprocessed_items(
    tmp_path: Path, *, fail_fast: bool
) -> None:
    """Counts describe finished attempts and remain separate from successful copies."""
    updates: list[tuple[int, int]] = []
    ledger = batch.execute(
        _sources(tmp_path),
        batch.BatchOptions(
            dry_run=False,
            existing="verify",
            fail_fast=fail_fast,
            output=None,
            output_dir=None,
        ),
        progress=lambda completed, total: updates.append((completed, total)),
    )
    try:
        assert updates == (
            [(0, 3), (1, 3), (2, 3)] if fail_fast else [(0, 3), (1, 3), (2, 3), (3, 3)]
        )
        assert (tmp_path / "first.mime-pruned.eml").exists()
        assert (tmp_path / "third.mime-pruned.eml").exists() is not fail_fast
    finally:
        report_stream.close(ledger)


def test_observer_fault_does_not_change_successful_publications(tmp_path: Path) -> None:
    """A failed observer is retired while the authoritative batch continues."""
    calls: list[int] = []

    def observer(completed: int, _total: int) -> None:
        calls.append(completed)
        message = "deliberate observer failure"
        raise RuntimeError(message)

    ledger = batch.execute(
        _sources(tmp_path),
        batch.BatchOptions(
            dry_run=False,
            existing="verify",
            fail_fast=False,
            output=None,
            output_dir=None,
        ),
        progress=observer,
    )
    try:
        assert calls == [0]
        assert (tmp_path / "first.mime-pruned.eml").exists()
        assert (tmp_path / "third.mime-pruned.eml").exists()
    finally:
        report_stream.close(ledger)


@pytest.mark.parametrize("failure", [MemoryError, SystemExit])
def test_observers_do_not_swallow_process_control_or_memory_failure(
    tmp_path: Path, failure: type[BaseException]
) -> None:
    """Only ordinary advisory faults are contained, not fatal caller controls."""

    def observer(_completed: int, _total: int) -> None:
        raise failure

    with pytest.raises(failure):
        batch.execute(
            _sources(tmp_path),
            batch.BatchOptions(
                dry_run=False,
                existing="verify",
                fail_fast=False,
                output=None,
                output_dir=None,
            ),
            progress=observer,
        )
    assert not (tmp_path / "first.mime-pruned.eml").exists()


def test_cli_progress_has_counts_only_and_preserves_final_json(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The real pipe receives advisory events while stdout remains one final report."""
    sources = _sources(tmp_path)
    reader, writer = os.pipe()
    before = os.get_blocking(writer)
    try:
        status = cli.main([
            "--progress-fd",
            str(writer),
            "--output-format=json",
            *sources,
        ])
        assert os.get_blocking(writer) is before
        os.close(writer)
        writer = -1
        records = os.read(reader, 65536).splitlines()
    finally:
        os.close(reader)
        if writer != -1:
            os.close(writer)
    captured = capsys.readouterr()
    report = json.loads(captured.out)
    assert status == report["exit_code"] == 9
    assert len(report["items"]) == 3
    events = [json.loads(line.removeprefix(b"EML_PROGRESS ")) for line in records]
    assert [(event["completed"], event["total"]) for event in events] == [
        (0, 3),
        (1, 3),
        (2, 3),
        (3, 3),
        (3, 3),
    ]
    assert all(line.startswith(b"EML_PROGRESS ") for line in records)
    assert all(event["schema"] == 1 for event in events)
    assert [event["stage"] for event in events] == ["processing"] * 4 + ["reporting"]
    assert all(
        set(event) == {"schema", "stage", "completed", "total"} for event in events
    )


@pytest.mark.parametrize("kind", ["closed", "full"])
def test_unavailable_progress_pipe_never_blocks_or_fails_processing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], kind: str
) -> None:
    """Closed readers and full non-draining pipes preserve normal batch behavior."""
    sources = _sources(tmp_path)
    reader, writer = os.pipe()
    try:
        if kind == "closed":
            os.close(reader)
            reader = -1
        else:
            _fill_pipe(writer)
        status = cli.main([
            "--progress-fd",
            str(writer),
            "--output-format=json",
            *sources,
        ])
        assert status == 9
        assert json.loads(capsys.readouterr().out)["items"][0]["status"] == "created"
    finally:
        if reader != -1:
            os.close(reader)
        os.close(writer)


@pytest.mark.parametrize("descriptor", [-1, 0, 1, 2, 999999])
def test_invalid_progress_descriptors_are_usage_errors(descriptor: int) -> None:
    """Progress cannot overwrite stdio or silently take ownership of invalid handles."""
    with ExitStack() as resources, pytest.raises(AppError) as failure:
        resources.enter_context(progress_transport.open_pipe(descriptor))
    assert failure.value.code is ExitCode.USAGE


def _fill_pipe(writer: int) -> None:
    """Create real output backpressure without a waiting consumer."""
    os.set_blocking(writer, False)
    while True:
        try:
            os.write(writer, b"x" * 4096)
        except BlockingIOError:
            return


def test_regular_file_is_not_a_progress_channel(tmp_path: Path) -> None:
    """Refuse ordinary files before changing flags or writing advisory bytes."""
    target = tmp_path / "untouched"
    target.write_bytes(b"original")
    with target.open("rb") as stream:
        before = os.get_blocking(stream.fileno()) if os.name != "nt" else None
        with ExitStack() as resources, pytest.raises(AppError) as failure:
            resources.enter_context(progress_transport.open_pipe(stream.fileno()))
        assert failure.value.code is ExitCode.USAGE
        assert failure.value.message == "progress descriptor must be a pipe"
        if before is not None:
            assert os.get_blocking(stream.fileno()) is before
    assert target.read_bytes() == b"original"


def test_read_end_is_rejected_without_consuming_or_closing_the_pipe() -> None:
    """A readable pipe is not writable; both caller-owned ends remain usable."""
    reader, writer = os.pipe()
    try:
        before = os.get_blocking(reader)
        with ExitStack() as resources, pytest.raises(AppError) as failure:
            resources.enter_context(progress_transport.open_pipe(reader))
        assert failure.value.code is ExitCode.USAGE
        assert os.get_blocking(reader) is before
        assert os.write(writer, b"retained") == 8
        assert os.read(reader, 8) == b"retained"
    finally:
        os.close(reader)
        os.close(writer)


def test_partial_write_retires_progress_without_retrying(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A short advisory write must not be followed by another misleading record."""
    writes: list[bytes] = []

    def partial(_descriptor: int, record: bytes) -> int:
        writes.append(record)
        return len(record) - 1

    monkeypatch.setattr(os, "write", partial)
    progress = progress_transport.ProgressPipe(99)
    progress.update(0, 2)
    progress.update(1, 2)
    progress.reporting()
    assert len(writes) == 1
    assert json.loads(writes[0].removeprefix(progress_transport.PREFIX)) == {
        "schema": 1,
        "stage": "processing",
        "completed": 0,
        "total": 2,
    }


def test_closed_reader_during_write_probe_remains_advisory(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Platforms may report a closed reader at the zero-byte write probe itself."""
    reader, writer = os.pipe()
    real_write = os.write

    def unavailable(descriptor: int, data: bytes) -> int:
        if not data:
            raise BrokenPipeError(errno.EPIPE, "reader disappeared during probe")
        return real_write(descriptor, data)

    monkeypatch.setattr(os, "write", unavailable)
    os.close(reader)
    try:
        before = os.get_blocking(writer)
        with progress_transport.open_pipe(writer) as progress:
            progress.update(0, 1)
        assert os.get_blocking(writer) is before
    finally:
        os.close(writer)


def test_reporting_without_attempt_counts_emits_nothing() -> None:
    """An idle writer has no fabricated zero-file report preparation event."""
    reader, writer = os.pipe()
    try:
        os.set_blocking(reader, False)
        progress_transport.ProgressPipe(writer).reporting()
        with pytest.raises(BlockingIOError):
            os.read(reader, 1)
    finally:
        os.close(reader)
        os.close(writer)


def test_writable_progress_handle_with_a_bad_crt_write_is_refused(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Native rights do not hide an independently invalid CRT descriptor."""
    reader, writer = os.pipe()
    try:

        def failed(_descriptor: int, _data: bytes) -> int:
            raise OSError(errno.EBADF, "bad descriptor")

        monkeypatch.setattr(os, "write", failed)
        with ExitStack() as scope, pytest.raises(AppError, match="must be writable"):
            scope.enter_context(progress_transport.open_pipe(writer))
    finally:
        os.close(reader)
        os.close(writer)


def test_denied_native_progress_access_preserves_descriptor_flags(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Reject denied kernel access before duplicating or probing the channel."""
    reader, writer = os.pipe()
    try:
        before = os.get_blocking(writer)
        monkeypatch.setattr(progress_transport.__dict__["os"], "name", "nt")

        def denied(descriptor: int) -> bool:
            assert descriptor == writer
            return False

        monkeypatch.setattr(
            progress_transport.__dict__["native_windows_pipe"],
            "has_write_access",
            denied,
        )
        with (
            ExitStack() as scope,
            pytest.raises(AppError, match="must be writable") as caught,
        ):
            scope.enter_context(progress_transport.open_pipe(writer))
        assert caught.value.code is ExitCode.USAGE
        assert caught.value.message == "progress pipe must be writable"
        assert os.get_blocking(writer) is before
    finally:
        os.close(reader)
        os.close(writer)
