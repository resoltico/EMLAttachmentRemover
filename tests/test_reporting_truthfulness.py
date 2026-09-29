"""Reports tell the whole truth about each copy, on the channel that was requested."""

from __future__ import annotations

import base64
import io
import os
import signal
import subprocess
import sys
import threading
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import (
    cancellation,
    native_values,
    report_spool,
    report_stream,
    reporting_v3,
)
from eml_attachment_remover.domain import PathValue

if TYPE_CHECKING:
    from pathlib import Path

MESSAGE = (
    b"From: a@example.test\r\nMIME-Version: 1.0\r\n"
    b"Content-Type: multipart/mixed; boundary=B\r\n\r\n"
    b"--B\r\nContent-Type: text/plain\r\n\r\nkept\r\n"
    b"--B\r\nContent-Type: application/pdf\r\n"
    b"Content-Disposition: attachment\r\n\r\nX\r\n--B--\r\n"
)


def _native(value: bytes) -> str:
    return base64.b64encode(value).decode("ascii")


def test_paths0_emits_a_native_only_name_from_its_exact_bytes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A path with no Unicode text is emitted, not silently dropped (finding 1)."""
    output = io.BytesIO()
    monkeypatch.setattr(sys, "stdout", type("Out", (), {"buffer": output})())
    final = {
        "text": None,
        "display": "/d/\\xffname.eml",
        "native_base64": _native(b"/d/\xffname.eml"),
        "native_utf16le_base64": None,
    }
    for status in ("created", "existing_verified", "failed"):
        report_stream._write_path_record(  # ruff: ignore[private-member-access] - streamed paths0 record.
            {"status": status, "publication": {"final_address": final}}
        )
    report_stream._write_path_record({"status": "created", "publication": None})  # ruff: ignore[private-member-access] - no receipt.
    assert output.getvalue() == b"/d/\xffname.eml\0" * 2


def test_report_path_value_rebuilds_every_serialized_field() -> None:
    """Streamed records reach the same serializer as in-memory ledgers."""
    assert native_values.report_path_value(None) is None
    assert native_values.report_path_value("not a mapping") is None
    assert native_values.report_path_value({
        "text": "t",
        "display": "d",
        "native_base64": "bmF0aXZl",
        "native_utf16le_base64": "dwBpAGQAZQA=",
    }) == PathValue("t", "d", "bmF0aXZl", "dwBpAGQAZQA=")
    assert native_values.report_path_value({
        "text": 1,
        "display": 2,
        "native_base64": 3,
        "native_utf16le_base64": 4,
    }) == PathValue(None, "2", None, None)


@pytest.mark.parametrize(
    ("destination", "expected"),
    [
        (
            {"parent": {"display": "/out"}, "basename_base64": _native(b"a.eml")},
            "/out/a.eml",
        ),
        (
            {
                "parent": {"display": "C:\\out"},
                "basename_base64": None,
                "basename_utf16le_base64": _native("ā.eml".encode("utf-16-le")),
            },
            "C:\\out\\ā.eml",
        ),
        (
            {
                "parent": {"display": "C:\\"},
                "basename_base64": None,
                "basename_utf16le_base64": _native("b.eml".encode("utf-16-le")),
            },
            "C:\\b.eml",
        ),
        ({"parent": None, "basename_base64": _native(b"c.eml")}, "/c.eml"),
        (
            {
                "parent": {"display": "C:\\out"},
                "basename_base64": None,
                "basename_utf16le_base64": _native(
                    "a\ud800.eml".encode("utf-16-le", "surrogatepass")
                ),
            },
            "C:\\out\\a\ud800.eml",
        ),
    ],
)
def test_planned_display_joins_parent_and_exact_basename(
    destination: dict[str, object], expected: str
) -> None:
    """A dry run shows where its copy would go, in the platform's own separator."""
    assert native_values.planned_display(destination) == expected


