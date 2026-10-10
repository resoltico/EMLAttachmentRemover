"""Derive runtime archive expectations from verified upstream inputs."""

from __future__ import annotations

import hashlib
import shutil
import stat
import subprocess
import tempfile
from contextlib import contextmanager
from pathlib import Path
from typing import TYPE_CHECKING

from tools import macos_runtime, macos_runtime_source

CODE_HASH_LENGTH = 64

if TYPE_CHECKING:
    from collections.abc import Generator


@contextmanager
def reference(architecture: str) -> Generator[Path]:
    """Reconstruct expected runtime resources from the authoritative archive pin.

    Yields:
        A private unsigned-reference tree, with original notices and safe links.

    """
    selected = macos_runtime_source.pin(architecture)
    with tempfile.TemporaryDirectory(prefix="eml-runtime-reference-") as temporary:
        root = Path(temporary)
        archive = macos_runtime_source.supplied_archive(selected)
        if archive is None:
            archive = root / "source.tar.zst"
            macos_runtime_source.download(archive, selected)
        source = macos_runtime_source.extract(archive, root / "source", selected)
        runtime = root / "Runtime"
        with macos_runtime_source.pinned_compiler(source, architecture) as compiler:
            macos_runtime.copy_install(source, runtime, compiler)
        yield runtime


def surface(root: Path, prefix: str) -> dict[str, int]:
    """Describe the exact source-derived runtime file types and modes.

    Returns:
        Archive names mapped to complete UNIX mode values.

    """
    result: dict[str, int] = {prefix + "/": stat.S_IFDIR | 0o755}
    for path in sorted(root.rglob("*")):
        name = prefix + "/" + path.relative_to(root).as_posix()
        if path.is_symlink():
            result[name] = stat.S_IFLNK | 0o777
        elif path.is_dir():
            result[name + "/"] = stat.S_IFDIR | 0o755
        else:
            result[name] = stat.S_IFREG | (
                0o755 if path.stat().st_mode & 0o111 else 0o644
            )
    return result


def _code_hash(path: Path, destination: Path) -> str:
    """Normalize a copied signature using one tool, without changing the delivered file.

    Returns:
        The SHA-256 CodeDirectory hash covering the normalized native code.

    Raises:
        ValueError: If native signature identity cannot be established.

    """
    shutil.copy2(path, destination)
    subprocess.run(
        [
            "/usr/bin/codesign",
            "--force",
            "--sign",
            "-",
            "--identifier",
            "io.github.resoltico.eml.runtime.verification",
            str(destination),
        ],
        check=True,
        capture_output=True,
        timeout=30,
    )
    description = subprocess.run(
        ["/usr/bin/codesign", "-dv", "--verbose=4", str(destination)],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    ).stderr
    values = [
        line.partition("=")[2]
        for line in description.splitlines()
        if line.startswith("CandidateCDHashFull sha256=")
    ]
    if len(values) != 1 or len(values[0]) != CODE_HASH_LENGTH:
        message = "runtime native code identity is unavailable"
        raise ValueError(message)
    return values[0]


def _bytecode_diagnostic(actual: Path, expected: Path) -> str:
    """Identify a mismatched pyc section without loading untrusted code.

    Returns:
        A bounded description containing both SHA-256 digests.

    """
    delivered = actual.read_bytes()
    trusted = expected.read_bytes()
    section = "header" if delivered[:16] != trusted[:16] else "payload"
    return (
        f" (bytecode {section}; delivered SHA-256="
        f"{hashlib.sha256(delivered).hexdigest()}; expected SHA-256="
        f"{hashlib.sha256(trusted).hexdigest()})"
    )


def verify(actual: Path, expected: Path, architecture: str) -> None:
    """Require the source-derived tree, resources and canonical native-code identities.

    Raises:
        ValueError: If delivered runtime files differ from trusted upstream resources.

    """
    if surface(actual, "Runtime") != surface(expected, "Runtime"):
        message = "runtime tree differs from verified upstream source"
        raise ValueError(message)
    macos_runtime.require_links(actual)
    native = set(macos_runtime.require_native(actual, architecture))
    with tempfile.TemporaryDirectory(prefix="eml-runtime-code-") as temporary:
        comparison = Path(temporary)
        for path in sorted(actual.rglob("*")):
            source = expected / path.relative_to(actual)
            if path.is_symlink():
                matches = path.readlink() == source.readlink()
            elif path.is_dir():
                continue
            elif path in native:
                matches = _code_hash(path, comparison / "actual") == _code_hash(
                    source, comparison / "expected"
                )
            else:
                matches = path.read_bytes() == source.read_bytes()
            if not matches:
                message = (
                    "runtime resource or native code differs from upstream source: "
                    + path.relative_to(actual).as_posix()
                )
                suffix = (
                    _bytecode_diagnostic(path, source) if path.suffix == ".pyc" else ""
                )
                raise ValueError(message + suffix)
