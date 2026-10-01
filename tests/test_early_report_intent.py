"""Expected early failures preserve parsed and inferred machine-report intent."""

from __future__ import annotations

import json
import signal
import sys
import tempfile
from io import BytesIO
from typing import TYPE_CHECKING, override

import pytest

from eml_attachment_remover import cli, report_emergency, report_spool
from eml_attachment_remover.report_delivery import StagedChannels
from tests.live_report_support import inputs

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("dry_run", [False, True])
def test_startup_storage_failure_preserves_json_and_mode(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    *,
    dry_run: bool,
) -> None:
    sources = inputs(tmp_path)

    def unavailable(*_args: object) -> StagedChannels:
        message = "public storage allocation failure"
        raise OSError(message)

    monkeypatch.setattr(StagedChannels, "open", unavailable)
    args = ["--dry-run"] if dry_run else []
    assert cli.main([*args, "--output-format=json", *sources]) == 7
    captured = capsys.readouterr()
    document = json.loads(captured.out)
    assert document["mode"] == ("dry-run" if dry_run else "apply")
    assert document["batch_error"]["code"] == "WRITE_ERROR"
    assert document["batch_error"]["phase"] == "report"
    assert document["summary"]["not_run"] == 2
    assert all(
        item["error"]["message"] == "not run after report startup failure"
        for item in document["items"]
    )
    assert not captured.err
    assert not list(tmp_path.glob("*.mime-pruned.eml"))


def test_unsafe_temporary_root_is_rejected_with_a_json_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    sources = inputs(tmp_path)
    monkeypatch.setattr(tempfile, "tempdir", str(report_spool.PROJECT_ROOT))
    assert cli.main(["--output-format=json", *sources]) == 7
    document = json.loads(capsys.readouterr().out)
    assert (
        "private terminal report root is unsafe" in document["batch_error"]["message"]
    )
    assert not list(tmp_path.glob("*.mime-pruned.eml"))


def test_postparse_usage_failure_keeps_the_dry_run_receipt(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert (
        cli.main([
            "--dry-run",
            "--output-format=json",
            "--output",
            "x",
            "a.eml",
            "b.eml",
        ])
        == 2
    )
    document = json.loads(capsys.readouterr().out)
    assert document["mode"] == "dry-run"
    assert document["summary"]["not_run"] == 2


def test_postparse_failure_keeps_a_negative_number_filename(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert (
        cli.main(["--output-format=json", "--output=out.eml", "-1", "second.eml"]) == 2
    )
    document = json.loads(capsys.readouterr().out)
    assert [item["source_request"]["text"] for item in document["items"]] == [
        "-1",
        "second.eml",
    ]


def test_a_parsing_failure_infers_dry_run_before_the_separator(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert (
        cli.main(["--dry-run", "--output-format=json", "--unknown", "--", "--dry-run"])
        == 2
    )
    document = json.loads(capsys.readouterr().out)
    assert document["mode"] == "dry-run"
    assert document["items"][0]["source_request"]["text"] == "--dry-run"


def test_flag_like_filenames_after_separator_do_not_change_intent(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert cli.main(["--output-format=json", "--unknown", "--", "--dry-run"]) == 2
    assert json.loads(capsys.readouterr().out)["mode"] == "apply"
    assert cli.main(["--unknown", "--", "--output-format=json"]) == 2
    captured = capsys.readouterr()
    assert not captured.out
    assert "USAGE" in captured.err


def test_parsed_final_format_overrides_raw_json_options(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def unavailable(*_args: object) -> StagedChannels:
        message = "public storage allocation failure"
        raise OSError(message)

    monkeypatch.setattr(StagedChannels, "open", unavailable)
    assert (
        cli.main(["--output-format=json", "--output-format=human", "source.eml"]) == 7
    )
    captured = capsys.readouterr()
    assert not captured.out
    assert "WRITE_ERROR" in captured.err


def test_a_signal_during_early_json_delivery_preserves_the_complete_error(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    class Signalled(BytesIO):
        @override
        def write(self, payload: object) -> int:
            if not self.tell():
                signal.raise_signal(signal.SIGINT)
            return super().write(payload)  # type: ignore[arg-type]

    class Output:
        encoding = "utf-16"
        buffer = Signalled()

    monkeypatch.setattr(sys, "stdout", Output())
    assert (
        cli.main(["--dry-run", "--output-format=json", "--output=x", "a", "b"]) == 130
    )
    document = json.loads(Output.buffer.getvalue().decode("ascii"))
    assert document["exit_code"] == 2
    assert document["mode"] == "dry-run"
    assert "after the report was delivered" in capsys.readouterr().err


def test_a_closed_pipe_during_early_json_delivery_returns_failure(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class Closed(BytesIO):
        @override
        def write(self, _payload: object) -> int:
            raise BrokenPipeError

    class Output:
        encoding = "utf-8"
        buffer = Closed()

    monkeypatch.setattr(sys, "stdout", Output())
    assert cli.main(["--output-format=json", "--output=x", "a", "b"]) == 1


def test_cancellation_during_startup_preserves_selected_json_intent(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def cancelled(*_args: object) -> StagedChannels:
        raise KeyboardInterrupt

    monkeypatch.setattr(StagedChannels, "open", cancelled)
    assert cli.main(["--dry-run", "--output-format=json", "source.eml"]) == 130
    document = json.loads(capsys.readouterr().out)
    assert document["mode"] == "dry-run"
    assert document["interrupted"]
    assert document["interruption"]["signal"] == "SIGINT"
    assert document["interruption"]["phase"] == "startup"


def test_preparse_failure_does_not_treat_an_output_value_as_a_source(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert (
        cli.main([
            "--output-format=json",
            "--output",
            "destination.eml",
            "--unknown",
            "source.eml",
        ])
        == 2
    )
    document = json.loads(capsys.readouterr().out)
    assert [item["source_request"]["text"] for item in document["items"]] == [
        "source.eml"
    ]


def test_an_invalid_format_with_a_second_equals_does_not_select_json(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert cli.main(["--output-format=--output-format=json", "source.eml"]) == 2
    captured = capsys.readouterr()
    assert not captured.out
    assert "USAGE" in captured.err


def test_interrupted_error_staging_keeps_parsed_sources_and_mode(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    original = report_emergency.stage
    calls: list[int] = []

    def staged(*args: object) -> StagedChannels:
        calls.append(1)
        if len(calls) == 1:
            raise KeyboardInterrupt
        return original(*args)  # type: ignore[arg-type]

    monkeypatch.setattr(report_emergency, "stage", staged)
    assert (
        cli.main([
            "--dry-run",
            "--output-format=json",
            "--output=out.eml",
            "first.eml",
            "second.eml",
        ])
        == 130
    )
    document = json.loads(capsys.readouterr().out)
    assert document["mode"] == "dry-run"
    assert document["interrupted"]
    assert [item["source_request"]["text"] for item in document["items"]] == [
        "first.eml",
        "second.eml",
    ]


@pytest.mark.parametrize("preceding_source", ["-", "-1"])
def test_a_preceding_filename_does_not_consume_a_later_dry_run_flag(
    capsys: pytest.CaptureFixture[str], preceding_source: str
) -> None:
    assert (
        cli.main([
            preceding_source,
            "--output",
            "--dry-run",
            "--output-format=json",
            "source.eml",
        ])
        == 2
    )
    document = json.loads(capsys.readouterr().out)
    assert document["mode"] == "dry-run"
