"""Native path representation, validation, and suffix derivation."""

from __future__ import annotations

import base64
import codecs
import ntpath
import os
import pathlib
import unicodedata
from typing import Final

from .domain import AppError, ExitCode, PathValue
from .native_windows import WindowsApi

MAX_PATH_BYTES: Final = 32 * 1024
MAX_RAW_BYTES: Final = 128 * 1024 * 1024
_WINDOWS_RESERVED: Final = {"CON", "PRN", "AUX", "NUL"} | {
    f"{prefix}{number}" for prefix in ("COM", "LPT") for number in range(1, 10)
}


def _base64_text(raw: bytes) -> str:
    """Encode known ASCII-safe evidence bytes as text.

    Returns:
        The canonical Base64 text for the supplied binary evidence.

    """
    return base64.b64encode(raw).decode()


def _utf16le(value: str) -> bytes:
    """Encode one Windows-native name with the codec's strict default policy.

    Returns:
        The raw UTF-16LE bytes used by native Windows APIs.

    """
    return codecs.utf_16_le_encode(value)[0]


def path_value(value: str) -> PathValue:
    """Return a platform-native, lossless path value for schema serialization.

    Returns:
        POSIX raw-byte or Windows UTF-16LE evidence plus a safe display string.

    """
    if os.name == "nt":
        return PathValue(
            value,
            _display(value),
            None,
            _base64_text(_utf16le(value)),
        )
    return PathValue(
        value,
        _display(value),
        _base64_text(os.fsencode(value)),
    )


def validate_argument(value: str) -> str:
    """Validate then explicitly expand one raw argv path without normalization.

    Returns:
        The expanded form that preserves all ordinary path traversal components.

    """
    expanded = os.fspath(pathlib.Path(value).expanduser())
    if os.name == "nt":
        validate_windows_argument(expanded)
    else:
        _validate_posix(expanded)
    return expanded


def validate_windows_argument(value: str) -> None:
    """Reject Windows path forms unsafe for handle-relative ordinary-file access.

    Raises:
        AppError: If a drive-relative, ADS, reserved, aliased, or invalid path appears.

    """
    try:
        raw = _utf16le(value)
    except UnicodeEncodeError as exc:
        message = "path contains an unpaired surrogate"
        raise AppError(ExitCode.INPUT_ERROR, message) from exc
    _validate_windows_namespace(value)
    drive, tail = ntpath.splitdrive(value)
    basename = ntpath.basename(value)
    if "\x00" in value or not basename:
        raise AppError(ExitCode.INPUT_ERROR, "path has an empty basename")
    if basename in {".", ".."}:
        raise AppError(ExitCode.INPUT_ERROR, "path basename may not be . or ..")
    if drive and not tail.startswith(("\\", "/")):
        raise AppError(ExitCode.INPUT_ERROR, "drive-relative paths are unsafe")
    if ":" in tail:
        raise AppError(ExitCode.INPUT_ERROR, "alternate data streams are unsafe")
    if len(raw) > MAX_PATH_BYTES:
        raise AppError(
            ExitCode.INPUT_ERROR, "path exceeds the 32 KiB native-path limit"
        )
    _validate_windows_components(tail)


def _validate_windows_namespace(value: str) -> None:
    """Reject device and unsafe extended namespace roots.

    Raises:
        AppError: If a path names a device-object or unsafe extended namespace.

    """
    if value.startswith("\\\\.\\"):
        raise AppError(ExitCode.INPUT_ERROR, "device path namespace is unsafe")
    if value.startswith("\\\\?\\"):
        suffix = value.removeprefix("\\\\?\\")
        drive_path = suffix[1:3] == ":\\"
        unc_path = suffix.casefold().startswith("unc\\")
        if not drive_path and not unc_path:
            raise AppError(ExitCode.INPUT_ERROR, "extended path namespace is unsafe")


def default_destination(source: str) -> str:
    """Derive the v3 suffix beside the requested directory entry.

    Returns:
        A path retaining the original parent expression and a ``.mime-pruned.eml`` name.

    """
    parent, name = ntpath.split(source) if os.name == "nt" else os.path.split(source)
    stem = name[:-4] if name.lower().endswith(".eml") else name
    suffix = stem + ".mime-pruned.eml"
    if os.name == "nt":
        return ntpath.join(parent, suffix)
    return os.fspath(pathlib.Path(parent) / suffix)


def require_native_backend() -> None:
    """Ensure the required Windows handle-rooted no-replace backend can be loaded.

    Raises:
        AppError: If the current Windows runtime lacks the required native API surface.

    """
    if os.name != "nt":
        return
    try:
        WindowsApi()
    except OSError as exc:
        message = "Windows native handle backend is unavailable"
        raise AppError(ExitCode.WRITE_ERROR, message) from exc


def _display(value: str) -> str:
    return "".join(
        character
        if unicodedata.category(character) not in {"Cc", "Cf", "Cs", "Zl", "Zp"}
        else f"\\u{ord(character):04x}"
        for character in value
    )


def _validate_posix(value: str) -> None:
    raw = os.fsencode(value)
    _parent, _separator, basename = raw.rpartition(b"/")
    if b"\x00" in raw or not basename:
        raise AppError(ExitCode.INPUT_ERROR, "path has an empty basename")
    if basename in {b".", b".."}:
        raise AppError(ExitCode.INPUT_ERROR, "path basename may not be . or ..")
    if len(raw) > MAX_PATH_BYTES:
        raise AppError(
            ExitCode.INPUT_ERROR, "path exceeds the 32 KiB native-path limit"
        )


def _validate_windows_components(tail: str) -> None:
    for component in tail.replace("/", "\\").split("\\"):
        if not component:
            continue
        if component[-1] in {".", " "}:
            raise AppError(ExitCode.INPUT_ERROR, "path has a trailing dot or space")
        if component.partition(".")[0].upper() in _WINDOWS_RESERVED:
            raise AppError(ExitCode.INPUT_ERROR, "path contains a reserved DOS name")
