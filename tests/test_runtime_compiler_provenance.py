"""Authenticated compiler selection is independent of host Python discovery."""

from __future__ import annotations

import platform
import sys
from pathlib import Path

import pytest
from tools import macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture


def test_native_pinned_compiler_never_uses_invoking_python(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """The verified source's own interpreter is the compiler authority."""
    monkeypatch.setattr(platform, "machine", lambda: "arm64")
    source = tmp_path / "authenticated-source"
    interpreter = source / "install/bin/python3.14"
    interpreter.parent.mkdir(parents=True)
    interpreter.write_bytes(b"authenticated interpreter")
    with macos_runtime_source.pinned_compiler(source, "arm64") as chosen:
        assert chosen == interpreter
        assert str(chosen) != sys.executable


@pytest.mark.parametrize(
    ("machine", "architecture"),
    [("unknown", "arm64"), ("arm64", "unknown")],
)
def test_pinned_compiler_rejects_unsupported_cpu(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    machine: str,
    architecture: str,
) -> None:
    """Never fall back to an ambient interpreter on an unexpected CPU."""
    monkeypatch.setattr(platform, "machine", lambda: machine)
    with (
        pytest.raises(ValueError, match="supported macOS CPU"),
        macos_runtime_source.pinned_compiler(tmp_path, architecture),
    ):
        pytest.fail("unknown CPU should have failed before yielding")


def test_cross_cpu_compiler_uses_an_independently_verified_archive(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Host-native cross-target compilation must reject altered cached bytes."""
    archive, selected = runtime_archive_fixture(tmp_path)
    monkeypatch.setattr(platform, "machine", lambda: "arm64")
    monkeypatch.setenv("EML_RUNTIME_SOURCE_DIRECTORY", str(tmp_path))
    monkeypatch.setattr(macos_runtime_source, "pin", lambda _arch: selected)
    with macos_runtime_source.pinned_compiler(tmp_path, "x86_64") as chosen:
        assert chosen.read_bytes() == b"test interpreter"
    corrupted = bytearray(archive.read_bytes())
    corrupted[-1] ^= 1
    archive.write_bytes(corrupted)
    with (
        pytest.raises(ValueError, match="digest"),
        macos_runtime_source.pinned_compiler(tmp_path, "x86_64"),
    ):
        pytest.fail("altered pinned compiler should never be yielded")


def test_cross_cpu_compiler_downloads_authenticated_source_when_uncached(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A cold cache still requires the independent pin and metadata."""
    archive, selected = runtime_archive_fixture(tmp_path)
    monkeypatch.setattr(platform, "machine", lambda: "arm64")
    monkeypatch.delenv("EML_RUNTIME_SOURCE_DIRECTORY", raising=False)
    monkeypatch.setattr(macos_runtime_source, "pin", lambda _arch: selected)
    requests: list[Path] = []

    def download(destination: Path, _selected: macos_runtime_source.RuntimePin) -> None:
        requests.append(destination)
        destination.write_bytes(archive.read_bytes())

    monkeypatch.setattr(macos_runtime_source, "download", download)
    with macos_runtime_source.pinned_compiler(tmp_path, "x86_64") as chosen:
        assert chosen.read_bytes() == b"test interpreter"
    assert len(requests) == 1
    assert not requests[0].exists()


