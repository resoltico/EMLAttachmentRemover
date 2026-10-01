"""Real copies survive first delivery-read faults without restarting partial output."""

from __future__ import annotations

import errno
import json
import sys
from email import policy
from email.parser import BytesParser
from io import BytesIO, TextIOWrapper
from typing import IO, TYPE_CHECKING, Any, override

import pytest

from eml_attachment_remover import cli, report_delivery, report_session
from eml_attachment_remover.report_delivery import StagedChannels
from tests.live_report_support import MESSAGE, inputs

if TYPE_CHECKING:
    from contextlib import ExitStack
    from pathlib import Path
    from typing import TextIO


class _ReadFault:
    """Prime once during sealing, then fail at a chosen delivery read."""

    def __init__(self, real: IO[Any], call: int) -> None:
        self.real = real
        self.call = call
        self.reads = 0

    def __getattr__(self, name: str) -> object:
        return getattr(self.real, name)

    def check(self) -> None:
        self.reads += 1
        if self.reads == self.call:
            raise OSError(errno.EIO, "public staged-read failure")

    def read(self, count: int) -> object:
        self.check()
        return self.real.read(count)


class _TextFault:
    def __init__(self, real: TextIO, call: int) -> None:
        self.real = real
        self.buffer = _ReadFault(real.buffer, call)

    def __getattr__(self, name: str) -> object:
        return getattr(self.real, name)

    def read(self, count: int) -> str:
        self.buffer.check()
        return self.real.read(count)


def _inject(
    monkeypatch: pytest.MonkeyPatch, call: int, *, stderr: bool = False
) -> None:
    original = StagedChannels.open

    def opened(
        _cls: type[StagedChannels], resources: ExitStack, fmt: str
    ) -> StagedChannels:
        channels = original(resources, fmt)
        return StagedChannels(
            channels.out if stderr else _TextFault(channels.out, call),  # type: ignore[arg-type]
            _TextFault(channels.err, call) if stderr else channels.err,  # type: ignore[arg-type]
        )

    monkeypatch.setattr(StagedChannels, "open", classmethod(opened))


def _verify_copies(root: Path, sources: list[str]) -> None:
    for source in sources:
        assert (root / source.rsplit("/", 1)[-1]).read_bytes() == MESSAGE
    copies = sorted(root.glob("*.mime-pruned.eml"))
    assert len(copies) == 2
    for copy in copies:
        parsed = BytesParser(policy=policy.default).parsebytes(copy.read_bytes())
        assert parsed.get_body().get_content().strip() == "public body"  # type: ignore[union-attr]
        assert not list(parsed.iter_attachments())


@pytest.mark.parametrize("fmt", ["json", "human", "paths0"])
@pytest.mark.parametrize("warning", [False, True])
def test_first_report_read_failure_recovers_real_processing_outcomes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    fmt: str,
    *,
    warning: bool,
) -> None:
    sources = inputs(tmp_path)
    _inject(monkeypatch, 2)
    original = report_session._write_selected  # ruff: ignore[private-member-access] - full render.

    def rendered(*args: object) -> None:
        original(*args)  # type: ignore[arg-type]
        if warning:
            sys.stderr.write("public warning\n")

    monkeypatch.setattr(report_session, "_write_selected", rendered)
    assert cli.main(["--output-format", fmt, *sources]) == 7
    captured = capsys.readouterr()
    if fmt == "json":
        document = json.loads(captured.out)
        assert document["summary"]["created"] == 2
        assert document["batch_error"]["code"] == "WRITE_ERROR"
        assert document["exit_code"] == 7
    elif fmt == "human":
        assert captured.out.count("created:") == 2
        assert captured.err.count("Batch: WRITE_ERROR:") == 1
    else:
        assert captured.out.count("\0") == 2
        assert captured.err.count("Batch: WRITE_ERROR:") == 1
    assert captured.err.count("public warning") == int(warning)
    if warning:
        if fmt == "json":
            assert captured.err == "public warning\n"
        else:
            assert "public warning\nBatch: WRITE_ERROR:" in captured.err
    _verify_copies(tmp_path, sources)


def test_partial_json_read_failure_never_emits_a_replacement(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    sources = inputs(tmp_path)
    _inject(monkeypatch, 3)
    monkeypatch.setattr(report_delivery, "CHUNK_SIZE", 16)
    assert cli.main(["--output-format=json", *sources]) == 70
    captured = capsys.readouterr()
    assert len(captured.out) == 16
    assert captured.out == '{"batch_error": '
    assert captured.err == (
        "remove-eml-attachments: error[INTERNAL_ERROR:70]: "
        "could not read staged report\n"
    )
    _verify_copies(tmp_path, sources)


@pytest.mark.parametrize("fmt", ["json", "paths0"])
def test_recovery_delivery_preserves_bytes_on_a_utf16_terminal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, fmt: str
) -> None:
    sources = inputs(tmp_path)
    _inject(monkeypatch, 2)
    raw = BytesIO()
    with TextIOWrapper(raw, encoding="utf-16") as terminal:
        monkeypatch.setattr(sys, "stdout", terminal)
        assert cli.main(["--output-format", fmt, *sources]) == 7
        if fmt == "json":
            assert json.loads(raw.getvalue().decode("ascii"))["summary"]["created"] == 2
        else:
            expected = b"".join(
                str(path).encode() + b"\0"
                for path in sorted(tmp_path.glob("*.mime-pruned.eml"))
            )
            assert raw.getvalue() == expected


def test_partial_stderr_does_not_preclude_an_unstarted_report_recovery(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    sources = inputs(tmp_path)
    _inject(monkeypatch, 3, stderr=True)
    monkeypatch.setattr(report_delivery, "CHUNK_SIZE", 16)
    original = report_session._write_selected  # ruff: ignore[private-member-access] - real render.

    def rendered(*args: object) -> None:
        original(*args)  # type: ignore[arg-type]
        sys.stderr.write("public warning continuing on the same line\n")

    monkeypatch.setattr(report_session, "_write_selected", rendered)
    assert cli.main(["--output-format=human", *sources]) == 7
    captured = capsys.readouterr()
    assert captured.out.count("created:") == 2
    assert len(captured.err.splitlines()[0]) == 16
    assert captured.err.splitlines()[1].startswith("Batch:")
    assert captured.err.count("Batch:") == 1
    _verify_copies(tmp_path, sources)


def test_broken_stdout_is_not_retried_as_a_staging_failure(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    class Closed(BytesIO):
        @override
        def write(self, _payload: object) -> int:
            raise BrokenPipeError

    class Output:
        encoding = "utf-8"
        buffer = Closed()

    sources = inputs(tmp_path)
    monkeypatch.setattr(sys, "stdout", Output())
    assert cli.main(["--output-format=json", *sources]) == 1
    assert not Output.buffer.getvalue()
    _verify_copies(tmp_path, sources)
