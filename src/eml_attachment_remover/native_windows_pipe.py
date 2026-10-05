"""Non-consuming Windows pipe access checks through documented object information."""

from __future__ import annotations

import ctypes
import errno

from .native_windows_abi import FILE_WRITE_DATA
from .native_windows_runtime import Msvcrt, ctypes_attribute


class ObjectBasicInformation(ctypes.Structure):
    """Fixed-width PUBLIC_OBJECT_BASIC_INFORMATION layout from winternl.h."""

    _fields_ = (
        ("attributes", ctypes.c_uint32),
        ("granted_access", ctypes.c_uint32),
        ("handle_count", ctypes.c_uint32),
        ("pointer_count", ctypes.c_uint32),
        ("reserved", ctypes.c_uint32 * 10),
    )


def has_write_access(descriptor: int) -> bool:
    """Inspect granted access without writing, consuming, or closing the pipe.

    Returns:
        Whether the handle grants FILE_WRITE_DATA.

    Raises:
        OSError: If bounded native object information cannot be obtained.

    """
    library = ctypes_attribute("WinDLL")("ntdll", use_last_error=True)
    operation = library.NtQueryObject
    operation.argtypes = [
        ctypes.c_void_p,
        ctypes.c_int,
        ctypes.c_void_p,
        ctypes.c_uint32,
        ctypes.POINTER(ctypes.c_uint32),
    ]
    operation.restype = ctypes.c_int32
    information = ObjectBasicInformation()
    returned = ctypes.c_uint32()
    result = operation(
        ctypes.c_void_p(Msvcrt.get_osfhandle(descriptor)),
        0,
        ctypes.byref(information),
        ctypes.sizeof(information),
        ctypes.byref(returned),
    )
    if result < 0 or returned.value != ctypes.sizeof(information):
        raise OSError(errno.EBADF, "progress pipe access information is unavailable")
    return bool(information.granted_access & FILE_WRITE_DATA)
