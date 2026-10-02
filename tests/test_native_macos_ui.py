"""Compile and qualify the self-contained native presentation on macOS."""

from __future__ import annotations

import json
import os
import platform
import plistlib
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Final

import pytest
from tools import macos_archive

ROOT: Final = Path(__file__).resolve().parents[1]
UI: Final = ROOT / "integrations" / "macos-ui"
MACOS_ONLY: Final = pytest.mark.skipif(
    sys.platform != "darwin", reason="native macOS UI"
)


@MACOS_ONLY
def test_native_report_model() -> None:
    """Exercise outcome presentation, late failures, and exact native addresses."""
    subprocess.run(["/bin/sh", str(UI / "test.sh")], check=True, timeout=180)


@MACOS_ONLY
@pytest.mark.skipif(
    platform.mac_ver()[0].startswith(("14.", "15.")),
    reason="Icon Composer compilation requires Xcode 26 or later",
)
def test_native_bundle_is_signed_for_host_and_processes_current_sources(
    tmp_path: Path,
) -> None:
    """A fresh single-CPU bundle admits a real processing receipt."""
    app = tmp_path / "EML Attachment Remover.app"
    environment = {**os.environ, "EML_REMOVER_PYTHON": sys.executable}
    subprocess.run(
        ["/bin/sh", str(UI / "build.sh"), str(app)],
        check=True,
        env=environment,
        timeout=240,
    )
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)],
        check=True,
        timeout=30,
    )
    architectures = subprocess.check_output(
        [
            "/usr/bin/xcrun",
            "lipo",
            "-archs",
            str(app / "Contents/MacOS/EMLAttachmentRemover"),
        ],
        text=True,
        timeout=30,
    )
    assert architectures.split() == [platform.machine()]
    _assert_no_development_runtime(app / "Contents/MacOS/EMLAttachmentRemover")
    with (app / "Contents/Info.plist").open("rb") as stream:
        metadata = plistlib.load(stream)
    # Qualify the customer ZIP, then exercise the extracted launcher and installer.
    archive = tmp_path / "customer.zip"
    macos_archive.package(app, archive)
    macos_archive.verify(
        archive,
        app / "Contents/Resources/remove-eml-attachments.pyz",
        metadata["CFBundleShortVersionString"],
        platform.machine(),
    )
    extracted = tmp_path / "download"
    subprocess.run(
        ["/usr/bin/ditto", "-x", "-k", str(archive), str(extracted)],
        check=True,
        timeout=30,
    )
    app = extracted / macos_archive.APP
    resources = app / "Contents/Resources"
    assert metadata["CFBundleIdentifier"] == "io.github.resoltico.emlattachmentremover"
    assert metadata["NSHumanReadableCopyright"] == "Copyright © 2026 Ervins Strauhmanis"
    assert metadata["CFBundleIconFile"] == "EML"
    assert metadata["CFBundleVersion"] == str(
        tomllib.loads((ROOT / "pyproject.toml").read_text())["tool"][
            "eml-attachment-remover"
        ]["macos"]["build-number"]
    )
    assert metadata["CFBundleIconName"] == "EML"
    assert (resources / "EML.icns").read_bytes().startswith(b"icns")
    assert (resources / "Assets.car").stat().st_size > 0
    assert (resources / "LICENSE").read_bytes() == (ROOT / "LICENSE").read_bytes()
    assert (resources / "ARTWORK.md").read_bytes() == (UI / "ARTWORK.md").read_bytes()
    source = tmp_path / "public.eml"
    source.write_bytes(
        b"From: qa@example.test\r\nSubject: Native UI QA\r\n\r\nPublic body.\r\n"
    )
    result = subprocess.run(
        ["/bin/sh", str(resources / "processing-launcher.sh"), str(source)],
        env={
            **environment,
            "EML_REMOVER_ZIPAPP": str(resources / "remove-eml-attachments.pyz"),
        },
        check=True,
        capture_output=True,
        timeout=30,
    )
    envelope = json.loads(result.stdout)
    assert envelope["process_status"] == 0
    assert envelope["report"]["version"] == metadata["CFBundleShortVersionString"]
    assert envelope["report"]["items"][0]["status"] == "created"
    assert (tmp_path / "public.mime-pruned.eml").is_file()
    _exercise_native_launch(app, (source,), environment, tmp_path)
    companion = tmp_path / "companion.eml"
    companion.write_bytes(b"Subject: Public companion\r\n\r\nPublic body.\r\n")
    _exercise_native_launch(app, (source, companion), environment, tmp_path)
    _exercise_installation(
        app, tmp_path, environment, extracted / "integrations/macos-ui"
    )


