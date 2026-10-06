"""Archive metadata budgets and runtime links fail before file materialization."""

from __future__ import annotations

import os
import plistlib
import stat
import zipfile
from typing import TYPE_CHECKING

import pytest
from tools import macos_archive
from tools.release_files import ReleaseQualificationError

if TYPE_CHECKING:
    from pathlib import Path


@pytest.mark.parametrize("mode", ["unknown", "missing", "oversized", "empty"])
def test_invalid_edition_or_metadata_size_is_refused_before_reconstruction(
    tmp_path: Path, mode: str
) -> None:
    """Invalid declarations cannot select or download a trusted runtime reference."""
    data = plistlib.dumps({"EMLRuntimeMode": mode, "EMLArchitecture": "arm64"})
    if mode == "oversized":
        data = b"x" * (macos_archive.MAX_BUNDLE_INFO + 1)
    elif mode == "empty":
        data = b""
    archive = tmp_path / "invalid.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
        output.writestr(macos_archive.APP + "/Contents/Info.plist", data)
    message = (
        "Native bundle metadata exceeds its read budget"
        if mode in {"oversized", "empty"}
        else "Native runtime edition metadata is invalid"
    )
    with pytest.raises(ReleaseQualificationError, match="^" + message + "$"):
        macos_archive.verify(archive, tmp_path / "unused.pyz", "4.0.0", "arm64")


def test_one_byte_bundle_metadata_fails_as_invalid_content_within_the_read_budget(
    tmp_path: Path,
) -> None:
    archive = tmp_path / "invalid-content.zip"
    with zipfile.ZipFile(archive, "w") as output:
        output.writestr(macos_archive.APP + "/Contents/Info.plist", b"x")
    with (
        zipfile.ZipFile(archive) as source,
        pytest.raises(plistlib.InvalidFileException),
    ):
        macos_archive.__dict__["_archive_identity"](source)


@pytest.mark.parametrize("trusted_reference", [False, True])
def test_runtime_link_requires_its_trusted_literal_target(
    tmp_path: Path, *, trusted_reference: bool
) -> None:
    """Neither no reference nor an attacker-controlled link target is approved."""
    runtime = tmp_path / "Runtime"
    runtime.mkdir()
    (runtime / "program").write_bytes(b"original runtime resource")
    (runtime / "alias").symlink_to("program")
    name = macos_archive.RUNTIME_PREFIX + "/alias"
    surface = {name: stat.S_IFLNK | 0o777}
    archive = tmp_path / "forged-link.zip"
    with zipfile.ZipFile(archive, "w") as output:
        output.writestr(name, b"../outside")
    message = (
        "Native runtime link target differs from source"
        if trusted_reference
        else "Native archive links require a trusted runtime reference"
    )
    with (
        zipfile.ZipFile(archive) as source,
        pytest.raises(ReleaseQualificationError, match="^" + message + "$"),
    ):
        macos_archive.__dict__["_validated_links"](
            source, surface, runtime if trusted_reference else None
        )
    assert (runtime / "alias").readlink().as_posix() == "program"
    assert (runtime / "program").read_bytes() == b"original runtime resource"


@pytest.mark.parametrize("supports_no_follow", [False, True])
def test_timestamp_restore_never_follows_links(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, supports_no_follow: bool
) -> None:
    target = tmp_path / "program"
    target.write_bytes(b"preserved")
    (tmp_path / "alias").symlink_to("program")
    calls: list[tuple[str, tuple[int, int], bool]] = []

    def stamp(path: Path, times: tuple[int, int], *, follow_symlinks: bool) -> None:
        assert type(follow_symlinks) is bool
        calls.append((path.name, times, follow_symlinks))

    monkeypatch.setattr(os, "utime", stamp)
    monkeypatch.setattr(
        os, "supports_follow_symlinks", {stamp} if supports_no_follow else set()
    )
    macos_archive.__dict__["_restore_times"](
        tmp_path, [zipfile.ZipInfo("program"), zipfile.ZipInfo("alias")], 1_700_000_000
    )
    assert calls == [
        ("alias", (1_700_000_000, 1_700_000_000), False),
        ("program", (1_700_000_000, 1_700_000_000), not supports_no_follow),
    ]
    assert target.read_bytes() == b"preserved"


def test_valid_bundle_metadata_includes_the_exact_read_budget(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    data = plistlib.dumps({"EMLRuntimeMode": "bundled", "EMLArchitecture": "x86_64"})
    path = tmp_path / "metadata.zip"
    with zipfile.ZipFile(path, "w") as output:
        output.writestr(macos_archive.APP + "/Contents/Info.plist", data)
    monkeypatch.setattr(macos_archive, "MAX_BUNDLE_INFO", len(data))
    with zipfile.ZipFile(path) as source:
        assert macos_archive.__dict__["_archive_identity"](source) == (
            "bundled",
            "x86_64",
        )
        monkeypatch.setattr(macos_archive, "MAX_BUNDLE_INFO", len(data) - 1)
        with pytest.raises(ReleaseQualificationError) as caught:
            macos_archive.__dict__["_archive_identity"](source)
    assert str(caught.value) == "Native bundle metadata exceeds its read budget"
