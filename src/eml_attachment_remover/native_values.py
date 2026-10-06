"""Native path representation, validation, and suffix derivation."""

from __future__ import annotations

import base64
import codecs
import ntpath
import os
import posixpath
import unicodedata
from collections.abc import Mapping
from typing import Final

from .domain import AppError, ExitCode, PathValue
from .native_windows import WindowsApi

MAX_PATH_BYTES: Final = 32 * 1024
MAX_RAW_BYTES: Final = 128 * 1024 * 1024
# Longest published final address a receipt may carry, in native units (bytes on
# POSIX, UTF-16 units on Windows). Report admission reserves exactly this much.
MAX_ADDRESS_UNITS: Final = 4096
UTF16_UNIT_BYTES: Final = 2
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
            safe_display(value),
            None,
            _base64_text(_utf16le(value)),
        )
    return PathValue(
        value,
        safe_display(value),
        _base64_text(os.fsencode(value)),
    )


def report_path_bytes(value: PathValue) -> bytes:
    """Return validated POSIX bytes for one proven report path.

    Returns:
        Exact native bytes for consumer transport.

    Raises:
        ValueError: If native report evidence is absent or unsafe.

    """
    if value.native_base64 is not None:
        native = base64.b64decode(value.native_base64, validate=True)
    elif value.text is not None:
        native = os.fsencode(value.text)
    else:
        message = "accepted final address lacks native path evidence"
        raise ValueError(message)
    if b"\0" in native:
        message = "accepted final address contains NUL"
        raise ValueError(message)
    return native


def report_path_value(value: object) -> PathValue | None:
    """Rebuild a serialized report path so every channel shares one serializer.

    Returns:
        The path evidence, or ``None`` when the record holds no path.

    """
    if not isinstance(value, Mapping):
        return None
    text = value.get("text")
    native = value.get("native_base64")
    utf16 = value.get("native_utf16le_base64")
    return PathValue(
        text if isinstance(text, str) else None,
        str(value.get("display")),
        native if isinstance(native, str) else None,
        utf16 if isinstance(utf16, str) else None,
    )


def _native_text(value: object) -> str:
    """Rebuild a serialized path's native text from its exact evidence.

    Returns:
        The decoded native text, or an empty string when the record has no path.

    """
    if not isinstance(value, Mapping):
        return ""
    posix = value.get("native_base64")
    if isinstance(posix, str):
        return os.fsdecode(base64.b64decode(posix))
    wide = value.get("native_utf16le_base64")
    if isinstance(wide, str):
        return base64.b64decode(wide).decode("utf-16-le", "surrogatepass")
    text = value.get("text")
    # A display is already escaped and escaping is idempotent: a safe last resort.
    return text if isinstance(text, str) else str(value.get("display") or "")


def address_units(address: PathValue) -> int:
    """Measure a resolved address in its backend's own unit.

    Returns:
        Bytes for a POSIX address, UTF-16 code units for a Windows address.

    """
    if address.native_base64 is not None:
        return len(base64.b64decode(address.native_base64))
    wide = base64.b64decode(str(address.native_utf16le_base64))
    return len(wide) // UTF16_UNIT_BYTES


def require_reportable_destination(
    parent_address: PathValue | None, basename: bytes | str
) -> None:
    """Refuse, before anything is published, an address the report cannot carry.

    The published file's final address is its resolved parent plus one separator and
    its basename. If the parent cannot be resolved, or the sum exceeds the address
    bound, the receipt could only fail after the copy became visible.

    Raises:
        AppError: If the destination's future final address cannot be reported.

    """
    if parent_address is None:
        message = "could not resolve the destination directory address"
        raise AppError(ExitCode.WRITE_ERROR, message)
    name_units = (
        len(basename)
        if isinstance(basename, bytes)
        else len(basename.encode("utf-16-le", "surrogatepass")) // UTF16_UNIT_BYTES
    )
    if address_units(parent_address) + 1 + name_units > MAX_ADDRESS_UNITS:
        message = "destination address exceeds the reportable limit"
        raise AppError(ExitCode.WRITE_ERROR, message)