def _exercise_installation(
    app: Path, tmp_path: Path, environment: dict[str, str], installer: Path
) -> None:
    """Check signed installation, retained prior files, and refusal of unknown apps."""
    home = tmp_path / "private-home"
    home.mkdir(mode=0o700)
    installed = home / "Applications" / "EML Attachment Remover.app"
    install_environment = {
        **environment,
        "HOME": str(home),
        "EML_REMOVER_UI_APP": str(installed),
    }
    subprocess.run(
        ["/bin/sh", str(installer / "install.sh"), str(app)],
        check=True,
        env=install_environment,
        timeout=60,
    )
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(installed)],
        check=True,
        timeout=30,
    )
    # Updating keeps the complete prior bundle, including user additions.
    sentinel = installed / "operator-notes.txt"
    sentinel.write_text("Retain this operator-owned addition.\n", encoding="utf-8")
    subprocess.run(
        ["/bin/sh", str(installer / "install.sh"), str(app)],
        check=True,
        env=install_environment,
        timeout=60,
    )
    backups = list(
        (home / "Library/Application Support/EML Attachment Remover UI Backups").glob(
            "previous-*/*.bundle-backup"
        )
    )
    assert len(backups) == 1
    runtime = (
        home / "Library/Application Support/EML Attachment Remover UI/runtime.json"
    )
    assert json.loads(runtime.read_text())["python"] == str(
        Path(sys.executable).resolve()
    )
    assert runtime.stat().st_mode & 0o077 == 0
    assert (backups[0] / "operator-notes.txt").read_text() == (
        "Retain this operator-owned addition.\n"
    )
    _exercise_runtime_rollback(app, installed, install_environment, installer, home)
    unknown = home / "unknown.app"
    unknown.mkdir()
    unknown_sentinel = unknown / "keep.txt"
    unknown_sentinel.write_text("Untouched.\n", encoding="utf-8")
    refused = subprocess.run(
        ["/bin/sh", str(installer / "install.sh"), str(app)],
        check=False,
        capture_output=True,
        env={**install_environment, "EML_REMOVER_UI_APP": str(unknown)},
        timeout=60,
    )
    assert refused.returncode != 0
    assert unknown_sentinel.read_text() == "Untouched.\n"


@pytest.mark.skipif(os.name == "nt", reason="POSIX integration static analysis")
def test_native_integration_shell_scripts_pass_static_analysis() -> None:
    """POSIX quality lanes check the native installation/build shell boundary."""
    shellcheck = shutil.which("shellcheck")
    assert shellcheck is not None, "Install the locked development dependencies."
    subprocess.run(
        [shellcheck, "--shell=sh", *(str(path) for path in sorted(UI.glob("*.sh")))],
        check=True,
        timeout=30,
    )


def test_custom_artwork_has_no_system_symbol_dependencies() -> None:
    """Branding and UI artwork cannot regress to vendor-provided symbol images."""
    for source in UI.rglob("*.swift"):
        text = source.read_text(encoding="utf-8")
        for forbidden in (
            "systemSymbolName:",
            "SymbolConfiguration(",
            "withSymbolConfiguration(",
            "CoreGlyphs",
        ):
            assert forbidden not in text, (source.name, forbidden)
    provenance = (UI / "ARTWORK.md").read_text(encoding="utf-8")
    assert "MIT license" in provenance
    assert "original geometric artwork" in provenance


def _exercise_runtime_rollback(
    app: Path, installed: Path, environment: dict[str, str], installer: Path, root: Path
) -> None:
    wrapper = root / "fault-python"
    bootstrap = root / "fault-runtime.py"
    bootstrap.write_text(
        "import os, sys\n"
        "sys.argv = sys.argv[1:]\n"
        "original = os.replace\n"
        "def replace(source, target):\n"
        "    if str(target).endswith('/runtime.json'):\n"
        "        raise OSError('Injected runtime publication failure')\n"
        "    return original(source, target)\n"
        "os.replace = replace\n"
        "exec(compile(sys.stdin.read(), '<installer>', 'exec'))\n",
        encoding="utf-8",
    )
    wrapper.write_text(
        '#!/bin/sh\nif [ "$1" = "-" ]; then\n exec "'
        + sys.executable
        + '" -B "'
        + str(bootstrap)
        + '" "$@"\nfi\nexec "'
        + sys.executable
        + '" -B "$@"\n',
        encoding="utf-8",
    )
    wrapper.chmod(0o700)
    runtime = (
        root / "Library/Application Support/EML Attachment Remover UI/runtime.json"
    )
    before = runtime.read_bytes()
    bundle_before = {
        str(file.relative_to(installed)): file.read_bytes()
        for file in installed.rglob("*")
        if file.is_file()
    }
    result = subprocess.run(
        ["/bin/sh", str(installer / "install.sh"), str(app)],
        env={**environment, "EML_REMOVER_PYTHON": str(wrapper)},
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    assert result.returncode == 4
    assert "Injected runtime publication failure" in result.stderr
    assert runtime.read_bytes() == before
    assert {
        str(file.relative_to(installed)): file.read_bytes()
        for file in installed.rglob("*")
        if file.is_file()
    } == bundle_before
    assert not list(runtime.parent.glob(".runtime-*"))
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(installed)],
        check=True,
        timeout=30,
    )


