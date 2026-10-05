"""Runtime ZIP approval derives from trusted upstream source."""

from __future__ import annotations

import os
import platform
import shutil
import stat
import subprocess
import sys
from typing import TYPE_CHECKING

import pytest
from tools import macos_runtime, macos_runtime_archive

if TYPE_CHECKING:
    from pathlib import Path


def _tree(root: Path) -> None:
    root.mkdir()
    (root / "licenses").mkdir()
    (root / "licenses/NOTICE").write_bytes(b"trusted notice")
    (root / "stdlib.py").write_bytes(b"trusted source")
    for path in root.rglob("*"):
        path.chmod(0o755 if path.is_dir() else 0o644)


@pytest.mark.parametrize("change", ["added", "removed", "modified", "mode"])
def test_self_declared_runtime_changes_cannot_expand_source_approval(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    change: str,
) -> None:
    """Delivered declarations cannot replace the source-derived resource contract."""
    expected = tmp_path / "expected"
    _tree(expected)
    actual = tmp_path / "actual"
    shutil.copytree(expected, actual)
    monkeypatch.setattr(macos_runtime, "require_native", lambda _root, _arch: [])
    if change == "added":
        (actual / "extra.py").write_bytes(b"unapproved")
    elif change == "removed":
        (actual / "licenses/NOTICE").unlink()
    elif change == "modified":
        (actual / "stdlib.py").write_bytes(b"altered source")
    else:
        (actual / "stdlib.py").chmod(0o755)
    with pytest.raises(ValueError, match="differs"):
        macos_runtime_archive.verify(actual, expected, "arm64")


def test_source_reference_approves_exact_resources(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Matching trees retain complete type/mode and literal resource checks."""
    expected = tmp_path / "expected"
    _tree(expected)
    actual = tmp_path / "actual"
    shutil.copytree(expected, actual)
    monkeypatch.setattr(macos_runtime, "require_native", lambda _root, _arch: [])
    macos_runtime_archive.verify(actual, expected, "arm64")
    surface = macos_runtime_archive.surface(actual, "Runtime")
    assert surface["Runtime/stdlib.py"] == stat.S_IFREG | 0o644
    assert surface["Runtime/licenses/"] == stat.S_IFDIR | 0o755


@pytest.mark.skipif(os.name == "nt", reason="POSIX symlink mode")
def test_an_internal_but_forged_link_is_not_accepted(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Internal links must still match the target declared by trusted source."""
    expected = tmp_path / "expected"
    _tree(expected)
    (expected / "link").symlink_to("stdlib.py")
    actual = tmp_path / "actual"
    shutil.copytree(expected, actual, symlinks=True)
    (actual / "link").unlink()
    (actual / "link").symlink_to("licenses/NOTICE")
    monkeypatch.setattr(macos_runtime, "require_native", lambda _root, _arch: [])
    with pytest.raises(ValueError, match="differs"):
        macos_runtime_archive.verify(actual, expected, "arm64")


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS code-signing boundary")
def test_valid_signatures_do_not_approve_changed_native_code(tmp_path: Path) -> None:
    """Canonical comparison accepts re-signing but refuses a different program."""
    expected = tmp_path / "expected"
    expected.mkdir()
    source = tmp_path / "program.c"
    source.write_text("int main(void) { return 0; }\n")
    program = expected / "program"
    subprocess.run(
        [
            "/usr/bin/xcrun",
            "clang",
            "-mmacosx-version-min=14.0",
            str(source),
            "-o",
            str(program),
        ],
        check=True,
        timeout=30,
    )
    actual = tmp_path / "actual"
    shutil.copytree(expected, actual)
    delivered = actual / "program"
    subprocess.run(
        [
            "/usr/bin/codesign",
            "--force",
            "--sign",
            "-",
            "-i",
            "other.identity",
            str(delivered),
        ],
        check=True,
        timeout=30,
    )
    macos_runtime_archive.verify(actual, expected, platform.machine())
    source.write_text("int main(void) { return 1; }\n")
    subprocess.run(
        [
            "/usr/bin/xcrun",
            "clang",
            "-mmacosx-version-min=14.0",
            str(source),
            "-o",
            str(delivered),
        ],
        check=True,
        timeout=30,
    )
    subprocess.run(
        ["/usr/bin/codesign", "--force", "--sign", "-", str(delivered)],
        check=True,
        timeout=30,
    )
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--strict", str(delivered)],
        check=True,
        timeout=30,
    )
    with pytest.raises(ValueError, match="native code differs"):
        macos_runtime_archive.verify(actual, expected, platform.machine())
