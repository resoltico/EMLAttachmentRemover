"""Exact successful-directory-sync receipt for the Windows native adapter."""

from __future__ import annotations

from dataclasses import dataclass, field

from eml_attachment_remover.native_windows import WindowsApi


@dataclass
class Kernel:
    """Record the non-null native directory handle passed to a successful sync."""

    handles: list[int | None] = field(default_factory=list)

    # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
    def FlushFileBuffers(self, value: object) -> int:
        handle = getattr(value, "value", None)
        assert isinstance(handle, int)
        self.handles.append(handle)
        return 1


def test_windows_directory_sync_passes_the_owned_handle_not_a_null_pointer() -> None:
    kernel = Kernel()
    api = object.__new__(WindowsApi)
    api.__dict__["kernel32"] = kernel

    api.sync_directory(0xBEEF)
    assert kernel.handles == [0xBEEF]
