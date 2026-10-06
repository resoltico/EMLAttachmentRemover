"""Construct and enforce normalized distribution archive member metadata."""

from __future__ import annotations

import stat
import zipfile

if __package__:
    from tools.build_timestamp import EPOCH, ZIP_TIME, zip_extra
else:
    from build_timestamp import EPOCH, ZIP_TIME, zip_extra  # type: ignore[import-not-found,no-redef]
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    import tarfile

SOURCE_TIMESTAMP: Final = EPOCH
ZIP_TIMESTAMP: Final = ZIP_TIME
SOURCE_MODE: Final = 0o644
UNIX_ZIP_SYSTEM: Final = 3
WHEEL_DIRECTORY_MODE: Final = 0o40755
WHEEL_SOURCE_MODE: Final = 0o100644
WHEEL_GENERATED_MODE: Final = 0o644


def regular_wheel_member(member: zipfile.ZipInfo) -> bool:
    """Return whether a wheel member is a regular file or directory.

    Returns:
        Whether the ZIP mode agrees with the member's directory marker.

    """
    file_type = stat.S_IFMT(member.external_attr >> 16)
    if member.is_dir():
        return file_type in {0, stat.S_IFDIR}
    return file_type in {0, stat.S_IFREG}


def tar_member_normalized(
    member: tarfile.TarInfo, *, timestamp: int = SOURCE_TIMESTAMP
) -> bool:
    """Return whether a source member has deterministic identity and mode.

    Returns:
        Whether the member matches the declared reproducible backend contract.

    """
    return (
        member.mtime == timestamp
        and member.mode == (0o755 if member.isdir() else SOURCE_MODE)
        and (member.uid, member.gid, member.uname, member.gname) == (0, 0, "", "")
    )


def wheel_member_normalized(
    member: zipfile.ZipInfo,
    *,
    generated: bool,
    timestamp: tuple[int, ...] = ZIP_TIMESTAMP,
) -> bool:
    """Return whether a wheel member has deterministic timestamp and mode.

    Returns:
        Whether the member matches the declared reproducible backend contract.

    """
    if member.is_dir():
        expected_mode = WHEEL_DIRECTORY_MODE
    else:
        expected_mode = WHEEL_GENERATED_MODE if generated else WHEEL_SOURCE_MODE
    return (
        member.date_time == timestamp
        and member.create_system == UNIX_ZIP_SYSTEM
        and member.external_attr >> 16 == expected_mode
    )


def zipapp_member(name: str) -> zipfile.ZipInfo:
    """Return build-timestamped ZIP metadata for a zipapp file or directory.

    Returns:
        A Unix ZIP member with the declared build time and normalized mode.

    """
    info = zipfile.ZipInfo(name, ZIP_TIME)
    info.extra = zip_extra()
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = UNIX_ZIP_SYSTEM
    mode = WHEEL_DIRECTORY_MODE if name.endswith("/") else WHEEL_SOURCE_MODE
    info.external_attr = mode << 16
    return info
