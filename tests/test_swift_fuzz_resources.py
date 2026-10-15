"""Reject excessive fuzz diagnostics and seeds before they waste storage."""

from __future__ import annotations

import json
from typing import TYPE_CHECKING

from tests.test_swift_fuzzing import MACOS, _run, copied_ui

if TYPE_CHECKING:
    from pathlib import Path

__all__ = ["copied_ui"]


@MACOS
def test_compile_log_overflow_is_bounded_and_fails_the_stage(
    copied_ui: Path, tmp_path: Path
) -> None:
    configuration = copied_ui / "fuzzing.toml"
    configuration.write_text(
        configuration.read_text().replace("log_bytes = 8388608", "log_bytes = 16")
    )
    (copied_ui / "swiftc.sh").write_text(
        '#!/bin/sh\ncase "$1" in\n--version) echo fixture;;\n'
        "--toolchain-directory) echo /fixture;;\n"
        "*) printf 'diagnostic-that-exceeds-sixteen-bytes';;\nesac\n"
    )
    output = tmp_path / "campaign"
    result = _run(copied_ui, output)
    assert result.returncode != 0
    assert (output / "compile.log").read_bytes() == b"diagnostic-that-"
    metadata = json.loads((output / "invocation.json").read_text())
    assert (metadata["stage"], metadata["outcome"]) == ("compile", "failed")
    assert metadata["limits"]["log_bytes_per_stage"] == 16
    assert metadata["limits"]["core_bytes"] == 0
    assert not (output / "replay.log").exists()


@MACOS
def test_seed_above_the_configured_input_limit_is_rejected(
    copied_ui: Path, tmp_path: Path
) -> None:
    (copied_ui / "Fuzz/corpus/oversize").write_bytes(b"x" * 65537)
    output = tmp_path / "campaign"
    result = _run(copied_ui, output, "-runs=1")
    assert result.returncode != 0
    metadata = json.loads((output / "invocation.json").read_text())
    assert metadata["stage"] == "snapshot"
    assert "exceeds configured max_length" in metadata["error"]
    assert not (output / "compile.log").exists()
