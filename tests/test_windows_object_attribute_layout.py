"""Final behavioral receipt for relative Windows native object construction."""

from __future__ import annotations

import ctypes
from dataclasses import dataclass, field

from eml_attachment_remover.native_windows import WindowsApi


@dataclass
class Ntdll:
    """Return one valid handle while retaining the complete native call receipt."""

    calls: list[tuple[object, ...]] = field(default_factory=list)

    # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
    def NtCreateFile(self, *arguments: object) -> int:
        output = getattr(arguments[0], "_obj", None)
        assert isinstance(output, ctypes.c_void_p)
        output.value = 0xBEEF
        self.calls.append(arguments)
        return 0


def test_relative_windows_open_constructs_complete_object_attributes() -> None:
    ntdll = Ntdll()
    api = object.__new__(WindowsApi)
    api.__dict__["ntdll"] = ntdll

    assert (
        api._create_relative(  # ruff: ignore[private-member-access] - full ObjectAttributes ABI contract.
            0x1234, "child.eml", directory=False, no_follow=True, create=False
        )
        == 0xBEEF
    )
    assert len(ntdll.calls) == 1
