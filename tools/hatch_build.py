"""Derive the CPython-specific pure-Python wheel tag from project metadata."""

from __future__ import annotations

import gzip
import io
import re
import tarfile
import tomllib
import zipfile
from pathlib import Path, PurePosixPath
from typing import Any, Final, override

if __package__ == "tools":
    from tools.build_timestamp import EPOCH, ZIP_TIME, zip_extra
else:
    from build_timestamp import EPOCH, ZIP_TIME, zip_extra  # type: ignore[import-not-found,no-redef]

from hatchling.builders.config import BuilderConfig
from hatchling.builders.hooks.plugin.interface import BuildHookInterface

REQUIRES_PATTERN: Final = re.compile(r">=(\d+)\.(\d+),<(\d+)\.(\d+)")


def wheel_tag(project_config: Path) -> str:
    """Return the exact CPython wheel tag derived from canonical metadata.

    Returns:
        A CPython-major/minor, ABI-independent, platform-independent tag.

    Raises:
        ValueError: If the runtime declaration cannot produce one safe tag.

    """
    with project_config.open("rb") as config_file:
        configuration = tomllib.load(config_file)
    requires_python = str(configuration["project"]["requires-python"])
    runtime = configuration["tool"]["eml-attachment-remover"]["runtime"]
    implementation = str(runtime["implementation"])
    match = REQUIRES_PATTERN.fullmatch(requires_python)
    if match is None or implementation != "CPython":
        message = "wheel tag requires exact CPython major/minor project bounds"
        raise ValueError(message)
    lower_major, lower_minor, upper_major, upper_minor = map(int, match.groups())
    if (upper_major, upper_minor) != (lower_major, lower_minor + 1):
        message = "wheel tag requires one supported CPython minor version"
        raise ValueError(message)
    return f"cp{lower_major}{lower_minor}-none-any"


class CustomBuildHook(BuildHookInterface[BuilderConfig]):
    """Set a precise wheel tag before Hatchling constructs the archive."""

    @override
    def initialize(self, version: str, build_data: dict[str, Any]) -> None:
        """Apply the metadata-derived tag to standard and editable wheels."""
        if self.target_name == "wheel":
            build_data["tag"] = wheel_tag(Path(self.root) / "pyproject.toml")

    @override
    def finalize(
        self, version: str, build_data: dict[str, Any], artifact_path: str
    ) -> None:
        """Include timestamped directories so source extraction retains build time."""
        if self.target_name == "wheel":
            timestamp_wheel(Path(artifact_path))
            return
        if self.target_name != "sdist":
            return
        path = Path(artifact_path)
        with tarfile.open(path, "r:gz") as source:
            members = [(member, source.extractfile(member)) for member in source]
            contents = [
                (member, stream.read() if stream else b"") for member, stream in members
            ]
        with (Path(self.root) / "pyproject.toml").open("rb") as stream:
            licenses = tomllib.load(stream)["tool"]["eml-attachment-remover"][
                "licenses"
            ]
        contents = [
            (
                member,
                content.replace(
                    f"License-Expression: {licenses['source']}\n".encode(),
                    (
                        f"License-Expression: {licenses['source']}\n"
                        "Dynamic: License-Expression\n"
                    ).encode(),
                    1,
                )
                if member.name.endswith("/PKG-INFO")
                else content,
            )
            for member, content in contents
        ]
        timestamp = EPOCH
        directories = sorted({
            str(parent)
            for member, _ in contents
            for parent in PurePosixPath(member.name).parents
            if str(parent) != "."
        })
        with (
            path.open("wb") as output,
            gzip.GzipFile(
                filename="", mode="wb", fileobj=output, mtime=timestamp
            ) as compressed,
            tarfile.open(fileobj=compressed, mode="w") as target,
        ):
            for name in directories:
                directory = tarfile.TarInfo(name)
                directory.type = tarfile.DIRTYPE
                directory.mode = 0o755
                directory.mtime = timestamp
                target.addfile(directory)
            for member, content in contents:
                member.size = len(content)
                target.addfile(member, io.BytesIO(content))


def timestamp_wheel(path: Path) -> None:
    """Attach UTC modification times without changing installed member contents."""
    with zipfile.ZipFile(path) as source:
        members = [(member, source.read(member)) for member in source.infolist()]
    names = {member.filename for member, _content in members}
    directories = {
        str(parent) + "/"
        for name in names
        for parent in PurePosixPath(name).parents
        if str(parent) != "."
    } - names
    for name in directories:
        directory = zipfile.ZipInfo(name)
        directory.create_system = 3
        directory.external_attr = 0o40755 << 16
        members.append((directory, b""))
    with zipfile.ZipFile(path, "w") as target:
        for member, content in sorted(members, key=lambda item: item[0].filename):
            member.date_time = ZIP_TIME
            member.extra = zip_extra()
            target.writestr(member, content)