def planned_display(destination: Mapping[str, object]) -> str:
    """Join a planned destination in its backend's native grammar, then escape once.

    Presentation text is never joined with raw path text: the folder and basename
    are rebuilt from their native evidence, and only the whole is made displayable.

    Returns:
        A single-line, control-free display of the planned path.

    """
    folder = _native_text(destination.get("parent"))
    posix = destination.get("basename_base64")
    separators: tuple[str, ...]
    if isinstance(posix, str):
        name, separators = os.fsdecode(base64.b64decode(posix)), ("/",)
    else:
        wide = base64.b64decode(str(destination.get("basename_utf16le_base64")))
        name, separators = wide.decode("utf-16-le", "surrogatepass"), ("\\", "/")
    joined = folder if folder.endswith(separators) else folder + separators[0]
    return safe_display(joined + name)


def validate_argument(value: str) -> str:
    """Validate one literal argv path without shell-style expansion.

    Returns:
        The exact path expression supplied by the caller.

    """
    _validate_file_argument(value)
    return value


def _validate_file_argument(value: str) -> None:
    """Validate one file-address spelling before any expansion can erase syntax."""
    if os.name == "nt":
        validate_windows_argument(value)
    else:
        _validate_posix(value)


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
    """Derive the MIME-pruned suffix beside the requested directory entry.

    Returns:
        A path retaining the original parent expression and a ``.mime-pruned.eml`` name.

    """
    split = ntpath.split if os.name == "nt" else posixpath.split
    parent, name = split(source)
    stem = name[:-4] if name.lower().endswith(".eml") else name
    suffix = stem + ".mime-pruned.eml"
    if os.name == "nt":
        return ntpath.join(parent, suffix)
    return f"{parent}/{suffix}" if parent else suffix


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


def safe_display(value: str) -> str:
    """Escape every character a terminal or log line must not interpret.

    Returns:
        The text with control, format, surrogate, and line-separator characters
        replaced by four-digit Unicode escapes.

    """
    if value.isprintable():
        # str.isprintable is false for every character of the Unicode categories Other
        # (Cc, Cf, Cs, Co, Cn) and Separator (Zl, Zp, Zs but the ASCII space). The set
        # escaped below is a subset of those, so printable text has nothing to escape
        # and is returned as is, at C speed instead of one lookup per character.
        return value
    return "".join(
        character
        if unicodedata.category(character) not in {"Cc", "Cf", "Cs", "Zl", "Zp"}
        else f"\\u{ord(character):04x}"
        for character in value
    )


def has_surrogate(text: str) -> bool:
    """Return whether text holds a code point in the surrogate range.

    Strict UTF-8, which is what ``str.encode`` does without arguments, refuses exactly
    the surrogate code points, so encoding is the whole test.

    Returns:
        Whether any character lies in U+D800 through U+DFFF.

    """
    try:
        text.encode()
    except UnicodeEncodeError:
        return True
    return False


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


class NativeName:
    """Require the exact native name type at the platform dispatch boundary."""

    @staticmethod
    def byte(value: bytes | str) -> bytes:
        """Return one POSIX byte name or reject an incompatible text value.

        Returns:
            The unchanged native bytes.

        Raises:
            TypeError: If the supplied name is not bytes.

        """
        if isinstance(value, bytes):
            return value
        message = "POSIX native name must be bytes"
        raise TypeError(message)

    @staticmethod
    def unicode(value: bytes | str) -> str:
        """Return one Windows Unicode name or reject incompatible bytes.

        Returns:
            The unchanged native Unicode text.

        Raises:
            TypeError: If the supplied name is not Unicode text.

        """
        if isinstance(value, str):
            return value
        message = "Windows native name must be Unicode"
        raise TypeError(message)
