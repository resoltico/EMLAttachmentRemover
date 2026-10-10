"""Verify and extract the pinned upstream runtime with its original notices."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import tarfile
import tempfile
import tomllib
import urllib.request
from contextlib import contextmanager
from pathlib import Path
from typing import TYPE_CHECKING, Final, TypedDict
from urllib.parse import unquote, urlsplit

ROOT: Final = Path(__file__).resolve().parents[1]
CONFIG: Final = ROOT / "integrations/macos-ui/runtime-source.toml"
CHUNK: Final = 1024 * 1024
MAX_METADATA: Final = 2 * CHUNK

if TYPE_CHECKING:
    from collections.abc import Generator


class RuntimePin(TypedDict):
    """The independently recorded archive identity and native target."""

    url: str
    sha256: str
    size: int
    target: str


def supplied_archive(selected: RuntimePin) -> Path | None:
    """Find an explicitly supplied source directory without trusting cached bytes.

    Returns:
        The pin-derived pathname, or None when fresh downloading is requested.

    """
    directory = os.environ.get("EML_RUNTIME_SOURCE_DIRECTORY")
    if not directory:
        return None
    return Path(directory) / unquote(Path(urlsplit(selected["url"]).path).name)


def pin(architecture: str) -> RuntimePin:
    """Read the one authoritative upstream artifact pin.

    Returns:
        Its explicit URL, size, digest and target.

    Raises:
        ValueError: If the architecture or pin fields are invalid.

    """
    with CONFIG.open("rb") as stream:
        value = tomllib.load(stream).get(architecture)
    if (
        not isinstance(value, dict)
        or set(value) != {"url", "sha256", "size", "target"}
        or not all(isinstance(value[key], str) for key in ("url", "sha256", "target"))
        or type(value["size"]) is not int
        or value["size"] <= 0
    ):
        message = "invalid runtime archive pin"
        raise ValueError(message)
    _require_url(value["url"])
    return RuntimePin(
        url=value["url"],
        sha256=value["sha256"],
        size=value["size"],
        target=value["target"],
    )


def verify(archive: Path, selected: RuntimePin) -> None:
    """Rehash a supplied or freshly downloaded archive before inspecting it.

    Raises:
        ValueError: If the archive is symbolic or differs from its pinned identity.

    """
    if archive.is_symlink() or archive.stat().st_size != selected["size"]:
        message = "runtime archive size or file type differs from pin"
        raise ValueError(message)
    digest = hashlib.sha256()
    with archive.open("rb") as stream:
        while block := stream.read(CHUNK):
            digest.update(block)
    if digest.hexdigest() != selected["sha256"]:
        message = "runtime archive digest differs from pin"
        raise ValueError(message)


def download(target: Path, selected: RuntimePin) -> None:
    """Fetch one bounded archive into a fresh private staging file.

    Raises:
        ValueError: If the response exceeds the pinned byte count.

    """
    with (
        urllib.request.urlopen(selected["url"], timeout=30) as response,  # ruff: ignore[suspicious-url-open-usage] - HTTPS GitHub release pin; downloaded bytes independently hash-verified.
        target.open("xb") as stream,
    ):
        size = 0
        while block := response.read(CHUNK):
            size += len(block)
            if size > selected["size"]:
                message = "runtime download exceeds pinned byte count"
                raise ValueError(message)
            stream.write(block)
    verify(target, selected)


def extract(archive: Path, destination: Path, selected: RuntimePin) -> Path:
    """Extract install resources and original notices, excluding upstream build objects.

    Returns:
        The extracted upstream ``python`` directory.

    Raises:
        ValueError: If metadata or the required installation/notices are inconsistent.

    """
    verify(archive, selected)
    with tarfile.open(archive, "r:zst") as source:
        metadata_member = source.getmember("python/PYTHON.json")
        _require_metadata(metadata_member)
        stream = source.extractfile(metadata_member)
        if stream is None:
            message = "runtime metadata is unavailable"
            raise ValueError(message)
        with stream:
            metadata: object = json.load(stream)
        _validate_metadata(metadata, selected)
        members = [
            member
            for member in source.getmembers()
            if member.name == "python/PYTHON.json"
            or member.name.startswith(("python/install/", "python/licenses/"))
        ]
        source.extractall(destination, members=members, filter="data")
    root = destination / "python"
    if (
        not (root / "install/bin/python3.14").is_file()
        or not (root / "licenses/LICENSE.cpython.txt").is_file()
    ):
        message = "runtime installation or original notices are missing"
        raise ValueError(message)
    return root


@contextmanager
def pinned_compiler(source: Path, architecture: str) -> Generator[Path]:
    """Yield a source-authenticated CPython compiler runnable on this host.

    The provided source has already passed the pinned-archive extraction.
    For cross-CPU targets, extract a separate pinned host-native distribution.
    Never select the compiler from PATH, uv discovery, or sys.executable.

    Yields:
        The verified native CPython compiler executable.

    Raises:
        ValueError: If the host or target architecture is unsupported.

    """
    host = platform.machine()
    if host not in {"arm64", "x86_64"} or architecture not in {"arm64", "x86_64"}:
        message = "runtime bytecode compiler requires a supported macOS CPU"
        raise ValueError(message)
    if host == architecture:
        yield source / "install/bin/python3.14"
        return
    selected = pin(host)
    with tempfile.TemporaryDirectory(prefix="eml-runtime-compiler-") as temporary:
        root = Path(temporary)
        archive = supplied_archive(selected)
        if archive is None:
            archive = root / "source.tar.zst"
            download(archive, selected)
        compiler_source = extract(archive, root / "source", selected)
        yield compiler_source / "install/bin/python3.14"


def _require_metadata(member: tarfile.TarInfo) -> None:
    """Require a bounded ordinary metadata file before opening it.

    Raises:
        ValueError: If metadata could redirect extraction or exceed its limit.

    """
    if not member.isfile() or not 0 < member.size <= MAX_METADATA:
        message = "invalid upstream runtime metadata member"
        raise ValueError(message)


def _validate_metadata(value: object, selected: RuntimePin) -> None:
    """Bind upstream metadata to the repository interpreter and native target.

    Raises:
        ValueError: If version, optimization, target or license identity differs.

    """
    version = (ROOT / ".python-version").read_text(encoding="utf-8").strip()
    expected = {
        "python_version": version,
        "target_triple": selected["target"],
        "build_options": "pgo+lto",
        "libpython_link_mode": "shared",
        "license_path": "licenses/LICENSE.cpython.txt",
    }
    if not isinstance(value, dict) or any(
        value.get(key) != required for key, required in expected.items()
    ):
        message = "upstream runtime metadata differs from the required contract"
        raise ValueError(message)


def _require_url(url: str) -> None:
    """Require the configured artifact to originate at the HTTPS GitHub release.

    Raises:
        ValueError: If configuration selects another scheme, origin or path.

    """
    parsed = urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.netloc != "github.com"
        or not parsed.path.startswith(
            "/astral-sh/python-build-standalone/releases/download/"
        )
        or parsed.query
        or parsed.fragment
    ):
        message = "runtime source must be an HTTPS upstream GitHub release"
        raise ValueError(message)


def cache(directory: Path) -> None:
    """Populate both pinned archives, retaining hash checks on cache hits."""
    directory.mkdir(parents=True, exist_ok=True)
    for architecture in ("arm64", "x86_64"):
        selected = pin(architecture)
        target = directory / unquote(Path(urlsplit(selected["url"]).path).name)
        if target.exists() or target.is_symlink():
            verify(target, selected)
            continue
        with tempfile.TemporaryDirectory(
            prefix=".download-", dir=directory
        ) as temporary:
            staging = Path(temporary) / "source.tar.zst"
            download(staging, selected)
            staging.replace(target)


def main() -> int:
    """Populate the CI or local source archive cache.

    Returns:
        Zero after both archives have been authenticated.

    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", required=True, type=Path)
    cache(parser.parse_args().directory)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
