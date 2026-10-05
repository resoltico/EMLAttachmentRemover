"""Build datetime contracts at real archive and extraction boundaries."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tarfile
import time
import zipfile
from pathlib import Path
from types import SimpleNamespace
from typing import cast

import pytest
from tools import build_backend, build_timestamp, build_zipapp
from tools.hatch_build import CustomBuildHook

ROOT = Path(__file__).resolve().parents[1]


def test_build_time_is_current_and_unix_field_is_exact(tmp_path: Path) -> None:
    assert abs(build_timestamp.EPOCH - time.time()) < 3600
    assert build_timestamp.EPOCH % 2 == 0
    assert time.localtime(build_timestamp.EPOCH)[:6] == build_timestamp.ZIP_TIME
    assert build_timestamp.zip_extra().hex().startswith("5554050001")
    child = tmp_path / "folder" / "file"
    child.parent.mkdir()
    child.write_text("example")
    build_timestamp.stamp_tree(tmp_path)
    assert {path.stat().st_mtime for path in (tmp_path, child.parent, child)} == {
        build_timestamp.EPOCH
    }
    assert build_backend.build_sdist.__module__ == "tools.build_backend"


@pytest.mark.parametrize("timezone", [None, "UTC"])
def test_real_source_wheel_and_zipapp_use_one_build_time(
    tmp_path: Path, timezone: str | None
) -> None:
    environment = {
        **os.environ,
        **build_timestamp.environment(),
        "PYTHONDONTWRITEBYTECODE": "1",
    }
    if timezone is not None:
        environment["TZ"] = timezone
    expected_time = (
        time.gmtime(build_timestamp.EPOCH)[:6]
        if timezone == "UTC"
        else build_timestamp.ZIP_TIME
    )
    executable = shutil.which("uv")
    assert executable is not None
    subprocess.run(
        [executable, "build", "--no-build-isolation", "--out-dir", str(tmp_path)],
        cwd=ROOT,
        env=environment,
        check=True,
        capture_output=True,
        timeout=60,
    )
    target = tmp_path / "processor.pyz"
    subprocess.run(
        [
            sys.executable,
            "-B",
            str(ROOT / "tools/build_zipapp.py"),
            "--target",
            str(target),
        ],
        cwd=ROOT,
        env=environment,
        check=True,
        capture_output=True,
        timeout=60,
    )
    source = next(tmp_path.glob("*.tar.gz"))
    with tarfile.open(source) as archive:
        members = archive.getmembers()
        assert any(member.isdir() for member in members)
        assert {member.mtime for member in members} == {build_timestamp.EPOCH}
        extracted = tmp_path / "source"
        archive.extractall(extracted, filter="data")
    assert all(
        path.stat().st_mtime == build_timestamp.EPOCH for path in extracted.rglob("*")
    )
    for artifact in (next(tmp_path.glob("*.whl")), target):
        with zipfile.ZipFile(artifact) as archive:
            assert any(member.is_dir() for member in archive.infolist())
            assert {member.date_time for member in archive.infolist()} == {
                expected_time
            }
            assert {member.extra for member in archive.infolist()} == {
                build_timestamp.zip_extra()
            }
        if sys.platform == "darwin":
            extracted_zip = tmp_path / (artifact.name + "-extracted")
            if artifact.suffix == ".pyz":
                subprocess.run(
                    ["/usr/bin/unzip", "-qq", str(artifact), "-d", str(extracted_zip)],
                    check=True,
                    capture_output=True,
                    timeout=10,
                )
            else:
                subprocess.run(
                    ["/usr/bin/ditto", "-x", "-k", str(artifact), str(extracted_zip)],
                    check=True,
                    timeout=10,
                )
            assert {path.stat().st_mtime for path in extracted_zip.rglob("*")} == {
                build_timestamp.EPOCH
            }


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS extraction boundary")
@pytest.mark.parametrize("utc_header", [False, True])
def test_macos_extractor_restores_local_datetime(
    tmp_path: Path, *, utc_header: bool
) -> None:
    archive = tmp_path / "example.zip"
    stamp = (
        time.gmtime(build_timestamp.EPOCH)[:6]
        if utc_header
        else build_timestamp.ZIP_TIME
    )
    info = zipfile.ZipInfo("folder/", stamp)
    info.extra = build_timestamp.zip_extra()
    file = zipfile.ZipInfo("folder/example.txt", stamp)
    file.extra = build_timestamp.zip_extra()
    with zipfile.ZipFile(archive, "w") as output:
        output.writestr(info, b"")
        output.writestr(file, b"example")
    extracted = tmp_path / "extracted"
    subprocess.run(
        ["/usr/bin/ditto", "-x", "-k", str(archive), str(extracted)],
        check=True,
        timeout=10,
    )
    assert (extracted / "folder/example.txt").stat().st_mtime == build_timestamp.EPOCH
    assert (extracted / "folder").stat().st_mtime == build_timestamp.EPOCH


def test_build_hook_ignores_other_artifact_targets(tmp_path: Path) -> None:
    hook = cast("CustomBuildHook", SimpleNamespace(target_name="other"))
    CustomBuildHook.finalize(hook, "standard", {}, str(tmp_path / "absent"))


def test_wheel_hook_preserves_contents_and_restores_absolute_datetime(
    tmp_path: Path,
) -> None:
    path = tmp_path / "example.whl"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("package/file.py", b"content")
    hook = cast("CustomBuildHook", SimpleNamespace(target_name="wheel"))
    CustomBuildHook.finalize(hook, "standard", {}, str(path))
    with zipfile.ZipFile(path) as archive:
        assert set(archive.namelist()) == {"package/", "package/file.py"}
        directory = archive.getinfo("package/")
        assert archive.read(directory) == b""
        assert directory.date_time == build_timestamp.ZIP_TIME
        assert directory.create_system == 3
        assert directory.external_attr >> 16 == 0o40755
        assert archive.read("package/file.py") == b"content"
        assert archive.getinfo("package/file.py").date_time == build_timestamp.ZIP_TIME
        assert (
            build_timestamp.zip_epoch(archive.getinfo("package/file.py").extra)
            == build_timestamp.EPOCH
        )


def test_timestamp_field_rejects_conflicting_absolute_times() -> None:
    field = bytearray(build_timestamp.zip_extra())
    field[-1] = 1
    with pytest.raises(ValueError, match=r"^ZIP build timestamp field is invalid$"):
        build_timestamp.zip_epoch(bytes(field))


@pytest.mark.parametrize(
    "extra",
    [b"", b"x" * 25, build_timestamp.zip_extra()[:6], build_timestamp.zip_extra()[:9]],
)
def test_timestamp_field_rejects_wrong_header_or_size(extra: bytes) -> None:
    with pytest.raises(ValueError, match=r"^ZIP build timestamp field is invalid$"):
        build_timestamp.zip_epoch(extra)


def test_timestamp_field_preserves_unsigned_unix_dates() -> None:
    assert (
        build_timestamp.zip_epoch(build_timestamp.zip_extra(3_000_000_000))
        == 3_000_000_000
    )


def test_direct_zipapp_build_preserves_directory_contract_and_file_datetime(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(build_zipapp, "EPOCH", 1_600_000_000)
    target = build_zipapp.build_zipapp(tmp_path / "processor.pyz", verify=False)
    assert target.stat().st_mtime == 1_600_000_000
    with zipfile.ZipFile(target) as archive:
        directories = {
            member.filename for member in archive.infolist() if member.is_dir()
        }
        assert directories == {
            "eml_attachment_remover/",
            "schema/",
            "eml_attachment_remover-4.0.0.dist-info/",
        }
        for name in directories:
            info = archive.getinfo(name)
            assert archive.read(info) == b""
            assert info.external_attr >> 16 == 0o40755


@pytest.mark.parametrize("editable", [False, True])
def test_backend_wheel_hooks_build_software_only_metadata(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, editable: bool
) -> None:
    """Exercise hooks under their declared PEP 517 backend-path context."""
    monkeypatch.chdir(ROOT)
    monkeypatch.syspath_prepend(str(ROOT / "tools"))
    build = build_backend.build_editable if editable else build_backend.build_wheel
    filename = build(str(tmp_path), {}, str(tmp_path))
    with zipfile.ZipFile(tmp_path / filename) as archive:
        metadata = archive.read(
            next(name for name in archive.namelist() if name.endswith("/METADATA"))
        )
        assert b"License-Expression: MPL-2.0\n" in metadata
        assert not any(name.endswith((".svg", ".icns")) for name in archive.namelist())