def _exercise_native_launch(
    app: Path,
    sources: tuple[Path, ...],
    environment: dict[str, str],
    root: Path,
    *,
    intel_slice: bool = False,
) -> None:
    log = root / "native-launch.txt"
    command = ["/usr/bin/arch", "-x86_64"] if intel_slice else []
    command.extend((
        str(app / "Contents/MacOS/EMLAttachmentRemover"),
        *map(str, sources),
    ))
    with (
        log.open("w") as errors,
        subprocess.Popen(
            command,
            env={**environment, "EML_REMOVER_UI_TRACE": "1"},
            stdout=subprocess.DEVNULL,
            stderr=errors,
        ) as process,
    ):
        try:
            deadline = time.monotonic() + 30
            while (
                process.poll() is None
                and time.monotonic() < deadline
                and "admitted status=0" not in log.read_text()
            ):
                time.sleep(0.05)
            assert "admitted status=0" in log.read_text(), (
                "Packaged native executable did not admit its processing receipt"
            )
            assert log.read_text().count("open-batch count=") == 1, (
                "One native launch must create exactly one batch"
            )
            assert f"open-batch count={len(sources)}" in log.read_text()
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=15)


@MACOS_ONLY
@pytest.mark.skipif(
    not os.environ.get("EML_DELIVERY_DIRECTORY"),
    reason="downloaded CI candidate required",
)
def test_downloaded_candidate_executes_on_this_os_and_cpu(tmp_path: Path) -> None:
    delivery = Path(os.environ["EML_DELIVERY_DIRECTORY"])
    archive = next(delivery.glob(f"*-macos-{platform.machine()}.zip"))
    extracted = tmp_path / "downloaded"
    subprocess.run(
        ["/usr/bin/ditto", "-x", "-k", str(archive), str(extracted)],
        check=True,
        timeout=30,
    )
    source = tmp_path / "candidate.eml"
    source.write_bytes(b"Subject: Public compatibility QA\r\n\r\nPublic body.\r\n")
    _exercise_native_launch(
        extracted / macos_archive.APP,
        (source,),
        {**os.environ, "EML_REMOVER_PYTHON": sys.executable},
        tmp_path,
    )
    assert (tmp_path / "candidate.mime-pruned.eml").is_file()
    if platform.machine() == "arm64" and platform.mac_ver()[0].startswith("14."):
        translated_machine = subprocess.check_output(
            ["/usr/bin/arch", "-x86_64", "/usr/bin/uname", "-m"],
            text=True,
            timeout=15,
        ).strip()
        assert translated_machine == "x86_64"
        intel_source = tmp_path / "candidate-intel.eml"
        intel_source.write_bytes(b"Subject: Intel slice QA\r\n\r\nPublic body.\r\n")
        intel_archive = next(delivery.glob("*-macos-x86_64.zip"))
        intel_extracted = tmp_path / "intel-downloaded"
        subprocess.run(
            ["/usr/bin/ditto", "-x", "-k", str(intel_archive), str(intel_extracted)],
            check=True,
            timeout=30,
        )
        _exercise_native_launch(
            intel_extracted / macos_archive.APP,
            (intel_source,),
            {**os.environ, "EML_REMOVER_PYTHON": sys.executable},
            tmp_path,
            intel_slice=True,
        )
        assert (tmp_path / "candidate-intel.mime-pruned.eml").is_file()


def _assert_no_development_runtime(executable: Path) -> None:
    """Reject sanitizer and fuzzer runtimes in the customer executable."""
    symbols = subprocess.check_output(
        ["/usr/bin/nm", str(executable)], text=True, timeout=30
    )
    dependencies = subprocess.check_output(
        ["/usr/bin/otool", "-L", str(executable)], text=True, timeout=30
    )
    for development_runtime in (
        "LLVMFuzzer",
        "__asan_",
        "__sanitizer_cov_",
        "clang_rt",
    ):
        assert development_runtime not in symbols + dependencies
