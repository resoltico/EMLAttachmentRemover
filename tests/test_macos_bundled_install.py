"""Qualify relocation and external-configuration isolation for bundled apps."""

from __future__ import annotations

import hashlib
import os
import platform
import plistlib
import subprocess
import sys
from pathlib import Path

import pytest
from tools import macos_archive

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "integrations/macos-ui"


@pytest.mark.skipif(sys.platform != "darwin", reason="native macOS installation")
@pytest.mark.skipif(
    platform.mac_ver()[0].startswith(("14.", "15.")),
    reason="Icon Composer compilation requires Xcode 26 or later",
)
def test_bundled_install_relocates_without_changing_external_selection(
    tmp_path: Path,
) -> None:
    """A relocated bundled runtime works without consulting another interpreter."""
    app = tmp_path / "source/EML Attachment Remover.app"
    subprocess.run(
        ["/bin/sh", str(UI / "build.sh"), str(app), platform.machine(), "bundled"],
        env={**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        check=True,
        timeout=240,
    )
    archive = tmp_path / "customer.zip"
    macos_archive.package(app, archive)
    macos_archive.verify(
        archive,
        app / "Contents/Resources/remove-eml-attachments.pyz",
        plistlib.loads((app / "Contents/Info.plist").read_bytes())[
            "CFBundleShortVersionString"
        ],
        platform.machine(),
        "bundled",
    )
    extracted = tmp_path / "download"
    subprocess.run(
        ["/usr/bin/ditto", "-x", "-k", str(archive), str(extracted)],
        check=True,
        timeout=30,
    )
    app = extracted / macos_archive.APP
    home = tmp_path / "private-home"
    configuration = home / "Library/Application Support/EML Attachment Remover UI"
    configuration.mkdir(mode=0o700, parents=True)
    runtime_selection = configuration / "runtime.json"
    sentinel = b'{"python":"/existing/managed-python"}\n'
    runtime_selection.write_bytes(sentinel)
    installed = home / "Applications/EML Attachment Remover.app"
    upstream_bytecode = {
        path.relative_to(app): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in app.rglob("*.pyc")
    }
    environment = {
        **os.environ,
        "HOME": str(home),
        "EML_REMOVER_UI_APP": str(installed),
        "EML_REMOVER_PYTHON": "/nonexistent/external-python",
    }
    subprocess.run(
        ["/bin/sh", str(UI / "install.sh"), str(app)],
        env=environment,
        check=True,
        timeout=60,
    )
    assert runtime_selection.read_bytes() == sentinel
    runtime = installed / "Contents/Resources/Runtime"
    prefix = subprocess.check_output(
        [
            str(runtime / "bin/python3.14"),
            "-I",
            "-B",
            "-c",
            "import sys, ssl, sqlite3, email, ctypes; print(sys.prefix)",
        ],
        env=environment,
        text=True,
        timeout=30,
    ).strip()
    assert Path(prefix).resolve() == runtime.resolve()
    assert {
        path.relative_to(installed): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in installed.rglob("*.pyc")
    } == upstream_bytecode
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(installed)],
        check=True,
        timeout=30,
    )
