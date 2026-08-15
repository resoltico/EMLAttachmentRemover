"""Enforce normalized Hatchling source and wheel member metadata."""

from __future__ import annotations

import stat
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    import tarfile
    import zipfile

SOURCE_TIMESTAMP: Final = 1_580_601_600
ZIP_TIMESTAMP: Final = (2020, 2, 2, 0, 0, 0)
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


def tar_member_normalized(member: tarfile.TarInfo) -> bool:
    """Return whether a source member has deterministic identity and mode.

    Returns:
        Whether the member matches the declared reproducible backend contract.

    """
    return (
        member.mtime == SOURCE_TIMESTAMP
        and member.mode == SOURCE_MODE
        and (member.uid, member.gid, member.uname, member.gname) == (0, 0, "", "")
    )


def wheel_member_normalized(member: zipfile.ZipInfo, *, generated: bool) -> bool:
    """Return whether a wheel member has deterministic timestamp and mode.

    Returns:
        Whether the member matches the declared reproducible backend contract.

    """
    if member.is_dir():
        expected_mode = WHEEL_DIRECTORY_MODE
    else:
        expected_mode = WHEEL_GENERATED_MODE if generated else WHEEL_SOURCE_MODE
    return (
        member.date_time == ZIP_TIMESTAMP
        and member.create_system == UNIX_ZIP_SYSTEM
        and member.external_attr >> 16 == expected_mode
    )
