"""End-to-end CLI receipts for cancellation, dispatch, and raw argv failures."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli
from eml_attachment_remover.cancellation import CancellationSignal
from eml_attachment_remover.domain import BatchLedger, ExitCode, ItemStatus
from eml_attachment_remover.native_paths import path_value

if TYPE_CHECKING:
    from pathlib import Path


def test_dispatch_cancellation_writes_one_terminal_json_receipt(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    """An active interruption retains completed work and terminalizes later inputs."""
    ledger = BatchLedger.from_requests([
        path_value("completed.eml"),
        path_value("not-started.eml"),
    ])
    ledger.items[0].finish(ItemStatus.CREATED)

    def interrupt(raw: list[str], state: cli._RunState) -> int:
        assert raw == ["--output-format=json"]
        state.ledger = ledger
        raise CancellationSignal(15, "SIGTERM")

    monkeypatch.setattr(cli, "_run", interrupt)

    assert cli._dispatch(["--output-format=json"]) == ExitCode.INTERRUPTED  # ruff: ignore[private-member-access] - full dispatch boundary contract.
    captured = capsys.readouterr()
    assert not captured.err
    document = json.loads(captured.out)
    assert (
        document["exit_code"],
        document["interrupted"],
        document["interruption"],
        document["summary"],
    ) == (
        130,
        True,
        {
            "signal": "SIGTERM",
            "reason": "interrupted by SIGTERM",
            "phase": "report",
        },
        {
            "created": 1,
            "existing_verified": 0,
            "would_create": 0,
            "failed": 0,
            "cancelled": 0,
            "not_run": 1,
            "published_with_error": 0,
            "total": 2,
        },
    )
    assert [
        (item["source_request"]["text"], item["status"], item["error"])
        for item in document["items"]
    ] == [
        ("completed.eml", "created", None),
        (
            "not-started.eml",
            "not_run",
            {
                "code": "INTERRUPTED",
                "message": "interrupted by SIGTERM",
                "mime_path": None,
                "phase": "report",
            },
        ),
    ]


@pytest.mark.parametrize(
    ("raised", "expected_status", "expected_error"),
    [
        (
            KeyboardInterrupt(),
            ExitCode.INTERRUPTED,
            "remove-eml-attachments: error[INTERRUPTED:130]: interrupted\n",
        ),
        (BrokenPipeError(), 1, ""),
        (
            RuntimeError("unexpected state"),
            ExitCode.INTERNAL_ERROR,
            "remove-eml-attachments: error[INTERNAL_ERROR:70]: unexpected state\n",
        ),
    ],
)
def test_dispatch_preserves_each_runtime_output_boundary(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    raised: BaseException,
    expected_status: int,
    expected_error: str,
) -> None:
    """Runtime failures have stable process statuses and no accidental stdout."""

    def raise_from_run(*_arguments: object) -> int:
        raise raised

    monkeypatch.setattr(cli, "_run", raise_from_run)

    assert cli._dispatch(["source.eml"]) == expected_status  # ruff: ignore[private-member-access] - runtime dispatcher contract.
    captured = capsys.readouterr()
    assert (captured.out, captured.err) == ("", expected_error)


def test_main_json_usage_excludes_consumed_option_values_from_source_receipts(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """A pre-parse JSON failure inventories sources without inventing option values."""
    arguments = [
        "--output-format",
        "json",
        "--output",
        "redirected.eml",
        "--force=legacy",
        "actual.eml",
        "--",
        "-literal.eml",
    ]

    assert cli.main(arguments) == ExitCode.USAGE
    captured = capsys.readouterr()
    assert not captured.err
    document = json.loads(captured.out)
    assert document["batch_error"] == {
        "code": "USAGE",
        "message": (
            "--force and --skip-existing were removed in v3; use "
            "--existing=error or --existing=verify"
        ),
        "mime_path": None,
        "phase": None,
    }
    assert [item["source_request"]["text"] for item in document["items"]] == [
        "actual.eml",
        "-literal.eml",
    ]
    assert [item["status"] for item in document["items"]] == ["not_run", "not_run"]
    assert "redirected.eml" not in captured.out


def test_main_human_validation_failure_includes_usage_and_nothing_on_stdout(
    capsys: pytest.CaptureFixture[str], tmp_path: Path
) -> None:
    """Post-parse output ownership failures retain the human usage channel."""
    first = tmp_path / "first.eml"
    second = tmp_path / "second.eml"

    assert cli.main(["--output", "copy.eml", str(first), str(second)]) == 2
    captured = capsys.readouterr()
    assert not captured.out
    assert captured.err.endswith(
        "remove-eml-attachments: error[USAGE:2]: "
        "--output may be used only with exactly one source\n"
    )
    assert captured.err.startswith("usage: remove-eml-attachments ")