def test_undecodable_posix_basename_stays_reversible_in_a_planned_display(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Native bytes with no text keep a surrogate escape; Windows has no such name."""
    monkeypatch.setattr(
        os, "fsdecode", lambda value: bytes(value).decode("utf-8", "surrogateescape")
    )
    destination = {
        "parent": {"display": "/"},
        "basename_base64": _native(b"\xff.eml"),
    }
    assert native_values.planned_display(destination) == "/\udcff.eml"


def test_human_lines_name_the_final_or_planned_destination() -> None:
    """Human output says where each copy is, or would be (finding 8)."""
    final = {"display": "/out/source.mime-pruned.eml"}
    planned = {"parent": {"display": "/plan"}, "basename_base64": _native(b"p.eml")}
    target = reporting_v3._target_display  # ruff: ignore[private-member-access] - human destination.
    assert target({"publication": {"final_address": final}}) == final["display"]
    assert target({"status": "would_create", "destination": planned}) == "/plan/p.eml"
    assert target({"status": "failed", "destination": planned}) is None
    assert target({"status": "would_create", "destination": None}) is None
    assert target({"publication": {"final_address": None}}) is None


def _deliver_through_two_signals() -> None:
    with cancellation.defer_cancellation():
        signal.raise_signal(signal.SIGINT)
        signal.raise_signal(signal.SIGINT)


def test_deferred_cancellation_is_raised_once_after_the_block() -> None:
    """A signal during delivery is held, then honored with its own name."""
    with (
        cancellation.install_cancellation_handlers(),
        pytest.raises(cancellation.CancellationSignal) as raised,
    ):
        _deliver_through_two_signals()
    assert (raised.value.number, raised.value.name) == (signal.SIGINT, "SIGINT")


def test_deferred_cancellation_without_a_signal_restores_handlers() -> None:
    """An undisturbed delivery raises nothing and leaves prior handlers in place."""
    before = signal.getsignal(signal.SIGINT)
    with cancellation.defer_cancellation():
        assert signal.getsignal(signal.SIGINT) is not before
    assert signal.getsignal(signal.SIGINT) is before


def test_deferred_cancellation_off_the_main_thread_changes_nothing() -> None:
    """Signal handlers belong to the main thread; workers only run the block."""
    seen: list[bool] = []

    def worker() -> None:
        before = signal.getsignal(signal.SIGINT)
        with cancellation.defer_cancellation():
            seen.append(signal.getsignal(signal.SIGINT) is before)

    thread = threading.Thread(target=worker)
    thread.start()
    thread.join()
    assert seen == [True]


def test_temp_root_check_applies_only_to_a_source_checkout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A zipapp's neighbouring directories are valid temporary roots (finding 9)."""
    inside = report_spool.PROJECT_ROOT / "build"
    inside.mkdir(exist_ok=True)
    monkeypatch.setattr("tempfile.gettempdir", lambda: str(inside))
    assert report_spool.SOURCE_CHECKOUT is True
    with pytest.raises(report_spool.ReportSpoolError):
        report_spool.private_temp_root()
    monkeypatch.setattr(report_spool, "SOURCE_CHECKOUT", False)
    assert report_spool.private_temp_root() == inside.resolve()


def test_ascii_terminal_gets_an_escaped_successful_human_report(tmp_path: Path) -> None:
    """A Unicode-named copy never fails on an ASCII terminal (finding 2)."""
    source = tmp_path / "rēķins.eml"
    source.write_bytes(MESSAGE)
    environment = {
        **os.environ,
        "PYTHONIOENCODING": "ascii:strict",
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    result = subprocess.run(
        [sys.executable, "-m", "eml_attachment_remover", "--", str(source)],
        env=environment,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
    line = result.stdout.decode("ascii")
    escaped = str(source).encode("ascii", "backslashreplace").decode("ascii")
    assert line.startswith(f"created: {escaped} -> ")
    # The copy's own name is what the audit's failure lost; it arrives escaped.
    assert line.endswith("\n")
    assert line.rstrip("\r\n").endswith("r\\u0113\\u0137ins.mime-pruned.eml")
    assert (tmp_path / "rēķins.mime-pruned.eml").exists()
