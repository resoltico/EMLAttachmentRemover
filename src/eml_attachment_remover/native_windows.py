"""Small ctypes adapter for Windows handle-rooted file operations.

The adapter intentionally exposes handles rather than rebuilding paths.  Windows
callers bind a directory once, address children through ``RootDirectory``, and use
the same live directory handle for the no-replace rename edge.
"""

from __future__ import annotations

import ctypes
import os
from dataclasses import dataclass
from typing import TYPE_CHECKING, NoReturn, Protocol, cast

from .native_windows_abi import (
    DELETE,
    ERROR_ALREADY_EXISTS,
    ERROR_FILE_EXISTS,
    FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO,
    FILE_CREATE,
    FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_ID_INFO,
    FILE_NON_DIRECTORY_FILE,
    FILE_OPEN,
    FILE_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES,
    FILE_READ_DATA,
    FILE_RENAME_INFO_EX,
    FILE_SHARE_ALL,
    FILE_STANDARD_INFO,
    FILE_SYNCHRONOUS_IO_NONALERT,
    FILE_TYPE_DISK,
    FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA,
    INVALID_HANDLE_VALUE,
    MIN_DRIVE_ABSOLUTE_LENGTH,
    O_BINARY,
    O_READ_ONLY,
    O_READ_WRITE,
    OPEN_EXISTING,
    SYNCHRONIZE,
)
from .native_windows_abi import (
    FileAttributeTagInfo as _FileAttributeTagInfo,
)
from .native_windows_abi import (
    FileBasicInfo as _FileBasicInfo,
)
from .native_windows_abi import (
    FileIdInfo as _FileIdInfo,
)
from .native_windows_abi import (
    FileStandardInfo as _FileStandardInfo,
)
from .native_windows_abi import (
    IoStatusBlock as _IoStatusBlock,
)
from .native_windows_abi import (
    ObjectAttributes as _ObjectAttributes,
)
from .native_windows_abi import (
    UnicodeString as _UnicodeString,
)

if TYPE_CHECKING:
    from collections.abc import Callable


def _ctypes_attribute(name: str) -> object:
    return getattr(ctypes, name)


def _win_dll() -> Callable[..., ctypes.CDLL]:
    return cast("Callable[..., ctypes.CDLL]", _ctypes_attribute("WinDLL"))


def _last_error() -> int:
    return cast("Callable[[], int]", _ctypes_attribute("get_last_error"))()


def _format_error(value: int) -> str:
    return cast("Callable[[int], str]", _ctypes_attribute("FormatError"))(value)


class _Msvcrt(Protocol):
    """The two CPython CRT bridge functions required for existing fd consumers."""

    def get_osfhandle(self, descriptor: int) -> int:
        """Return the OS handle owned by one CRT descriptor."""

    def open_osfhandle(self, handle: int, flags: int) -> int:
        """Transfer a native handle into a CRT descriptor."""


def _msvcrt() -> _Msvcrt:
    return cast("_Msvcrt", __import__("msvcrt"))


@dataclass(frozen=True, slots=True)
class WindowsFileInfo:
    """Identity and observable stability facts read from one Windows handle."""

    volume_serial: int
    file_id: int
    change_time: int
    last_write_time: int
    size: int
    directory: bool
    reparse: bool


