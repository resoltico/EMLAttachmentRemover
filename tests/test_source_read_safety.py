# ruff: file-ignore[private-member-access]
"""Contracts for binding source validation to the descriptor being read."""

from __future__ import annotations

import os
import stat
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pytest

from eml_attachment_remover import models, processing


def test_source_reader_inspects_the_open_descriptor_and_reads_exact_bytes() -> None:
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "public.eml"
        source.write_bytes(b"PUBLIC\r\n")

        assert processing._read_source(source) == b"PUBLIC\r\n"


def test_source_reader_rejects_a_nonregular_descriptor_without_blocking() -> None:
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "public.eml"
        source.write_bytes(b"PUBLIC")

        def special_metadata(_descriptor: int) -> SimpleNamespace:
            return SimpleNamespace(st_mode=stat.S_IFIFO | stat.S_IRUSR)

        with patch.object(os, "fstat", side_effect=special_metadata):
            with pytest.raises(models.CliError) as raised:
                processing._read_source(source)

    assert raised.value.code is models.ExitCode.INPUT_ERROR
    assert "not a regular file at read time" in raised.value.message


def test_source_reader_requests_nonblocking_binary_open_when_available() -> None:
    source = Path("public.eml")
    with patch.object(os, "open", side_effect=OSError("PUBLIC FLAGS")) as open_:
        with pytest.raises(models.CliError):
            processing._read_source(source)

    flags = open_.call_args.args[1]
    assert flags & os.O_RDONLY == os.O_RDONLY
    assert flags & getattr(os, "O_NONBLOCK", 0) == getattr(os, "O_NONBLOCK", 0)
    assert flags & getattr(os, "O_BINARY", 0) == getattr(os, "O_BINARY", 0)


def test_source_reader_uses_zero_for_unavailable_portable_flags() -> None:
    source = Path("public.eml")
    calls: list[tuple[Path, int]] = []

    def fail_open(path: Path, flags: int) -> int:
        calls.append((path, flags))
        raise OSError

    portable_os = SimpleNamespace(O_RDONLY=4, O_BINARY=8, open=fail_open)
    with (
        patch.object(processing, "os", portable_os),
        pytest.raises(models.CliError),
    ):
        processing._read_source(source)

    assert calls == [(source, 12)]
