"""Final exact receipts for non-equivalent native adapter survivors."""

from __future__ import annotations

import stat
from dataclasses import dataclass

import pytest

from eml_attachment_remover import native_posix, native_values
from eml_attachment_remover.domain import AppError, ExitCode
from eml_attachment_remover.native_windows import WindowsApi, WindowsFileInfo
from eml_attachment_remover.native_windows_abi import (
    FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO,
    FILE_ID_INFO,
    FILE_STANDARD_INFO,
    FILE_TYPE_DISK,
    FileAttributeTagInfo,
    FileBasicInfo,
    FileIdInfo,
    FileStandardInfo,
)


@dataclass(frozen=True)
class DirectoryMetadata:
    """Minimal nonregular POSIX metadata receipt."""

    st_dev: int = 7
    st_ino: int = 8
    st_mode: int = stat.S_IFDIR | 0o700
    st_ctime_ns: int = 901


def test_posix_inspection_reports_exact_nonregular_source_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    closes: list[int] = []
    monkeypatch.setattr(native_posix, "_parent", lambda _path: (41, "parent", b"mail"))
    monkeypatch.setattr(native_posix.__dict__["os"], "open", lambda *_args, **_kw: 42)
    monkeypatch.setattr(
        native_posix.__dict__["os"], "fstat", lambda _fd: DirectoryMetadata()
    )
    monkeypatch.setattr(native_posix.__dict__["os"], "close", closes.append)

    with pytest.raises(AppError) as captured:
        native_posix._inspect_source_identity("mail")  # ruff: ignore[private-member-access] - source kind is checked after an opened descriptor.
    assert captured.value == AppError(
        ExitCode.INPUT_ERROR, "selected source is not a regular file"
    )
    assert closes == [42, 41]


def test_windows_info_keeps_zero_attribute_bits_as_regular_and_not_reparse(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    @dataclass
    class Kernel:
        @staticmethod
        # ruff: ignore[invalid-function-name] - mirrors the Windows ABI entry point.
        def GetFileType(_handle: object) -> int:
            return FILE_TYPE_DISK

    def get_info(_self: WindowsApi, _handle: int, kind: int, result: object) -> None:
        if kind == FILE_ID_INFO:
            assert isinstance(result, FileIdInfo)
            result.VolumeSerialNumber = 7
        elif kind == FILE_BASIC_INFO:
            assert isinstance(result, FileBasicInfo)
            result.ChangeTime = 901
            result.LastWriteTime = 902
        elif kind == FILE_STANDARD_INFO:
            assert isinstance(result, FileStandardInfo)
            result.EndOfFile = 3
            result.Directory = 0
        elif kind == FILE_ATTRIBUTE_TAG_INFO:
            assert isinstance(result, FileAttributeTagInfo)
            result.FileAttributes = 0
        else:
            pytest.fail(f"unexpected kind {kind}")

    api = object.__new__(WindowsApi)
    api.__dict__["kernel32"] = Kernel()
    monkeypatch.setattr(WindowsApi, "_get_info", get_info)
    assert api.info(99) == WindowsFileInfo(
        volume_serial=7,
        file_id=0,
        change_time=901,
        last_write_time=902,
        size=3,
        directory=False,
        reparse=False,
    )


def test_native_values_windows_defaults_preserve_path_grammar_and_utf16_evidence(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    with monkeypatch.context() as context:
        context.setattr(native_values.__dict__["os"], "name", "nt")
        assert native_values.default_destination("C:\\odd\\Mail.EML") == (
            "C:\\odd\\Mail.mime-pruned.eml"
        )
        assert native_values.path_value("C:\\π.eml").native_utf16le_base64 == (
            "QwA6AFwAwAMuAGUAbQBsAA=="
        )
