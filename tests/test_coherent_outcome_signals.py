"""Real traced SIGINT cannot interrupt successful receipt or terminal commitment."""

from __future__ import annotations

import hashlib
import inspect
import json
import os
import signal
import sys
from email import policy
from email.parser import BytesParser
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import batch, cli, staged_output
from eml_attachment_remover.domain import LedgerItem
from tests.live_report_support import MESSAGE, inputs
from tests.trace_implementation_support import traced_implementation

if TYPE_CHECKING:
    from pathlib import Path
    from types import FrameType

    from _typeshed import TraceFunction


def _statement(function: object, text: str) -> int:
    lines, first = inspect.getsourcelines(function)  # type: ignore[arg-type]
    return next(first + index for index, line in enumerate(lines) if text in line)


@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
@pytest.mark.parametrize(
    "boundary",
    [
        "before_visibility",
        "publisher_return",
        "receipt_received",
        "outcome_commit",
        "status_mirror",
        "terminal_flag",
    ],
)
def test_traced_signal_preserves_coherent_publication_results(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    fmt: str,
    boundary: str,
) -> None:
    sources = inputs(tmp_path)
    code, target = _target(boundary)
    sent: list[int] = []

    def trace(frame: FrameType, event: str, _arg: object) -> TraceFunction:
        if (
            event == "line"
            and frame.f_code is code
            and frame.f_lineno == target
            and not sent
        ):
            sent.append(1)
            signal.raise_signal(signal.SIGINT)
        return trace

    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        assert cli.main(["--output-format", fmt, *sources]) == 130
    finally:
        sys.settrace(previous)
    captured = capsys.readouterr()
    expected = 0 if boundary == "before_visibility" else 1
    copies = list(tmp_path.glob("*.mime-pruned.eml"))
    assert len(copies) == expected
    assert sent == [1]
    _verify_files(tmp_path, sources, copies)
    if fmt == "json":
        report = json.loads(captured.out)
        assert report["interrupted"]
        assert all(item["terminalized"] for item in report["items"])
        assert report["summary"]["created"] == expected
        if expected:
            item = report["items"][0]
            assert item["status"] == "created"
            assert item["publication"]["address_verified"]
            assert (
                item["publication"]["sha256"]
                == hashlib.sha256(copies[0].read_bytes()).hexdigest()
            )
    elif fmt == "paths0":
        assert captured.out.count("\0") == expected
        assert captured.err.count("Interrupted:") == 1
    else:
        assert captured.out.count("created:") == expected
        assert captured.err.count("Interrupted:") == 1


def _target(boundary: str) -> tuple[object, int]:
    """Find semantic statements independently of source line offsets.

    Returns:
        The target code object and statement line.

    """
    targets = {
        "before_visibility": (
            staged_output._publish_edge,  # ruff: ignore[private-member-access] - visibility boundary.
            "checkpoint(before_visibility=True)",
        ),
        "publisher_return": (staged_output._finish_or_raise, "return receipt"),  # ruff: ignore[private-member-access] - proven receipt return.
        "receipt_received": (
            batch._existing_or_publish,  # ruff: ignore[private-member-access] - receipt transfer.
            "item.phase = ItemPhase.PUBLISHED",
        ),
        "outcome_commit": (LedgerItem.finish, "self._outcome = outcome"),
        "status_mirror": (LedgerItem.finish, "self.status = outcome.status"),
        "terminal_flag": (LedgerItem.finish, "self.terminalized = True"),
    }
    selected_function, needle = targets[boundary]
    function = traced_implementation(selected_function)
    target = _statement(function, needle)
    code = function.__code__
    return code, target


def _verify_files(root: Path, sources: list[str], copies: list[Path]) -> None:
    """Check originals, retained bodies, and actual attachment removal."""
    for source in sources:
        assert (root / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
    for copy in copies:
        parsed = BytesParser(policy=policy.default).parsebytes(copy.read_bytes())
        assert parsed.get_body().get_content().strip() == "public body"  # type: ignore[union-attr]
        assert not list(parsed.iter_attachments())


def test_a_known_cancellation_prevents_further_candidate_writes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    sources = inputs(tmp_path)
    function = traced_implementation(staged_output._verify_staged)  # ruff: ignore[private-member-access] - pre-write cancellation checkpoint.
    target = _statement(function, "checkpoint(")
    sent: list[int] = []
    writes: list[int] = []
    original = os.write

    def written(descriptor: int, payload: bytes) -> int:
        if sent and b"public body" in payload:
            writes.append(1)
        return original(descriptor, payload)

    def trace(frame: FrameType, event: str, _arg: object) -> TraceFunction:
        if (
            event == "line"
            and frame.f_code is function.__code__
            and frame.f_lineno == target
            and not sent
        ):
            sent.append(1)
            signal.raise_signal(signal.SIGINT)
        return trace

    monkeypatch.setattr(os, "write", written)
    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        assert cli.main(["--output-format=json", *sources]) == 130
    finally:
        sys.settrace(previous)
    assert sent == [1]
    assert not writes
    assert not list(tmp_path.glob("*.mime-pruned.eml"))
    assert json.loads(capsys.readouterr().out)["interrupted"]
