"""Verify that private macOS runtimes publish only after completed signing."""

from __future__ import annotations

from contextlib import nullcontext
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

import pytest
from tools import macos_runtime, macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture

if TYPE_CHECKING:
    from pathlib import Path


@dataclass(slots=True)
class PublicationFixture:
    """Record each authentication, preparation, and publication boundary."""

    root: Path
    archive: Path
    pin: macos_runtime_source.RuntimePin
    source_mode: str
    sign_failure: bool
    events: list[str] = field(default_factory=list)

    @property
    def target(self) -> Path:
        """Return the previously absent publication destination.

        Returns:
            A destination separate from the private staging directory.

        """
        return self.root / "not-yet-created" / "Runtime"

    def select(self, architecture: str) -> macos_runtime_source.RuntimePin:
        """Select the authenticated source without changing its identity.

        Returns:
            The selected pinned source identity.

        """
        assert architecture == "arm64"
        self.events.append("pin")
        return self.pin

    def supplied(self, candidate: macos_runtime_source.RuntimePin) -> Path | None:
        """Optionally supply a previously authenticated archive.

        Returns:
            The cached source path, or None for fresh download.

        """
        assert candidate is self.pin
        assert self.source_mode != "explicit"
        self.events.append("supplied")
        return self.archive if self.source_mode == "cached" else None

    def download(self, path: Path, candidate: macos_runtime_source.RuntimePin) -> None:
        """Check authenticated fresh acquisition occurs only in staging."""
        assert self.source_mode == "download"
        assert candidate is self.pin
        assert path.name == "source.tar.zst"
        assert path.parent.parent == self.target.parent
        self.events.append("download")
        path.write_bytes(self.archive.read_bytes())

    def extract(
        self,
        path: Path,
        destination: Path,
        candidate: macos_runtime_source.RuntimePin,
    ) -> Path:
        """Return a synthetic extracted source in the correct staging tree.

        Returns:
            The created source directory in private staging.

        """
        assert candidate is self.pin
        if self.source_mode == "download":
            assert path.name == "source.tar.zst"
        else:
            assert path == self.archive
        assert path.read_bytes() == self.archive.read_bytes()
        assert destination.name == "source"
        assert destination.parent.parent == self.target.parent
        self.events.append("extract")
        destination.mkdir()
        return destination

    def copy(self, source: Path, staging: Path, compiler: Path) -> None:
        """Populate a stage with a synthetic native artifact."""
        assert source.name == "source"
        assert staging == source.parent / "Runtime"
        assert compiler == self.root / "compiler"
        self.events.append("copy")
        staging.mkdir()
        (staging / "native-binary").write_bytes(b"verified synthetic runtime")

    def native(self, staging: Path, architecture: str) -> list[Path]:
        """Confirm native validation sees staged, never published bytes.

        Returns:
            A list of staged synthetic native-code paths.

        """
        assert architecture == "arm64"
        assert (staging / "native-binary").read_bytes() == b"verified synthetic runtime"
        assert not self.target.exists()
        self.events.append("native")
        return [staging / "native-binary"]

    def sign(self, paths: list[Path]) -> None:
        """Verify that signing finishes before the destination appears."""
        assert len(paths) == 1
        assert paths[0].name == "native-binary"
        assert paths[0].read_bytes() == b"verified synthetic runtime"
        assert not self.target.exists()
        self.events.append("sign")
        if self.sign_failure:
            message = "synthetic signature failure"
            raise ValueError(message)

    def install(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """Replace only external boundaries, not the publication logic."""
        monkeypatch.setattr(macos_runtime_source, "pin", self.select)
        monkeypatch.setattr(macos_runtime_source, "supplied_archive", self.supplied)
        monkeypatch.setattr(macos_runtime_source, "download", self.download)
        monkeypatch.setattr(macos_runtime_source, "extract", self.extract)
        monkeypatch.setattr(
            macos_runtime_source,
            "pinned_compiler",
            lambda _source, _architecture: nullcontext(self.root / "compiler"),
        )
        monkeypatch.setattr(macos_runtime, "copy_install", self.copy)
        monkeypatch.setattr(macos_runtime, "require_native", self.native)
        monkeypatch.setattr(macos_runtime, "_sign", self.sign)

    def expected_events(self) -> list[str]:
        """Describe acquisition and publication in observable order.

        Returns:
            The exact expected order of boundary invocations.

        """
        events = ["pin"]
        if self.source_mode != "explicit":
            events.append("supplied")
        if self.source_mode == "download":
            events.append("download")
        return [*events, "extract", "copy", "native", "sign"]


@pytest.mark.parametrize("source_mode", ["explicit", "cached", "download"])
@pytest.mark.parametrize("sign_failure", [False, True])
def test_authenticated_runtime_publication_is_transactional(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    source_mode: str,
    *,
    sign_failure: bool,
) -> None:
    """Successful signing publishes; signing failures leave neither output nor stage."""
    archive, pin = runtime_archive_fixture(tmp_path)
    fixture = PublicationFixture(tmp_path, archive, pin, source_mode, sign_failure)
    fixture.install(monkeypatch)
    selected_archive = archive if source_mode == "explicit" else None
    if sign_failure:
        with pytest.raises(ValueError, match=r"^synthetic signature failure$"):
            macos_runtime.prepare(fixture.target, "arm64", selected_archive)
        assert not fixture.target.exists()
        assert list(fixture.target.parent.iterdir()) == []
    else:
        assert (
            macos_runtime.prepare(fixture.target, "arm64", selected_archive)
            == fixture.target
        )
        assert (
            fixture.target / "native-binary"
        ).read_bytes() == b"verified synthetic runtime"
        assert list(fixture.target.parent.iterdir()) == [fixture.target]
    assert fixture.events == fixture.expected_events()
