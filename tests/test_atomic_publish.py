# ruff: file-ignore[private-member-access]
"""Contracts for portable atomic no-replace publication."""

from __future__ import annotations

import ctypes
import errno
import sys
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from eml_attachment_remover import atomic_publish, models


def _native_operation(result: int) -> MagicMock:
    """Return a callable native operation.

    Returns:
        A synthetic C library exposing both supported function names.

    """
    operation = MagicMock(return_value=result)
    operation.argtypes = None
    ctypes_library = MagicMock()
    ctypes_library.renamex_np = operation
    ctypes_library.renameat2 = operation
    return ctypes_library


def test_darwin_native_no_replace_uses_exclusive_atomic_rename() -> None:
    library = _native_operation(0)
    with (
        patch.object(sys, "platform", new="darwin"),
        patch.object(ctypes, "CDLL", return_value=library) as load_library,
    ):
        published = atomic_publish._native_no_replace(
            Path("temporary.eml"),
            Path("destination.eml"),
        )

    assert published
    load_library.assert_called_once_with(None, use_errno=True)
    assert library.renamex_np.call_args.args == (
        b"temporary.eml",
        b"destination.eml",
        atomic_publish.RENAME_EXCL,
    )


def test_linux_native_no_replace_uses_renameat2_when_available() -> None:
    library = _native_operation(0)
    with (
        patch.object(sys, "platform", new="linux"),
        patch.object(ctypes, "CDLL", return_value=library),
    ):
        published = atomic_publish._native_no_replace(
            Path("temporary.eml"),
            Path("destination.eml"),
        )

    assert published
    assert library.renameat2.call_args.args == (
        atomic_publish.AT_FDCWD,
        b"temporary.eml",
        atomic_publish.AT_FDCWD,
        b"destination.eml",
        atomic_publish.RENAME_NOREPLACE,
    )
    assert library.renameat2.restype is ctypes.c_int


@pytest.mark.parametrize("platform_name", ["darwin", "linux", "win32"])
def test_unavailable_native_operation_requests_portable_fallback(
    platform_name: str,
) -> None:
    library = MagicMock(spec=[])
    with (
        patch.object(sys, "platform", new=platform_name),
        patch.object(ctypes, "CDLL", return_value=library),
    ):
        assert not atomic_publish._native_no_replace(
            Path("temporary.eml"),
            Path("destination.eml"),
        )


@pytest.mark.parametrize("error_number", [errno.ENOSYS, errno.ENOTSUP])
def test_unsupported_native_operation_requests_hardlink_fallback(
    error_number: int,
) -> None:
    library = _native_operation(-1)
    with (
        patch.object(sys, "platform", new="darwin"),
        patch.object(ctypes, "CDLL", return_value=library),
        patch.object(ctypes, "get_errno", return_value=error_number),
    ):
        assert not atomic_publish._native_no_replace(
            Path("temporary.eml"),
            Path("destination.eml"),
        )


def test_native_existing_destination_is_conflict() -> None:
    temporary = Path("temporary.eml")
    destination = Path("destination.eml")
    with (
        patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
        patch.object(
            atomic_publish,
            "_native_no_replace",
            side_effect=FileExistsError("PUBLIC OCCUPIED"),
        ),
    ):
        with pytest.raises(models.CliError) as raised:
            atomic_publish.publish_without_clobber(temporary, destination)

    assert raised.value.code is models.ExitCode.OUTPUT_CONFLICT
    assert raised.value.message == (
        "output was created by another process: destination.eml"
    )


def test_successful_native_publication_bypasses_hardlink_fallback() -> None:
    temporary = Path("temporary.eml")
    destination = Path("destination.eml")
    with (
        patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
        patch.object(
            atomic_publish,
            "_native_no_replace",
            return_value=True,
        ) as native_no_replace,
        patch.object(
            atomic_publish,
            "_link_and_remove_temporary",
        ) as link_and_remove,
    ):
        atomic_publish.publish_without_clobber(temporary, destination)

    native_no_replace.assert_called_once_with(temporary, destination)
    link_and_remove.assert_not_called()


def test_native_unexpected_failure_retains_write_error_when_target_is_absent() -> None:
    library = _native_operation(-1)
    with (
        patch.object(sys, "platform", new="darwin"),
        patch.object(ctypes, "CDLL", return_value=library),
        patch.object(ctypes, "get_errno", return_value=errno.EIO),
    ):
        with pytest.raises(OSError, match=r"destination\.eml"):
            atomic_publish._native_no_replace(
                Path("temporary.eml"),
                Path("destination.eml"),
            )


@pytest.mark.parametrize("target_state", ["absent", "present"])
def test_publication_os_error_is_classified_by_the_final_target(
    target_state: str,
) -> None:
    temporary = Path("temporary.eml")
    destination = MagicMock(spec=Path)
    if target_state == "present":
        destination.lstat.return_value = MagicMock()
    else:
        destination.lstat.side_effect = OSError("PUBLIC ABSENT")
    with (
        patch.object(atomic_publish.os, "name", new="posix"),  # type: ignore[attr-defined]
        patch.object(
            atomic_publish,
            "_native_no_replace",
            side_effect=OSError("PUBLIC FAILURE"),
        ),
    ):
        with pytest.raises(models.CliError) as raised:
            atomic_publish.publish_without_clobber(temporary, destination)

    expected = (
        models.ExitCode.OUTPUT_CONFLICT
        if target_state == "present"
        else models.ExitCode.WRITE_ERROR
    )
    assert raised.value.code is expected
    expected_message = (
        f"output was created by another process: {destination}"
        if target_state == "present"
        else f"could not publish verified output at {destination}: PUBLIC FAILURE"
    )
    assert raised.value.message == expected_message


def test_native_publish_works_where_hardlinks_are_unavailable() -> None:
    with tempfile.TemporaryDirectory() as directory:
        base = Path(directory)
        temporary = base / "temporary.eml"
        destination = base / "destination.eml"
        temporary.write_bytes(b"PUBLIC VERIFIED")
        with patch.object(
            Path,
            "hardlink_to",
            side_effect=OSError("PUBLIC HARDLINK UNSUPPORTED"),
        ):
            atomic_publish.publish_without_clobber(temporary, destination)

        assert destination.read_bytes() == b"PUBLIC VERIFIED"
        assert not temporary.exists()
