"""Entry-point, runtime, source-version, and single-file facade coverage."""

from __future__ import annotations

import importlib
import platform
import runpy
import sys
from importlib.metadata import PackageNotFoundError
from typing import TYPE_CHECKING, cast

import pytest

from eml_attachment_remover import app, processing, runtime
from eml_attachment_remover.domain import ItemStatus

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path

    from _pytest.capture import CaptureFixture
    from _pytest.monkeypatch import MonkeyPatch


def test_runtime_support_and_main_cover_supported_and_rejected_interpreters(
    monkeypatch: MonkeyPatch, capsys: CaptureFixture[str]
) -> None:
    assert runtime.runtime_supported("CPython", (3, 14)) is True
    assert runtime.runtime_supported("PyPy", (3, 14)) is False
    monkeypatch.setattr(platform, "python_implementation", lambda: "PyPy")
    monkeypatch.setattr(sys, "version_info", (3, 13, 0, "final", 0))
    assert runtime.main() == 1
    assert "requires CPython 3.14.x" in capsys.readouterr().err
    monkeypatch.setattr(platform, "python_implementation", lambda: "CPython")
    monkeypatch.setattr(sys, "version_info", (3, 14, 7, "final", 0))
    monkeypatch.setitem(runtime.__dict__, "application_main", lambda: 17)
    assert runtime.main() == 17


def test_module_entrypoint_delegates_to_runtime(monkeypatch: MonkeyPatch) -> None:
    monkeypatch.setitem(runtime.__dict__, "main", lambda: 19)
    with pytest.raises(SystemExit) as captured:
        runpy.run_module("eml_attachment_remover.__main__", run_name="__main__")
    assert captured.value.code == 19


def test_app_facade_and_single_file_processing_use_batch_pipeline(
    tmp_path: Path,
) -> None:
    importlib.reload(app)
    source = tmp_path / "message.eml"
    source.write_bytes(b"Content-Type: text/plain\r\n\r\nbody\r\n")
    ledger = processing.process_file(str(source), dry_run=True)
    assert ledger.items[0].status is ItemStatus.WOULD_CREATE
    assert callable(app.main)


def test_source_version_uses_metadata_and_fallback(monkeypatch: MonkeyPatch) -> None:
    version_module = importlib.import_module("eml_attachment_remover._version")
    project = cast("Callable[[], str]", version_module.__dict__["_project_version"])
    resolved = cast("Callable[[], str]", version_module.__dict__["_resolved_version"])
    assert project() == "3.0.0"
    monkeypatch.setitem(
        version_module.__dict__, "distribution_version", lambda _name: "9.9.9"
    )
    assert resolved() == "9.9.9"

    def absent(_name: str) -> str:
        raise PackageNotFoundError

    monkeypatch.setitem(version_module.__dict__, "distribution_version", absent)
    assert resolved() == project()
