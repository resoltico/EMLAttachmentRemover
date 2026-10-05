"""Pinned runtime source and native-code trust boundaries have negative controls."""

from __future__ import annotations

import csv
import errno
import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
from contextlib import ExitStack
from pathlib import Path

import pytest
from tools import (
    macos_runtime,
    macos_runtime_archive,
    macos_runtime_source,
    runtime_notices,
)


def runtime_archive_fixture(
    tmp_path: Path, *, escape: bool = False, version: str = "3.14.8"
) -> tuple[Path, macos_runtime_source.RuntimePin]:
    """Construct a small independently specified upstream runtime archive.

    Returns:
        The fixture archive and its independently computed identity pin.

    """
    archive = tmp_path / "runtime.tar.zst"
    metadata = {
        "python_version": version,
        "target_triple": "aarch64-apple-darwin",
        "build_options": "pgo+lto",
        "libpython_link_mode": "shared",
        "license_path": "licenses/LICENSE.cpython.txt",
    }
    with tarfile.open(archive, "w:zst") as output:
        for name, content in {
            "python/PYTHON.json": json.dumps(metadata).encode(),
            "python/install/bin/python3.14": b"test interpreter",
            "python/licenses/LICENSE.cpython.txt": b"original notice",
            "python/build/unused.o": b"unneeded build object",
        }.items():
            member = tarfile.TarInfo(name)
            member.size = len(content)
            output.addfile(member, io.BytesIO(content))
        if escape:
            member = tarfile.TarInfo("python/install/escape")
            member.type = tarfile.SYMTYPE
            member.linkname = "/etc/passwd"
            output.addfile(member)
    pin = macos_runtime_source.RuntimePin(
        url="https://github.com/astral-sh/python-build-standalone/releases/download/example/runtime.tar.zst",
        sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),
        size=archive.stat().st_size,
        target="aarch64-apple-darwin",
    )
    return archive, pin


def test_verified_installation_keeps_original_notices_and_excludes_build_objects(
    tmp_path: Path,
) -> None:
    """Only validated install resources and their original notices reach extraction."""
    archive, pin = runtime_archive_fixture(tmp_path)
    root = macos_runtime_source.extract(archive, tmp_path / "extracted", pin)
    assert (root / "install/bin/python3.14").read_bytes() == b"test interpreter"
    assert (root / "licenses/LICENSE.cpython.txt").read_bytes() == b"original notice"
    assert not (root / "build").exists()


def test_archive_corruption_is_refused_before_extraction(tmp_path: Path) -> None:
    """A same-sized corrupted cache cannot satisfy its recorded digest."""
    archive, pin = runtime_archive_fixture(tmp_path)
    content = bytearray(archive.read_bytes())
    content[-1] ^= 1
    archive.write_bytes(content)
    with pytest.raises(ValueError, match="digest"):
        macos_runtime_source.extract(archive, tmp_path / "extracted", pin)
    assert not (tmp_path / "extracted").exists()


def test_escaping_links_are_refused_by_safe_extraction(tmp_path: Path) -> None:
    """Even a pin-matching archive cannot install an external symbolic link."""
    archive, pin = runtime_archive_fixture(tmp_path, escape=True)
    with pytest.raises(tarfile.FilterError):
        macos_runtime_source.extract(archive, tmp_path / "extracted", pin)


def test_runtime_version_must_match_the_repository_interpreter(tmp_path: Path) -> None:
    """A source pin cannot silently select another maintenance version."""
    archive, pin = runtime_archive_fixture(tmp_path, version="3.14.7")
    with pytest.raises(ValueError, match="metadata differs"):
        macos_runtime_source.extract(archive, tmp_path / "extracted", pin)


