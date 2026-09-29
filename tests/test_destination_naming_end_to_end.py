"""Default output names on a real filesystem: fitted, stable, and never colliding."""

from __future__ import annotations

import base64
import hashlib
import json
import os
from typing import TYPE_CHECKING

from eml_attachment_remover import cli
from eml_attachment_remover.batch import BatchOptions, execute
from eml_attachment_remover.domain import ExitCode, ItemStatus

if TYPE_CHECKING:
    from pathlib import Path

    import pytest

MESSAGE = b"From: a@example.test\r\n\r\nretained\r\n"
SUFFIX = ".mime-pruned.eml"


def _tail(name: str) -> str:
    """Return the hash-and-suffix tail, computed independently of the fitter.

    Returns:
        ``-<16 hex of sha256(native name)>.mime-pruned.eml``, native meaning
        UTF-16 on Windows and UTF-8 elsewhere.

    """
    encoding = "utf-16-le" if os.name == "nt" else "utf-8"
    digest = hashlib.sha256(name.encode(encoding)).hexdigest()
    return f"-{digest[:16]}{SUFFIX}"


def _options(**overrides: object) -> BatchOptions:
    values: dict[str, object] = {
        "dry_run": False,
        "existing": "error",
        "fail_fast": False,
        "output": None,
        "output_dir": None,
    }
    values.update(overrides)
    return BatchOptions(**values)  # type: ignore[arg-type]


def test_a_legal_250_byte_source_gets_a_legal_output_and_reruns_verify(
    tmp_path: Path,
) -> None:
    """The audit's case: default naming works, is stable, and never collides."""
    first = tmp_path / ("f" * 246 + ".eml")
    second = tmp_path / ("f" * 245 + "g.eml")
    for source in (first, second):
        source.write_bytes(MESSAGE)
    created = execute([str(first), str(second)], _options())
    assert [item.status for item in created.items] == [ItemStatus.CREATED] * 2
    outputs = sorted(path.name for path in tmp_path.glob("*.mime-pruned.eml"))
    assert len(outputs) == 2
    assert all(len(name) <= 255 for name in outputs)
    assert all(name.startswith("f" * 200) for name in outputs)
    verified = execute([str(first), str(second)], _options(existing="verify"))
    assert [item.status for item in verified.items] == [
        ItemStatus.EXISTING_VERIFIED
    ] * 2
    conflict = execute([str(first)], _options())
    assert conflict.items[0].status is ItemStatus.FAILED
    assert conflict.items[0].error is not None
    assert conflict.items[0].error.code is ExitCode.OUTPUT_CONFLICT


def test_output_directory_names_are_fitted_to_that_directory(tmp_path: Path) -> None:
    """--output-dir gets a fitted name, whatever the source directory allows."""
    source = tmp_path / ("d" * 250 + ".eml")
    source.write_bytes(MESSAGE)
    target = tmp_path / "out"
    target.mkdir()
    ledger = execute([str(source)], _options(output_dir=str(target)))
    assert ledger.items[0].status is ItemStatus.CREATED
    (created,) = target.iterdir()
    assert len(created.name) <= 255
    assert created.read_bytes() == MESSAGE


def test_explicit_output_is_never_renamed(tmp_path: Path) -> None:
    """An explicit destination that is too long is a clear item failure, unaltered."""
    source = tmp_path / "source.eml"
    source.write_bytes(MESSAGE)
    explicit = tmp_path / ("e" * 300 + ".eml")
    ledger = execute([str(source)], _options(output=str(explicit)))
    assert ledger.items[0].status is ItemStatus.FAILED
    assert ledger.items[0].error is not None
    # The kernel's own refusal is surfaced; its class differs between hosts.
    assert ledger.items[0].error.code in {ExitCode.WRITE_ERROR, ExitCode.INPUT_ERROR}
    assert list(tmp_path.iterdir()) == [source]


def test_dry_run_reports_the_fitted_destination(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    """The planned destination names the copy that would really be created."""
    source = tmp_path / ("h" * 250 + ".eml")
    source.write_bytes(MESSAGE)
    arguments = ["--dry-run", "--output-format", "json", "--", str(source)]
    assert cli.main(arguments) == 0
    destination = json.loads(capsys.readouterr().out)["items"][0]["destination"]
    name = "h" * (255 - len(_tail(source.name))) + _tail(source.name)
    posix, windows = (
        destination["basename_base64"],
        destination["basename_utf16le_base64"],
    )
    actual = (
        base64.b64decode(posix).decode()
        if posix is not None
        else base64.b64decode(windows).decode("utf-16-le")
    )
    assert actual == name
    assert list(tmp_path.iterdir()) == [source]
