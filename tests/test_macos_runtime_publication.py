"""Verify that a private macOS runtime is published only after complete signing."""

from __future__ import annotations

from contextlib import nullcontext
from pathlib import Path

import pytest
from tools import macos_runtime, macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture


@pytest.mark.parametrize("source_mode", ("explicit", "cached", "download"))
@pytest.mark.parametrize("sign_failure", (False, True))
def test_authenticated_runtime_publication_is_transactional(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    source_mode: str,
    *,
    sign_failure: bool,
) -> None:
    """Only a signed staging tree becomes the destination; failures leave none."""
    archive, pin = runtime_archive_fixture(tmp_path)
    archive_bytes = archive.read_bytes()
    target = tmp_path / "not-yet-created" / "Runtime"
    events: list[str] = []

    def select(architecture: str) -> macos_runtime_source.RuntimePin:
        assert architecture == "arm64"
        events.append("pin")
        return pin

    def supplied(candidate: macos_runtime_source.RuntimePin) -> Path | None:
        assert candidate is pin
        assert source_mode != "explicit"
        events.append("supplied")
        return archive if source_mode == "cached" else None

    def download(path: Path, candidate: macos_runtime_source.RuntimePin) -> None:
        assert source_mode == "download" and candidate is pin
        assert path.name == "source.tar.zst" and path.parent.parent == target.parent
        events.append("download")
        path.write_bytes(archive_bytes)

    def extract(
        path: Path,
        destination: Path,
        candidate: macos_runtime_source.RuntimePin,
    ) -> Path:
        assert candidate is pin
        if source_mode == "download":
            assert path.name == "source.tar.zst"
        else:
            assert path == archive
        assert path.read_bytes() == archive_bytes
        assert destination.name == "source"
        assert destination.parent.parent == target.parent
        events.append("extract")
        destination.mkdir()
        return destination

    def copy(source: Path, staging: Path, compiler: Path) -> None:
        assert source.name == "source" and staging == source.parent / "Runtime"
        assert compiler == tmp_path / "compiler"
        events.append("copy")
        staging.mkdir()
        (staging / "native-binary").write_bytes(b"verified synthetic runtime")

    def native(staging: Path, architecture: str) -> list[Path]:
        assert architecture == "arm64"
        assert (staging / "native-binary").read_bytes() == b"verified synthetic runtime"
        assert not target.exists()
        events.append("native")
        return [staging / "native-binary"]

    def sign(paths: list[Path]) -> None:
        assert len(paths) == 1 and paths[0].name == "native-binary"
        assert paths[0].read_bytes() == b"verified synthetic runtime"
        assert not target.exists()
        events.append("sign")
        if sign_failure:
            raise ValueError("synthetic signature failure")

    monkeypatch.setattr(macos_runtime_source, "pin", select)
    monkeypatch.setattr(macos_runtime_source, "supplied_archive", supplied)
    monkeypatch.setattr(macos_runtime_source, "download", download)
    monkeypatch.setattr(macos_runtime_source, "extract", extract)
    monkeypatch.setattr(
        macos_runtime_source,
        "pinned_compiler",
        lambda _source, _architecture: nullcontext(tmp_path / "compiler"),
    )
    monkeypatch.setattr(macos_runtime, "copy_install", copy)
    monkeypatch.setattr(macos_runtime, "require_native", native)
    monkeypatch.setattr(macos_runtime, "_sign", sign)

    selected_archive = archive if source_mode == "explicit" else None
    if sign_failure:
        with pytest.raises(ValueError, match="^synthetic signature failure$"):
            macos_runtime.prepare(target, "arm64", selected_archive)
        assert not target.exists()
    else:
        assert macos_runtime.prepare(target, "arm64", selected_archive) == target
        assert (target / "native-binary").read_bytes() == b"verified synthetic runtime"

    assert list(target.parent.iterdir()) == ([] if sign_failure else [target])
    expected = ["pin"]
    if source_mode != "explicit":
        expected.append("supplied")
    if source_mode == "download":
        expected.append("download")
    assert events == [*expected, "extract", "copy", "native", "sign"]
