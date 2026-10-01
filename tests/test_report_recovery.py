"""Complete sessions preserve receipts across native paths and persistent failures."""

from __future__ import annotations

import json
import os
import sys
from contextlib import ExitStack
from io import BytesIO, TextIOWrapper
from typing import TYPE_CHECKING, override

import pytest

from eml_attachment_remover import (
    cli,
    report_emergency,
    report_session,
    report_spool,
    report_stream,
)
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import (
    AppError,
    BatchLedger,
    ExitCode,
    ItemStatus,
    PublicationReceipt,
)
from eml_attachment_remover.native_paths import path_value
from eml_attachment_remover.report_delivery import StagedChannels
from tests.report_session_support import complete_owned, open_session

if TYPE_CHECKING:
    from typing import TextIO


def _created(paths: list[str]) -> BatchLedger:
    ledger = BatchLedger.from_requests([path_value("source.eml") for _ in paths])
    for item, path in zip(ledger.items, paths, strict=True):
        item.publication = PublicationReceipt(
            "visible",
            None,
            "a" * 64,
            "succeeded",
            "succeeded",
            address_verified=True,
            final_address=path_value(path),
            temp_cleanup="removed",
        )
        item.finish(ItemStatus.CREATED)
    report_stream.start(ledger)
    report_stream.archive_all(ledger)
    return ledger


@pytest.mark.skipif(os.name == "nt", reason="POSIX native filenames")
@pytest.mark.parametrize("prefix_count", [0, 100])
@pytest.mark.parametrize("native", [b"native-\xff.eml", b"native-\xe9.eml"])
def test_native_paths_survive_the_complete_session_at_every_position(
    monkeypatch: pytest.MonkeyPatch,
    prefix_count: int,
    native: bytes,
) -> None:
    raw = BytesIO()
    terminal = TextIOWrapper(raw, encoding="utf-16")
    monkeypatch.setattr(sys, "stdout", terminal)
    paths = ["/" + "a" * 110 for _ in range(prefix_count)] + [os.fsdecode(native)]
    ledger = _created(paths)
    try:
        with ExitStack() as resources:
            assert open_session(resources, "paths0").publish(ledger, 0) == 0
        assert raw.getvalue() == b"".join(os.fsencode(path) + b"\0" for path in paths)
    finally:
        report_stream.close(ledger)


class _Broken:
    """A staging boundary that never recovers."""

    def __init__(self, real: TextIO, operation: str) -> None:
        self.real = real
        self.operation = operation
        self.buffer = real.buffer

    def __getattr__(self, name: str) -> object:
        if name == self.operation:

            def fail(*_args: object) -> object:
                message = "persistent staging failure"
                raise OSError(message)

            return fail
        return getattr(self.real, name)


@pytest.mark.parametrize("operation", ["flush", "write", "seek", "truncate"])
@pytest.mark.parametrize("already_recovering", [False, True])
def test_persistent_staging_failure_preserves_completed_receipts(
    capsys: pytest.CaptureFixture[str],
    operation: str,
    *,
    already_recovering: bool,
) -> None:
    ledger = _created(["output.eml"])
    if already_recovering:
        report_stream.recover(ledger)
    try:
        with ExitStack() as resources:
            session = open_session(resources, "json")
            session.channels = StagedChannels(
                _Broken(session.channels.out, operation),  # type: ignore[arg-type]
                session.channels.err,
            )
            assert session.publish(ledger, 0) == int(ExitCode.WRITE_ERROR)
        document = json.loads(capsys.readouterr().out)
        assert document["summary"]["created"] == 1
        assert (
            document["items"][0]["publication"]["final_address"]["text"] == "output.eml"
        )
        assert document["items"][0]["transformation"] is None
        assert document["exit_code"] == int(ExitCode.WRITE_ERROR)
        assert session.delivery_started
    finally:
        report_stream.close(ledger)


@pytest.mark.parametrize("output_format", ["json", "human", "paths0"])
def test_memory_recovery_does_not_read_failed_spools(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    output_format: str,
) -> None:
    ledger = _created(["output.eml"])
    ledger.items[0].error = AppError(ExitCode.WRITE_ERROR, "detail", (1, 2), "publish")

    def unavailable(_spool: report_spool.ReportSpool) -> object:
        message = "persistent unavailable spool"
        raise OSError(message)

    monkeypatch.setattr(report_spool.ReportSpool, "records", unavailable)
    try:
        with ExitStack() as resources:
            session = open_session(resources, output_format)
            assert session.publish(ledger, 0) == int(ExitCode.WRITE_ERROR)
        captured = capsys.readouterr()
        if output_format == "json":
            item = json.loads(captured.out)["items"][0]
            assert item["error"]["mime_path"] is None
        elif output_format == "paths0":
            assert captured.out == "output.eml\0"
        else:
            assert "created: source.eml -> output.eml" in captured.out
        assert session.delivery_started
    finally:
        report_stream.close(ledger)


def test_emergency_memory_has_an_enforced_capacity(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(report_emergency, "MEMORY_LIMIT", 3)
    with report_emergency.BoundedBuffer() as buffer:
        assert buffer.write(b"abc") == 3
        with pytest.raises(OSError, match="memory capacity"):
            buffer.write(b"d")
        assert buffer.getvalue() == b"abc"


def test_a_persistently_failed_buffer_cannot_replace_recovery_during_close(
    capsys: pytest.CaptureFixture[str],
) -> None:
    class Full(BytesIO):
        """A text wrapper's byte buffer rejects every write and flush retry."""

        @override
        def write(self, _payload: object) -> int:
            message = "persistent no space"
            raise OSError(message)

        @override
        def flush(self) -> None:
            message = "persistent flush failure during close"
            raise OSError(message)

    ledger = _created(["output.eml"])
    try:
        with ExitStack() as resources:
            failed = resources.enter_context(TextIOWrapper(Full(), encoding="ascii"))
            session = open_session(resources, "json")
            diagnostics = session.channels.err
            session.channels = StagedChannels(failed, session.channels.err)
            assert session.publish(ledger, 0) == int(ExitCode.WRITE_ERROR)
            assert failed.closed
            assert diagnostics.closed
        assert json.loads(capsys.readouterr().out)["summary"]["created"] == 1
    finally:
        report_stream.close(ledger)


def test_cli_cancellation_during_memory_staging_restarts_with_fresh_channels(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    ledger = _created(["output.eml"])
    original = report_emergency.stage
    completed = ledger
    attempts: list[int] = []

    def full(*_args: object) -> None:
        message = "persistent no space"
        raise OSError(message)

    def interrupted(
        resources: ExitStack,
        ledger: BatchLedger,
        output_format: str,
        mode: str,
        status: int,
    ) -> StagedChannels:
        channels = original(resources, ledger, output_format, mode, status)
        attempts.append(1)
        if len(attempts) == 1:
            raise CancellationSignal(2, "SIGINT")
        return channels

    monkeypatch.setattr(
        cli, "execute", lambda *_args, ledger: complete_owned(ledger, completed)
    )
    monkeypatch.setattr(report_session, "_write_selected", full)
    monkeypatch.setattr(report_emergency, "stage", interrupted)
    try:
        assert cli.main(["--output-format", "json", "source.eml"]) == 130
        document = json.loads(capsys.readouterr().out)
        assert document["exit_code"] == 130
        assert document["interrupted"]
        assert document["summary"]["created"] == 1
        assert len(attempts) == 2
    finally:
        report_stream.close(ledger)