@pytest.mark.parametrize(
    "lines",
    [
        [],
        ["cmd LC_BUILD_VERSION", "platform 2", "minos 11.0"],
        ["cmd LC_BUILD_VERSION", "platform 1", "minos 14.1"],
        ["cmd LC_SOURCE_VERSION", "version 11.0"],
    ],
)
def test_native_deployment_controls_reject_wrong_os_newer_os_and_missing_commands(
    lines: list[str],
) -> None:
    """CPU identity alone cannot establish minimum-OS compatibility."""
    with pytest.raises(ValueError, match="runtime binary") as caught:
        macos_runtime.require_deployment(lines)
    assert str(caught.value) == (
        "runtime binary does not target macOS"
        if "platform 2" in lines
        else "runtime binary minimum macOS version is unavailable or unsupported"
    )


def test_linker_version_is_not_confused_with_deployment_target() -> None:
    """Compiler/linker version numbers cannot raise the runtime's OS requirement."""
    macos_runtime.require_deployment([
        "cmd LC_BUILD_VERSION",
        "platform 1",
        "minos 11.0",
        "ntools 1",
        "version 1170.0",
        "cmd LC_SOURCE_VERSION",
        "version 99999.0",
    ])
    macos_runtime.require_deployment(["cmd LC_VERSION_MIN_MACOSX", "version 10.15"])
    macos_runtime.require_deployment([
        "cmd LC_BUILD_VERSION",
        "platform 1",
        "minos 14.0.0",
    ])
    with pytest.raises(ValueError, match="unsupported"):
        macos_runtime.require_deployment([
            "cmd LC_BUILD_VERSION",
            "platform 1",
            "minos 14.0.1",
        ])


def test_private_runtime_links_must_stay_inside_relocated_tree(tmp_path: Path) -> None:
    """Relative internal links survive relocation; external targets do not."""
    root = tmp_path / "Runtime"
    root.mkdir()
    (root / "python").write_bytes(b"executable")
    link = root / "python3"
    link.symlink_to("python")
    macos_runtime.require_links(root)
    link.unlink()
    link.symlink_to(Path("..") / "external")
    (tmp_path / "external").write_bytes(b"outside")
    with pytest.raises(
        ValueError, match=r"^runtime symbolic link escapes its private installation$"
    ):
        macos_runtime.require_links(root)
    link.unlink()
    link.symlink_to("missing")
    with pytest.raises(FileNotFoundError) as missing:
        macos_runtime.require_links(root)
    assert missing.value.errno == errno.ENOENT
    with pytest.raises(FileNotFoundError, match="absent-runtime"):
        macos_runtime.require_links(tmp_path / "absent-runtime")


@pytest.mark.parametrize("consumer", ["build", "verification"])
def test_supplied_source_directory_does_not_bypass_integrity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, consumer: str
) -> None:
    """A corrupted explicitly supplied source fails before either tree is created."""
    archive, pin = runtime_archive_fixture(tmp_path)
    monkeypatch.setenv("EML_RUNTIME_SOURCE_DIRECTORY", str(tmp_path))
    monkeypatch.setattr(macos_runtime_source, "pin", lambda _architecture: pin)
    content = bytearray(archive.read_bytes())
    content[-1] ^= 1
    archive.write_bytes(content)
    if consumer == "build":
        with pytest.raises(ValueError, match="digest"):
            macos_runtime.prepare(tmp_path / "Runtime", "arm64")
    else:
        with ExitStack() as resources, pytest.raises(ValueError, match="digest"):
            resources.enter_context(macos_runtime_archive.reference("arm64"))
    assert not (tmp_path / "Runtime").exists()


