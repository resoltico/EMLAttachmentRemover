"""Prepare a relocated, notice-complete and ad-hoc-signed private CPython runtime."""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Final

from tools import macos_runtime_source, runtime_notices, runtime_pruning

MACHO: Final = {
    b"\xcf\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xca\xfe\xba\xbe",
    b"\xbe\xba\xfe\xca",
}


def binaries(root: Path) -> list[Path]:
    """Discover actual Mach-O files by content, rather than executable suffix.

    Returns:
        Every ordinary Mach-O file beneath the private runtime.

    """
    result = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            magic = stream.read(4)
        if magic in MACHO:
            result.append(path)
    return result


def require_links(root: Path) -> None:
    """Reject absolute, escaping or dangling runtime links.

    Raises:
        ValueError: If a link does not stay inside the relocated runtime.

    """
    resolved = root.resolve(strict=True)
    for path in root.rglob("*"):
        if not path.is_symlink():
            continue
        if path.readlink().is_absolute() or not path.resolve(
            strict=True
        ).is_relative_to(resolved):
            message = "runtime symbolic link escapes its private installation"
            raise ValueError(message)


def _command(arguments: list[str]) -> str:
    """Execute a bounded native verification or signing command.

    Returns:
        Its complete diagnostic output.

    """
    return subprocess.check_output(
        arguments, text=True, stderr=subprocess.STDOUT, timeout=30
    )


def require_native(root: Path, architecture: str) -> list[Path]:
    """Check every binary's CPU and minimum macOS deployment target.

    Returns:
        The verified native code paths for inside-out signing.

    Raises:
        ValueError: If native code is missing, mixed-CPU or newer than macOS 14.

    """
    paths = binaries(root)
    if not paths:
        message = "runtime has no native executable code"
        raise ValueError(message)
    for path in paths:
        if _command(["/usr/bin/lipo", "-archs", str(path)]).strip() != architecture:
            message = "runtime binary architecture differs from selected CPU"
            raise ValueError(message)
        lines = _command(["/usr/bin/otool", "-l", str(path)]).splitlines()
        require_deployment(lines)
    return paths


def require_deployment(lines: list[str]) -> None:
    """Read only macOS deployment commands, not unrelated library/source versions.

    Raises:
        ValueError: If native code lacks a compatible macOS deployment command.

    """
    selected: str | None = None
    versions: list[str] = []
    for line in lines:
        text = line.strip()
        if text.startswith("cmd "):
            selected = {
                "cmd LC_BUILD_VERSION": "minos",
                "cmd LC_VERSION_MIN_MACOSX": "version",
            }.get(text)
        elif (
            selected == "minos"
            and text.startswith("platform ")
            and text != "platform 1"
        ):
            message = "runtime binary does not target macOS"
            raise ValueError(message)
        elif selected and text.startswith(selected + " "):
            versions.append(text.split()[1])
    if not versions or any(
        tuple(map(int, version.split("."))) > (14, 0, 0) for version in versions
    ):
        message = "runtime binary minimum macOS version is unavailable or unsupported"
        raise ValueError(message)


def copy_install(source: Path, target: Path) -> None:
    """Copy runtime, preserve notices and precompile processing modules.

    Compiler failures propagate with subprocess diagnostics.

    Raises:
        ValueError: If a link or the build interpreter fails validation.

    """
    shutil.copytree(source / "install", target, symlinks=True)
    shutil.copytree(source / "licenses", target / "licenses")
    shutil.copy2(source / "PYTHON.json", target / "PYTHON.json")
    require_links(target)
    runtime_notices.apply(target)
    runtime_pruning.prune(target)
    for path in sorted(target.rglob("*.a"), reverse=True):
        if path.is_file() or path.is_symlink():
            path.unlink()
    expected = (macos_runtime_source.ROOT / ".python-version").read_text().strip()
    if (
        sys.implementation.name != "cpython"
        or tuple(map(int, expected.split("."))) != sys.version_info[:3]
    ):
        message = "runtime bytecode requires the pinned CPython build interpreter"
        raise ValueError(message)
    stdlib = target / "lib/python3.14"
    for cache in stdlib.rglob("__pycache__"):
        shutil.rmtree(cache)
    # A fresh compiler process avoids marshal differences from prior string interning.
    subprocess.run(
        [
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
            str(stdlib),
        ],
        check=True,
        capture_output=True,
        timeout=120,
    )
    for path in [target, *target.rglob("*")]:
        if not path.is_symlink():
            mode = 0o755 if path.is_dir() or path.stat().st_mode & 0o111 else 0o644
            path.chmod(mode)
    require_links(target)


def _sign(paths: list[Path]) -> None:
    """Ad-hoc sign every native component and verify each signature independently."""
    for path in sorted(paths, key=lambda value: len(value.parts), reverse=True):
        _command(["/usr/bin/codesign", "--force", "--sign", "-", str(path)])
        _command(["/usr/bin/codesign", "--verify", "--strict", str(path)])


def prepare(target: Path, architecture: str, archive: Path | None = None) -> Path:
    """Prepare one fresh private runtime from verified upstream inputs.

    Returns:
        The published runtime directory; it contains original third-party notices.

    Raises:
        ValueError: If the output already exists or a verification boundary fails.

    """
    if target.exists() or target.is_symlink():
        message = "use a fresh private runtime destination"
        raise ValueError(message)
    selected = macos_runtime_source.pin(architecture)
    if archive is None:
        archive = macos_runtime_source.supplied_archive(selected)
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".eml-runtime-build-", dir=target.parent
    ) as temporary:
        staging = Path(temporary)
        if archive is None:
            archive = staging / "source.tar.zst"
            macos_runtime_source.download(archive, selected)
        source = macos_runtime_source.extract(archive, staging / "source", selected)
        runtime = staging / "Runtime"
        copy_install(source, runtime)
        _sign(require_native(runtime, architecture))
        runtime.rename(target)
    return target


def main() -> int:
    """Enter the macOS build helper using explicit runtime inputs.

    Returns:
        Zero after complete preparation and native signature verification.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, type=Path)
    parser.add_argument("--architecture", required=True, choices=("arm64", "x86_64"))
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    target = prepare(args.target, args.architecture, args.archive)
    print(json.dumps({"runtime": str(target), "architecture": args.architecture}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
