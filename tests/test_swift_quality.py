"""Real Swift tool failure controls enforce the declared source contracts."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Final

import pytest

ROOT: Final = Path(__file__).resolve().parents[1]
UI: Final = ROOT / "integrations/macos-ui"
PINS: Final = tomllib.loads((UI / "toolchain.toml").read_text())
TOOLS: Final = Path(
    os.environ.get(
        "EML_SWIFT_TOOLS",
        str(
            Path.home()
            / "Library/Application Support/EML Attachment Remover Toolchains"
        ),
    )
)
MACOS: Final = pytest.mark.skipif(
    sys.platform != "darwin", reason="macOS Swift toolchain"
)


def _binary(tool: str) -> str:
    path = TOOLS / (tool + "-" + PINS[tool]["version"]) / tool
    assert path.is_file(), "Install the pinned Swift quality tools."
    return str(path)


@MACOS
@pytest.mark.parametrize("source", ["let value=1\n", "let =\n"])
def test_formatter_strictly_rejects_style_and_parse_failures(
    tmp_path: Path, source: str
) -> None:
    path = tmp_path / "Invalid.swift"
    path.write_text(source)
    result = subprocess.run(
        [
            _binary("swift-format"),
            "lint",
            "--strict",
            "--configuration",
            str(UI / "swift-format.json"),
            str(path),
        ],
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    assert result.returncode != 0
    assert result.stderr


@MACOS
@pytest.mark.parametrize(
    ("rule", "source"),
    [
        ("force_try", "func task() throws {}\nfunc work() { try! task() }\n"),
        ("function_body_length", "func work() {\n" + "print(1)\n" * 61 + "}\n"),
        (
            "type_body_length",
            "struct Large {\n"
            + "\n".join(f"let value{index} = {index}" for index in range(301))
            + "\n}\n",
        ),
        ("file_length", "// code budget control\n" * 451),
        (
            "cyclomatic_complexity",
            "func work(_ value: Int) {\n"
            + "if value > 0 { print(value) }\n" * 10
            + "}\n",
        ),
        (
            "nesting",
            "struct First { struct Second { struct Third { struct Fourth {} } } }\n",
        ),
    ],
)
def test_swiftlint_rejects_unsafe_and_oversized_code(
    tmp_path: Path, rule: str, source: str
) -> None:
    path = tmp_path / "Invalid.swift"
    path.write_text(source)
    environment = dict(os.environ)
    environment["TOOLCHAIN_DIR"] = subprocess.check_output(
        ["/bin/sh", str(UI / "swiftc.sh"), "--toolchain-directory"], text=True
    ).strip()
    result = subprocess.run(
        [
            _binary("swiftlint"),
            "lint",
            "--strict",
            "--no-cache",
            "--config",
            str(UI / "swiftlint.yml"),
            "--reporter",
            "json",
            str(path),
        ],
        env=environment,
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    assert result.returncode != 0
    assert rule in {item["rule_id"] for item in json.loads(result.stdout)}
