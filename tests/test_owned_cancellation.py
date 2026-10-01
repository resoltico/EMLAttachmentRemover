"""Real signals cannot separate completed publications from their preowned ledger."""

from __future__ import annotations

import inspect
import json
import signal
from email import policy
from email.parser import BytesParser
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    batch,
    batch_execution,
    cli,
    report_session,
    report_stream,
)
from eml_attachment_remover.domain import BatchLedger
from tests.live_report_support import MESSAGE, inputs
from tests.trace_implementation_support import traced_implementation

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
@pytest.mark.parametrize(
    "boundary",
    [
        "reservation",
        "inventory",
        "publication",
        "finalization",
        "final_archive",
        "return",
        "report",
    ],
)
def test_real_cancellation_retains_known_outcomes_at_every_handoff(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    fmt: str,
    boundary: str,
) -> None:
    sources = inputs(tmp_path)
    injection = _Boundary(boundary)
    injection.install(monkeypatch)
    assert cli.main(["--output-format", fmt, *sources]) == 130
    captured = capsys.readouterr()
    expected = (
        0
        if boundary in {"reservation", "inventory"}
        else 1
        if boundary == "publication"
        else 2
    )
    copies = list(tmp_path.glob("*.mime-pruned.eml"))
    assert len(copies) == expected
    assert injection.sent == [1]
    for source in sources:
        assert (tmp_path / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
    for copy in copies:
        parsed = BytesParser(policy=policy.default).parsebytes(copy.read_bytes())
        assert parsed.get_body().get_content().strip() == "public body"  # type: ignore[union-attr]
    if fmt == "json":
        document = json.loads(captured.out)
        assert document["exit_code"] == 130
        assert document["interrupted"]
        assert document["summary"]["created"] == expected
        assert (
            document["summary"]["not_run"] + document["summary"]["cancelled"]
            == 2 - expected
        )
        assert document["batch_error"] is None
    elif fmt == "paths0":
        assert captured.out.count("\0") == expected
        assert captured.err.count("Interrupted:") == 1
    else:
        assert captured.out.count("created:") == expected
        assert captured.err.count("Interrupted:") == 1
        assert (
            captured.out.count("not_run:") + captured.out.count("cancelled:")
            == 2 - expected
        )


class _Boundary:
    """Install one real-signal seam without changing MIME or publication behavior."""

    def __init__(self, boundary: str) -> None:
        self.boundary = boundary
        self.sent: list[int] = []
        self.start = report_stream.start
        self.inventory = batch._inventory  # ruff: ignore[private-member-access] - real inventory.
        self.archive = report_stream.archive_or_recover
        self.finalize = BatchLedger.finalize_not_run
        self.execute = batch.execute
        self.publish = report_session.ReportSession.publish
        self.final_archive_code = traced_implementation(
            batch_execution._execute,  # ruff: ignore[private-member-access] - final batch archive owner.
        ).__code__

    def interrupt(self) -> None:
        if not self.sent:
            self.sent.append(1)
            signal.raise_signal(signal.SIGINT)

    def install(self, monkeypatch: pytest.MonkeyPatch) -> None:
        # Plain functions preserve instance binding when installed on classes.
        def finalized(ledger: BatchLedger, reason: str) -> None:
            self.finalized(ledger, reason)

        def publishing(
            session: report_session.ReportSession, ledger: BatchLedger, status: int
        ) -> int:
            return self.publishing(session, ledger, status)

        seams = {
            "reservation": (report_stream, "start", self.reserved),
            "inventory": (batch, "_inventory", self.inventoried),
            "publication": (report_stream, "archive_or_recover", self.archived),
            "final_archive": (report_stream, "archive_or_recover", self.archived),
            "finalization": (
                BatchLedger,
                "finalize_not_run",
                finalized,
            ),
            "return": (cli, "execute", self.returned),
            "report": (
                report_session.ReportSession,
                "publish",
                publishing,
            ),
        }
        target, name, callback = seams[self.boundary]
        monkeypatch.setattr(target, name, callback)

    def reserved(self, ledger: BatchLedger) -> None:
        self.start(ledger)
        self.interrupt()

    def inventoried(self, *args: object) -> object:
        result = self.inventory(*args)  # type: ignore[arg-type]
        self.interrupt()
        return result

    def archived(self, ledger: BatchLedger, *args: object) -> bool:
        frame = inspect.currentframe()
        caller = (
            frame.f_back.f_code
            if frame is not None and frame.f_back is not None
            else None
        )
        if self.boundary == "final_archive" and caller is self.final_archive_code:
            self.interrupt()
        result = self.archive(ledger, *args)  # type: ignore[arg-type]
        if self.boundary == "publication" and ledger.items[0].terminalized:
            self.interrupt()
        return result

    def finalized(self, ledger: BatchLedger, reason: str) -> None:
        self.finalize(ledger, reason)
        if reason == "not run":
            self.interrupt()

    def returned(
        self, sources: list[str], options: batch.BatchOptions, *, ledger: BatchLedger
    ) -> BatchLedger:
        result = self.execute(sources, options, ledger=ledger)
        assert result is ledger
        self.interrupt()
        return result

    def publishing(
        self, session: report_session.ReportSession, ledger: BatchLedger, status: int
    ) -> int:
        self.interrupt()
        return self.publish(session, ledger, status)
