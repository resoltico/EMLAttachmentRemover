"""ABI-shape receipt for a complete relative Windows native open."""

from __future__ import annotations

import ctypes
from dataclasses import dataclass, field

import pytest

from eml_attachment_remover.native_windows import WindowsApi


@dataclass
class Ntdll:
    """Capture the complete NtCreateFile argument vector for one success."""

    calls: list[tuple[object, ...]] = field(default_factory=list)

    # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
    def NtCreateFile(self, *arguments: object) -> int:
        output = getattr(arguments[0], "_obj", None)
        assert isinstance(output, ctypes.c_void_p)
        output.value = 0xFEED
        self.calls.append(arguments)
        return 0


@pytest.mark.parametrize("writable", [False, True])
def test_relative_windows_open_reaches_ntcreatefile_with_all_abi_arguments(
    *, writable: bool
) -> None:
    ntdll = Ntdll()
    api = object.__new__(WindowsApi)
    api.__dict__["ntdll"] = ntdll

    access_options = {"writable": True} if writable else {}
    assert (
        api._create_relative(  # ruff: ignore[private-member-access] - full ObjectAttributes construction must precede this native call.
            0x11,
            "child",
            directory=True,
            no_follow=False,
            create=False,
            **access_options,
        )
        == 0xFEED
    )
    assert len(ntdll.calls) == 1
    assert len(ntdll.calls[0]) == 11

    access = ntdll.calls[0][1]
    assert isinstance(access, int)
    assert bool(access & 0x2) is writable
