"""Windows ABI constants and ctypes layouts for the narrow native adapter."""

from __future__ import annotations

import ctypes
from typing import Final

INVALID_HANDLE_VALUE: Final = ctypes.c_void_p(-1).value
FILE_READ_DATA: Final = 0x0001
FILE_WRITE_DATA: Final = 0x0002
FILE_TRAVERSE: Final = 0x0020
FILE_READ_ATTRIBUTES: Final = 0x0080
FILE_WRITE_ATTRIBUTES: Final = 0x0100
DELETE: Final = 0x0001_0000
SYNCHRONIZE: Final = 0x0010_0000
FILE_SHARE_ALL: Final = 0x0000_0007
FILE_OPEN: Final = 1
FILE_CREATE: Final = 2
FILE_NON_DIRECTORY_FILE: Final = 0x0000_0040
FILE_SYNCHRONOUS_IO_NONALERT: Final = 0x0000_0020
FILE_OPEN_REPARSE_POINT: Final = 0x0020_0000
FILE_FLAG_BACKUP_SEMANTICS: Final = 0x0200_0000
OPEN_EXISTING: Final = 3
FILE_ATTRIBUTE_DIRECTORY: Final = 0x0010
FILE_ATTRIBUTE_REPARSE_POINT: Final = 0x0400
FILE_TYPE_DISK: Final = 1
FILE_ID_INFO: Final = 18
FILE_BASIC_INFO: Final = 0
FILE_STANDARD_INFO: Final = 1
FILE_ATTRIBUTE_TAG_INFO: Final = 9
FILE_RENAME_INFO: Final = 3
FILE_DISPOSITION_INFO: Final = 4
ERROR_FILE_EXISTS: Final = 80
ERROR_ALREADY_EXISTS: Final = 183
O_BINARY: Final = 0x8000
O_READ_ONLY: Final = 0
O_READ_WRITE: Final = 2
MIN_DRIVE_ABSOLUTE_LENGTH: Final = 3


class UnicodeString(ctypes.Structure):
    """Native ``UNICODE_STRING`` layout retaining UTF-16 byte length exactly."""

    _fields_ = [
        ("Length", ctypes.c_ushort),
        ("MaximumLength", ctypes.c_ushort),
        ("Buffer", ctypes.c_void_p),
    ]


class ObjectAttributes(ctypes.Structure):
    """Native ``OBJECT_ATTRIBUTES`` layout including a RootDirectory handle."""

    _fields_ = [
        ("Length", ctypes.c_ulong),
        ("RootDirectory", ctypes.c_void_p),
        ("ObjectName", ctypes.POINTER(UnicodeString)),
        ("Attributes", ctypes.c_ulong),
        ("SecurityDescriptor", ctypes.c_void_p),
        ("SecurityQualityOfService", ctypes.c_void_p),
    ]


class IoStatusBlock(ctypes.Structure):
    """Native ``IO_STATUS_BLOCK`` layout supplied to ``NtCreateFile``."""

    _fields_ = [("Status", ctypes.c_long), ("Information", ctypes.c_size_t)]


class FileIdInfo(ctypes.Structure):
    """Native FILE_ID_INFO layout with a volume serial and 128-bit identifier."""

    _fields_ = [
        ("VolumeSerialNumber", ctypes.c_ulonglong),
        ("FileId", ctypes.c_ubyte * 16),
    ]


class FileBasicInfo(ctypes.Structure):
    """Native FILE_BASIC_INFO layout used for change and write timestamps."""

    _fields_ = [
        ("CreationTime", ctypes.c_longlong),
        ("LastAccessTime", ctypes.c_longlong),
        ("LastWriteTime", ctypes.c_longlong),
        ("ChangeTime", ctypes.c_longlong),
        ("FileAttributes", ctypes.c_ulong),
    ]


class FileStandardInfo(ctypes.Structure):
    """Native FILE_STANDARD_INFO layout used for regular-file and size checks."""

    _fields_ = [
        ("AllocationSize", ctypes.c_longlong),
        ("EndOfFile", ctypes.c_longlong),
        ("NumberOfLinks", ctypes.c_ulong),
        ("DeletePending", ctypes.c_ubyte),
        ("Directory", ctypes.c_ubyte),
    ]


class FileAttributeTagInfo(ctypes.Structure):
    """Native FILE_ATTRIBUTE_TAG_INFO layout used for reparse-point checks."""

    _fields_ = [("FileAttributes", ctypes.c_ulong), ("ReparseTag", ctypes.c_ulong)]
