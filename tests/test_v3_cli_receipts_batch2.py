"""Public CLI channel receipts independent from mocked dispatcher tests."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

from eml_attachment_remover import cli

if TYPE_CHECKING:
    from pathlib import Path


def _message() -> bytes:
    """Return one deterministic attachment-bearing source message.

    Returns:
        A physical multipart source with exactly one removable attachment.

    """
    return (
        b"Content-Type: multipart/mixed; boundary=m\r\n\r\n"
        b"--m\r\nContent-Type: text/plain\r\n\r\nretained\r\n"
        b"--m\r\nContent-Type: application/octet-stream\r\n"
        b"Content-Disposition: attachment\r\n\r\nremoved\r\n--m--\r\n"
    )


def test_main_dry_run_json_reports_one_complete_would_create_receipt(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The public parser, batch pipeline, and JSON selector preserve dry-run facts."""
    source = tmp_path / "source.eml"
    source.write_bytes(_message())
    assert cli.main(["--dry-run", "--output-format=json", str(source)]) == 0
    captured = capsys.readouterr()
    assert not captured.err
    document = json.loads(captured.out)
    assert (
        document["mode"],
        document["ok"],
        document["exit_code"],
        document["summary"],
    ) == (
        "dry-run",
        True,
        0,
        {
            "created": 0,
            "existing_verified": 0,
            "would_create": 1,
            "failed": 0,
            "cancelled": 0,
            "not_run": 0,
            "published_with_error": 0,
            "total": 1,
        },
    )
    item = document["items"][0]
    assert (
        item["status"],
        item["publication"],
        item["transformation"]["removal_roots"],
    ) == (
        "would_create",
        {
            "visibility": "not_attempted",
            "identity": None,
            "sha256": None,
            "file_sync": "not_attempted",
            "directory_sync": "not_attempted",
            "address_verified": False,
            "final_address": None,
            "temp_cleanup": "not_attempted",
        },
        [
            {
                "mime_path": [1],
                "content_type": "application/octet-stream",
                "reason": "EXPLICIT_ATTACHMENT",
            }
        ],
    )


def test_main_rejects_dry_run_paths0_without_binary_output(
    tmp_path: Path, capfd: pytest.CaptureFixture[str]
) -> None:
    """The incompatible dry-run/paths0 pair stops at argument validation."""
    source = tmp_path / "source.eml"
    source.write_bytes(_message())
    assert cli.main(["--dry-run", "--output-format=paths0", str(source)]) == 2
    captured = capfd.readouterr()
    assert not captured.out
    assert captured.err.endswith(
        "remove-eml-attachments: error[USAGE:2]: "
        "--output-format=paths0 cannot be used with --dry-run\n"
    )


def test_main_version_uses_argparse_success_without_a_source(
    capsys: pytest.CaptureFixture[str],
) -> None:
    """Version handling stays a parser-owned successful process termination."""
    with pytest.raises(SystemExit) as raised:
        cli.main(["--version"])
    assert raised.value.code == 0
    captured = capsys.readouterr()
    assert captured.out == "remove-eml-attachments 3.0.0\n"
    assert not captured.err