def test_cached_sources_download_once_and_reject_corruption(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Reuse authenticated inputs without silently accepting corrupted cache hits."""
    content = b"independent archive contents"
    downloads: list[str] = []

    def selected(architecture: str) -> macos_runtime_source.RuntimePin:
        return macos_runtime_source.RuntimePin(
            url=f"https://github.com/astral-sh/python-build-standalone/releases/download/example/{architecture}.tar.zst",
            sha256=hashlib.sha256(content).hexdigest(),
            size=len(content),
            target=architecture,
        )

    def download(path: Path, pin: macos_runtime_source.RuntimePin) -> None:
        downloads.append(pin["target"])
        assert path.parent.parent == cache
        assert path.parent.name.startswith(".download-")
        path.write_bytes(content)
        macos_runtime_source.verify(path, pin)

    monkeypatch.setattr(macos_runtime_source, "pin", selected)
    monkeypatch.setattr(macos_runtime_source, "download", download)
    cache = tmp_path / "nested" / "cache"
    macos_runtime_source.cache(cache)
    (cache / "x86_64.tar.zst").unlink()
    macos_runtime_source.cache(cache)
    assert downloads == ["arm64", "x86_64", "x86_64"]
    macos_runtime_source.cache(cache)
    assert downloads == ["arm64", "x86_64", "x86_64"]
    (cache / "arm64.tar.zst").write_bytes(b"x" * len(content))
    with pytest.raises(ValueError, match="digest"):
        macos_runtime_source.cache(cache)
    assert downloads == ["arm64", "x86_64", "x86_64"]


def test_runtime_bytecode_is_relocatable_and_independently_rebuilt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Different staging paths yield identical caches with no private filename."""
    source = tmp_path / "source"
    module = source / "install/lib/python3.14/example.py"
    module.parent.mkdir(parents=True)
    module.write_text("VALUE = 42\n")
    stale = module.parent / "__pycache__" / "obsolete.cpython-314.pyc"
    stale.parent.mkdir()
    stale.write_bytes(b"obsolete bytecode")
    csv_source = Path(csv.__file__)
    (module.parent / "csv.py").write_bytes(csv_source.read_bytes())
    (source / "licenses").mkdir()
    (source / "PYTHON.json").write_text("{}")
    monkeypatch.setattr(runtime_notices, "apply", lambda _root: None)
    compile_command = subprocess.run

    def compile_runtime(
        args: list[str],
        *,
        check: bool,
        capture_output: bool,
        timeout: int,
    ) -> subprocess.CompletedProcess[bytes]:
        assert args[:-1] == [
            sys.executable,
            "-I",
            "-B",
            "-m",
            "compileall",
            "-q",
            "-q",
            "-f",
            "--invalidation-mode",
            "unchecked-hash",
            "-o",
            "0",
            "-d",
            "python3.14",
        ]
        assert (check, capture_output, timeout) == (True, True, 120)
        return compile_command(
            args, check=check, capture_output=capture_output, timeout=timeout
        )

    monkeypatch.setattr(subprocess, "run", compile_runtime)
    actual, expected = tmp_path / "actual", tmp_path / "expected"
    macos_runtime.copy_install(source, actual)
    macos_runtime.copy_install(source, expected)
    caches = list(actual.rglob("*.pyc"))
    assert len(caches) == 2
    assert not (actual / stale.relative_to(source / "install")).exists()
    caches = sorted(caches, key=lambda path: path.name, reverse=True)
    bytecode = caches[0].read_bytes()
    assert int.from_bytes(bytecode[4:8], "little") == 1
    assert os.fsencode(Path("python3.14") / "example.py") in bytecode
    assert str(tmp_path).encode() not in bytecode
    assert all(
        cache.read_bytes() == (expected / cache.relative_to(actual)).read_bytes()
        for cache in caches
    )
    monkeypatch.setattr(macos_runtime, "require_native", lambda _root, _arch: [])
    macos_runtime_archive.verify(actual, expected, "arm64")
    caches[0].write_bytes(bytecode[:-1] + bytes([bytecode[-1] ^ 1]))
    with pytest.raises(ValueError, match="differs"):
        macos_runtime_archive.verify(actual, expected, "arm64")


def test_runtime_resource_modes_and_internal_links_survive_copying(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "source"
    executable = source / "install/bin/python"
    executable.parent.mkdir(parents=True)
    executable.write_bytes(b"synthetic executable mode fixture")
    executable.chmod(0o401)
    (executable.parent / "python3").symlink_to("python")
    module = source / "install/lib/python3.14/example.py"
    module.parent.mkdir(parents=True)
    module.write_text("VALUE = 42\n")
    (source / "licenses").mkdir()
    (source / "PYTHON.json").write_text("{}")
    monkeypatch.setattr(runtime_notices, "apply", lambda _root: None)
    actual = tmp_path / "runtime"
    macos_runtime.copy_install(source, actual)
    assert (actual / "bin/python3").is_symlink()
    if os.name != "nt":
        assert (actual / "bin/python").stat().st_mode & 0o777 == 0o755
        assert (actual / "lib").stat().st_mode & 0o777 == 0o755
        assert (actual / "lib/python3.14/example.py").stat().st_mode & 0o777 == 0o644


def test_runtime_preparation_rejects_an_unpinned_compiler(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "source"
    (source / "install").mkdir(parents=True)
    (source / "licenses").mkdir()
    (source / "PYTHON.json").write_text("{}")
    (tmp_path / ".python-version").write_text("0.0.0")
    monkeypatch.setattr(runtime_notices, "apply", lambda _root: None)
    monkeypatch.setattr(macos_runtime_source, "ROOT", tmp_path)
    with pytest.raises(
        ValueError,
        match=r"^runtime bytecode requires the pinned CPython build interpreter$",
    ):
        macos_runtime.copy_install(source, tmp_path / "runtime")


def test_runtime_source_command_routes_the_directory_and_has_cli_help(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[Path] = []
    monkeypatch.setattr(macos_runtime_source, "cache", calls.append)
    monkeypatch.setattr(sys, "argv", ["runtime-source", "--directory", str(tmp_path)])
    assert macos_runtime_source.main() == 0
    assert calls == [tmp_path]
    help_result = subprocess.run(
        [sys.executable, "-B", "-m", "tools.macos_runtime_source", "--help"],
        check=True,
        text=True,
        capture_output=True,
        timeout=10,
    )
    assert "--directory" in help_result.stdout
    assert "pinned upstream runtime with its original notices" in help_result.stdout


@pytest.mark.parametrize(
    ("operation", "arguments"),
    [
        ("cache", []),
        ("prepare", []),
        ("prepare", ["--target", "unused"]),
        ("prepare", ["--architecture", "arm64"]),
        ("prepare", ["--target", "unused", "--architecture", "universal"]),
    ],
)
def test_runtime_commands_reject_missing_or_unsupported_inputs_before_work(
    monkeypatch: pytest.MonkeyPatch,
    operation: str,
    arguments: list[str],
) -> None:
    calls: list[str] = []
    monkeypatch.setattr(
        macos_runtime, "prepare", lambda *_args, **_kwargs: calls.append("prepare")
    )
    monkeypatch.setattr(
        macos_runtime_source, "cache", lambda _path: calls.append("cache")
    )
    monkeypatch.setattr(sys, "argv", ["runtime-command", *arguments])
    selected = macos_runtime_source if operation == "cache" else macos_runtime
    with pytest.raises(SystemExit) as caught:
        selected.main()
    assert caught.value.code == 2
    assert calls == []


def test_runtime_staging_stays_on_the_destination_volume_and_cleans_up_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    archive, selected = runtime_archive_fixture(tmp_path)
    target = tmp_path / "nested" / "runtime"
    allocated: list[Path] = []
    temporary_directory = tempfile.TemporaryDirectory

    def staging(
        *, prefix: str | None, **options: Path
    ) -> tempfile.TemporaryDirectory[str]:
        directory = options["dir"]
        assert directory == target.parent
        allocated.append(directory)
        return temporary_directory(prefix=prefix, dir=directory)

    monkeypatch.setattr(tempfile, "TemporaryDirectory", staging)
    monkeypatch.setattr(macos_runtime_source, "pin", lambda _cpu: selected)
    with pytest.raises(ValueError, match=r"^runtime has no native executable code$"):
        macos_runtime.prepare(target, "arm64", archive)
    assert allocated == [target.parent]
    assert not target.exists()
    assert list(target.parent.iterdir()) == []
