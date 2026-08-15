"""Write and atomically commit verified derived EML files."""

from __future__ import annotations

import os
import tempfile
from contextlib import suppress
from pathlib import Path
from typing import TYPE_CHECKING, BinaryIO

from .mime_serialization import (
    _leaf_fingerprints,
    _serialize_to_stream,
    _structure_fingerprint,
    _verify_serialized_message,
)
from .models import CliError, ExitCode, OutputPlan
from .output_commit import _apply_source_mode, _commit_output

if TYPE_CHECKING:
    from email.message import EmailMessage


def _write_open_temporary(
    output: BinaryIO,
    message: EmailMessage,
    raw: bytes,
    *,
    modified: bool,
) -> None:
    """Write and synchronize one already-open temporary stream."""
    if modified:
        _serialize_to_stream(message, output)
    else:
        output.write(raw)
    output.flush()
    os.fsync(output.fileno())


def _write_temporary(
    destination: Path,
    message: EmailMessage,
    raw: bytes,
    *,
    modified: bool,
) -> Path:
    """Write output through an exclusive temporary file beside the destination.

    Returns:
        The path of the fully flushed temporary file.

    Raises:
        CliError: If the temporary file cannot be created or written.

    """
    try:
        descriptor, raw_path = tempfile.mkstemp(
            prefix=".eml-remove-",
            suffix=".tmp",
            dir=destination.parent,
        )
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not create a temporary file in {destination.parent}: {exc}",
        ) from exc
    temporary = Path(raw_path)
    try:
        with os.fdopen(descriptor, "wb") as output:
            _write_open_temporary(output, message, raw, modified=modified)
    except (OSError, UnicodeError, ValueError) as exc:
        with suppress(OSError):
            os.close(descriptor)
        try:
            temporary.unlink()
        except OSError as cleanup_exc:
            raise CliError(
                ExitCode.WRITE_ERROR,
                f"could not write temporary output {temporary}: {exc}; "
                f"could not remove sensitive temporary file: {cleanup_exc}",
            ) from exc
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not write temporary output {temporary}: {exc}",
        ) from exc
    return temporary


def _temporary_size(temporary: Path) -> int:
    """Return the completed temporary file size as a stable write result.

    Returns:
        The temporary file size in bytes.

    Raises:
        CliError: If the temporary file cannot be inspected.

    """
    try:
        return temporary.stat().st_size
    except OSError as exc:
        raise CliError(
            ExitCode.WRITE_ERROR,
            f"could not inspect temporary output {temporary}: {exc}",
        ) from exc


def _verify_unchanged_output(temporary: Path, expected: bytes) -> None:
    """Require an unmodified output to remain byte-for-byte identical.

    Raises:
        CliError: If the temporary file cannot be read or its bytes changed.

    """
    try:
        actual = temporary.read_bytes()
    except OSError as exc:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            f"could not read unchanged output for verification: {exc}",
        ) from exc
    if actual != expected:
        raise CliError(
            ExitCode.VERIFICATION_ERROR,
            "unchanged output did not preserve the source bytes exactly",
        )


def _produce_output(plan: OutputPlan) -> tuple[int, str | None]:
    """Write, verify, permission-adjust, and commit one output file.

    Returns:
        The final output size and any permission-copy warning.

    Raises:
        CliError: If writing, verification, publication, or cleanup fails.

    """
    expected = _leaf_fingerprints(plan.message)
    expected_structure = _structure_fingerprint(plan.message)
    temporary = _write_temporary(
        plan.destination,
        plan.message,
        plan.raw,
        modified=plan.modified,
    )
    cleanup_state = "output was not published"
    try:
        if plan.modified:
            _verify_serialized_message(temporary, expected, expected_structure)
        else:
            _verify_unchanged_output(temporary, plan.raw)
        mode_warning = _apply_source_mode(plan.source, temporary)
        output_size = _temporary_size(temporary)
        _commit_output(
            temporary,
            plan.destination,
            plan.source,
            force=plan.force,
        )
        cleanup_state = "published output is valid"
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass
        except OSError as exc:
            raise CliError(
                ExitCode.WRITE_ERROR,
                f"could not remove sensitive temporary file {temporary} "
                f"({cleanup_state}): {exc}",
            ) from exc
    return output_size, mode_warning