class WindowsApi:
    """Typed ctypes adapter for handle-rooted Windows file operations."""

    def __init__(self) -> None:
        """Load the required kernel and ntdll entry points.

        Raises:
            OSError: If the current platform is not Windows.

        """
        if os.name != "nt":
            msg = "Windows handle operations are unavailable on this platform"
            raise OSError(msg)
        self.kernel32 = _win_dll()("kernel32", use_last_error=True)
        self.ntdll = _win_dll()("ntdll", use_last_error=True)
        self._configure_calls()

    def _configure_calls(self) -> None:
        self.kernel32.CreateFileW.argtypes = [
            ctypes.c_wchar_p,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_void_p,
        ]
        self.kernel32.CreateFileW.restype = ctypes.c_void_p
        self.kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
        self.kernel32.CloseHandle.restype = ctypes.c_int
        self.kernel32.GetFileInformationByHandleEx.argtypes = [
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_void_p,
            ctypes.c_ulong,
        ]
        self.kernel32.GetFileInformationByHandleEx.restype = ctypes.c_int
        self.kernel32.GetFileType.argtypes = [ctypes.c_void_p]
        self.kernel32.GetFileType.restype = ctypes.c_ulong
        self.kernel32.GetFinalPathNameByHandleW.argtypes = [
            ctypes.c_void_p,
            ctypes.c_wchar_p,
            ctypes.c_ulong,
            ctypes.c_ulong,
        ]
        self.kernel32.GetFinalPathNameByHandleW.restype = ctypes.c_ulong
        self.kernel32.SetFileInformationByHandle.argtypes = [
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_void_p,
            ctypes.c_ulong,
        ]
        self.kernel32.SetFileInformationByHandle.restype = ctypes.c_int
        self.kernel32.FlushFileBuffers.argtypes = [ctypes.c_void_p]
        self.kernel32.FlushFileBuffers.restype = ctypes.c_int
        self.ntdll.NtCreateFile.argtypes = [
            ctypes.POINTER(ctypes.c_void_p),
            ctypes.c_ulong,
            ctypes.POINTER(_ObjectAttributes),
            ctypes.POINTER(_IoStatusBlock),
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_ulong,
            ctypes.c_void_p,
            ctypes.c_ulong,
        ]
        self.ntdll.NtCreateFile.restype = ctypes.c_long
        self.ntdll.RtlNtStatusToDosError.argtypes = [ctypes.c_long]
        self.ntdll.RtlNtStatusToDosError.restype = ctypes.c_ulong

    def close(self, handle: int) -> None:
        """Close one owned native handle."""
        if not self.kernel32.CloseHandle(ctypes.c_void_p(handle)):
            self._raise_last("could not close handle")

    @staticmethod
    def descriptor_from_handle(handle: int, *, read_only: bool) -> int:
        """Transfer one handle to a binary CRT descriptor.

        Returns:
            A descriptor that takes ownership of the supplied native handle.

        """
        return _msvcrt().open_osfhandle(
            handle, O_BINARY | (O_READ_ONLY if read_only else O_READ_WRITE)
        )

    @staticmethod
    def handle_from_descriptor(descriptor: int) -> int:
        """Return the native handle retained by an open CRT descriptor.

        Returns:
            The handle valid only while the descriptor remains open.

        """
        return _msvcrt().get_osfhandle(descriptor)

    def open_directory(self, expression: str, root: int | None) -> int:
        """Open one directory through an absolute or captured-root expression.

        Returns:
            An owned directory handle.

        Raises:
            OSError: If root binding or the native open fails.

        """
        if root is None and _is_absolute(expression):
            return self._create_file_directory(expression)
        if root is None:
            msg = "relative directory requires the captured working directory"
            raise OSError(msg)
        return self._create_relative(
            root,
            expression,
            directory=True,
            no_follow=False,
            create=False,
        )

    def open_child(self, parent: int, name: str, *, no_follow: bool = False) -> int:
        """Open one child relative to a live parent handle.

        Returns:
            An owned child handle.

        """
        return self._create_relative(
            parent,
            name,
            directory=False,
            no_follow=no_follow,
            create=False,
        )

    def create_child(self, parent: int, name: str) -> int:
        """Create one exclusive child relative to a live parent handle.

        Returns:
            An owned exclusive stage handle.

        """
        return self._create_relative(
            parent,
            name,
            directory=False,
            no_follow=False,
            create=True,
        )

    def info(self, handle: int) -> WindowsFileInfo:
        """Query identity and observable stability facts from one handle.

        Returns:
            The volume/file identity, size, attributes, and timestamps.

        Raises:
            OSError: If the handle is not a disk file or metadata cannot be queried.

        """
        if self.kernel32.GetFileType(ctypes.c_void_p(handle)) != FILE_TYPE_DISK:
            msg = "opened object is not a disk file"
            raise OSError(msg)
        identifier = _FileIdInfo()
        basic = _FileBasicInfo()
        standard = _FileStandardInfo()
        attributes = _FileAttributeTagInfo()
        self._get_info(handle, FILE_ID_INFO, identifier)
        self._get_info(handle, FILE_BASIC_INFO, basic)
        self._get_info(handle, FILE_STANDARD_INFO, standard)
        self._get_info(handle, FILE_ATTRIBUTE_TAG_INFO, attributes)
        return WindowsFileInfo(
            identifier.VolumeSerialNumber,
            int.from_bytes(bytes(identifier.FileId), "little"),
            basic.ChangeTime,
            basic.LastWriteTime,
            standard.EndOfFile,
            bool(
                standard.Directory
                or attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY
            ),
            bool(attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT),
        )

    def final_path(self, handle: int) -> str | None:
        """Obtain a nullable normalized final-path snapshot from one handle.

        Returns:
            The final address when Windows can resolve it, otherwise null.

        """
        required = self.kernel32.GetFinalPathNameByHandleW(
            ctypes.c_void_p(handle), None, 0, 0
        )
        if required == 0:
            return None
        buffer = ctypes.create_unicode_buffer(required)
        observed = self.kernel32.GetFinalPathNameByHandleW(
            ctypes.c_void_p(handle), buffer, required, 0
        )
        return None if observed == 0 or observed >= required else buffer.value

    def publish_no_replace(self, stage: int, parent: int, name: str) -> None:
        """Rename an open stage into a live parent without replacement.

        Raises:
            FileExistsError: If the destination name is already occupied.

        """
        encoded = _utf16(name)
        size = 20 + len(encoded)
        buffer = (ctypes.c_ubyte * size)()
        ctypes.memset(buffer, 0, size)
        ctypes.cast(buffer, ctypes.POINTER(ctypes.c_ulong))[0] = 0
        ctypes.cast(ctypes.byref(buffer, 8), ctypes.POINTER(ctypes.c_void_p))[0] = (
            ctypes.c_void_p(parent)
        )
        ctypes.cast(ctypes.byref(buffer, 16), ctypes.POINTER(ctypes.c_ulong))[0] = len(
            encoded
        )
        ctypes.memmove(ctypes.byref(buffer, 20), encoded, len(encoded))
        if self.kernel32.SetFileInformationByHandle(
            ctypes.c_void_p(stage), FILE_RENAME_INFO_EX, buffer, size
        ):
            return
        error = _last_error()
        if error in {ERROR_FILE_EXISTS, ERROR_ALREADY_EXISTS}:
            raise FileExistsError(error, "destination already exists", name)
        self._raise_last("could not publish candidate")

    def discard_private_stage(self, handle: int) -> None:
        """Mark one private stage for deletion when its last handle closes."""
        delete = ctypes.c_ubyte(1)
        if not self.kernel32.SetFileInformationByHandle(
            ctypes.c_void_p(handle), FILE_DISPOSITION_INFO, ctypes.byref(delete), 1
        ):
            self._raise_last("could not remove private staging file")

    def sync_directory(self, handle: int) -> None:
        """Flush a live destination directory handle."""
        if not self.kernel32.FlushFileBuffers(ctypes.c_void_p(handle)):
            self._raise_last("could not sync destination directory")

    def _create_file_directory(self, expression: str) -> int:
        handle = self.kernel32.CreateFileW(
            expression,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_ALL,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
        if handle != INVALID_HANDLE_VALUE:
            return int(handle)
        return self._raise_last("could not open path parent")

    def _create_relative(
        self,
        parent: int,
        name: str,
        *,
        directory: bool,
        no_follow: bool,
        create: bool,
    ) -> int:
        buffer = ctypes.create_unicode_buffer(name)
        encoded = _utf16(name)
        string = _UnicodeString(len(encoded), len(encoded), ctypes.addressof(buffer))
        attributes = _ObjectAttributes(
            ctypes.sizeof(_ObjectAttributes),
            ctypes.c_void_p(parent),
            ctypes.pointer(string),
            0,
            None,
            None,
        )
        handle = ctypes.c_void_p()
        status = _IoStatusBlock()
        options = FILE_SYNCHRONOUS_IO_NONALERT
        if not directory:
            options |= FILE_NON_DIRECTORY_FILE
        if no_follow:
            options |= FILE_OPEN_REPARSE_POINT
        access = FILE_READ_ATTRIBUTES | SYNCHRONIZE
        if create:
            access |= FILE_READ_DATA | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | DELETE
        else:
            access |= FILE_READ_DATA
        result = self.ntdll.NtCreateFile(
            ctypes.byref(handle),
            access,
            ctypes.byref(attributes),
            ctypes.byref(status),
            None,
            0,
            FILE_SHARE_ALL,
            FILE_CREATE if create else FILE_OPEN,
            options,
            None,
            0,
        )
        if result >= 0 and handle.value is not None:
            return handle.value
        if result >= 0:
            message = "NtCreateFile returned a null handle"
            raise OSError(message)
        error = self.ntdll.RtlNtStatusToDosError(result)
        raise OSError(error, f"NtCreateFile failed with status {result:#x}", name)

    def _get_info(self, handle: int, kind: int, result: ctypes.Structure) -> None:
        if not self.kernel32.GetFileInformationByHandleEx(
            ctypes.c_void_p(handle), kind, ctypes.byref(result), ctypes.sizeof(result)
        ):
            self._raise_last("could not query handle identity")

    @staticmethod
    def _raise_last(message: str) -> NoReturn:
        error = _last_error()
        raise OSError(error, f"{message}: {_format_error(error)}")


def _utf16(value: str) -> bytes:
    return value.encode("utf-16-le", "strict")


def _is_absolute(value: str) -> bool:
    return value.startswith(("\\\\", "//")) or (
        len(value) >= MIN_DRIVE_ABSOLUTE_LENGTH and value[1:3] in {":\\", ":/"}
    )
