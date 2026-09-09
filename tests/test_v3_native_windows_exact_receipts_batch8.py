"""Exact Windows publication and primary native-value mutation receipts."""

from __future__ import annotations

from dataclasses import dataclass, field

import pytest

from eml_attachment_remover import native_values, native_windows
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.native_windows import WindowsApi
from eml_attachment_remover.native_windows_abi import FILE_RENAME_INFORMATION_EX


def test_windows_value_validation_and_default_destination_keep_every_terminal_form(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        for value in ("C:\\safe\\leaf\\", "C:/safe/leaf/", "C:\\safe\\bad\x00.eml"):
            with pytest.raises(AppError) as captured:
                native_values.validate_windows_argument(value)
            assert captured.value == AppError(
                ExitCode.INPUT_ERROR, "path has an empty basename"
            )
        assert native_values.default_destination("C:\\safe\\Mail.EML") == (
            "C:\\safe\\Mail.mime-pruned.eml"
        )

    with pytest.raises(AppError) as posix_error:
        native_values._validate_posix("folder/")  # ruff: ignore[private-member-access] - POSIX trailing paths have no leaf entry.
    assert posix_error.value == AppError(
        ExitCode.INPUT_ERROR, "path has an empty basename"
    )


@dataclass
class Ntdll:
    """Record native rename packets and translate one selected failure status."""

    result: int
    translated: int = 5
    statuses: list[int] = field(default_factory=list)
    packets: list[bytes] = field(default_factory=list)
    tails: list[tuple[object, ...]] = field(default_factory=list)

    # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
    def NtSetInformationFile(self, *arguments: object) -> int:
        packet = getattr(arguments[2], "_obj", None)
        assert packet is not None
        self.packets.append(bytes(packet))
        self.tails.append(arguments[3:])
        return self.result

    # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
    def RtlNtStatusToDosError(self, status: int) -> int:
        self.statuses.append(status)
        return self.translated


def test_windows_publish_packet_accepts_full_unsigned_handle_and_exact_layout() -> None:
    ntdll = Ntdll(result=0)
    api = object.__new__(WindowsApi)
    api.__dict__["ntdll"] = ntdll
    parent = 0xFEDCBA9876543210
    api.publish_no_replace(0x1234, parent, "out.eml")

    assert len(ntdll.packets) == 1
    packet = ntdll.packets[0]
    encoded = b"o\x00u\x00t\x00.\x00e\x00m\x00l\x00"
    assert int.from_bytes(packet[0:4], "little") == 0
    assert int.from_bytes(packet[8:16], "little") == parent
    assert int.from_bytes(packet[16:20], "little") == len(encoded)
    assert packet[20 : 20 + len(encoded)] == encoded
    assert ntdll.tails == [(20 + len(encoded), FILE_RENAME_INFORMATION_EX)]


def test_windows_publish_translates_the_actual_negative_native_status(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ntdll = Ntdll(result=-0x1234, translated=5)
    api = object.__new__(WindowsApi)
    api.__dict__["ntdll"] = ntdll
    monkeypatch.setattr(native_windows, "_format_error", lambda value: f"win-{value}")

    with pytest.raises(
        OSError, match=r"could not publish candidate: win-5"
    ) as captured:
        api.publish_no_replace(0x1234, 0x5678, "out.eml")
    assert (captured.value.errno, captured.value.strerror) == (
        5,
        "could not publish candidate: win-5",
    )
    assert ntdll.statuses == [-0x1234]
