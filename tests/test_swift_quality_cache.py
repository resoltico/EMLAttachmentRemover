"""Native binary caching preserves platform isolation and installer validation."""

from __future__ import annotations

import importlib
import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
ACTION = ROOT / ".github/actions/swift-quality-cache/action.yml"


def test_cache_is_isolated_and_bound_to_its_build_inputs() -> None:
    action = importlib.import_module("yaml").safe_load(ACTION.read_text())
    assert action["runs"]["using"] == "composite"
    steps = action["runs"]["steps"]
    cache = steps[1]
    runtime_action = (ROOT / ".github/actions/runtime-source/action.yml").read_text()
    assert cache["uses"].split("@")[1] in runtime_action
    options = cache["with"]
    assert options["path"] == "${{ runner.temp }}/eml-swift-quality-tools"
    assert "restore-keys" not in options
    assert "${{ steps.platform.outputs.identity }}" in options["key"]
    assert set(re.findall(r"'([^']+)'", options["key"])) == {
        "integrations/macos-ui/toolchain.toml",
        "integrations/macos-ui/install-quality-tools.sh",
        "integrations/macos-ui/install-swift-toolchain.sh",
        "integrations/macos-ui/swiftc.sh",
        "tools/pinned_download.py",
        ".github/actions/swift-quality-cache/action.yml",
    }
    assert all("run" not in step for step in steps[1:])


@pytest.mark.skipif(os.name == "nt", reason="macOS composite uses POSIX shell")
@pytest.mark.parametrize("failed_probe", ["none", "sdk", "os-build"])
@pytest.mark.parametrize(
    "identity",
    [
        ("27.0", "26A425", "arm64", "27.0", "26A425"),
        ("27.1", "26A425", "arm64", "27.0", "26A425"),
        ("27.0", "26B501", "arm64", "27.0", "26A425"),
        ("27.0", "26A425", "x86_64", "27.0", "26A425"),
        ("27.0", "26A425", "arm64", "27.1", "26A425"),
        ("27.0", "26A425", "arm64", "27.0", "26B501"),
    ],
)
def test_real_shell_keys_actual_os_cpu_and_sdk_and_exports_private_path(
    tmp_path: Path, identity: tuple[str, str, str, str, str], failed_probe: str
) -> None:
    binaries = tmp_path / "commands"
    binaries.mkdir()
    commands = {
        "sw_vers": (
            'if [ "$1" = -buildVersion ]; then '
            '[ "$TEST_FAIL_PROBE" != os-build ] || exit 7; '
            'printf "%s\\n" "$TEST_OS_BUILD"; '
            'else printf "%s\\n" "$TEST_OS"; fi'
        ),
        "uname": 'printf "%s\\n" "$TEST_CPU"',
        "xcrun": (
            'if [ "$TEST_FAIL_PROBE" = sdk ]; then exit 7; fi; '
            'if [ "$1" = --show-sdk-version ]; then printf "%s\\n" "$TEST_SDK"; '
            'else printf "%s\\n" "$TEST_SDK_BUILD"; fi'
        ),
    }
    for name, body in commands.items():
        command = binaries / name
        command.write_text("#!/bin/sh\n" + body + "\n")
        command.chmod(0o755)
    output = tmp_path / "output"
    environment_file = tmp_path / "environment"
    private_root = tmp_path / "runner temporary"
    environment = {
        **os.environ,
        "PATH": str(binaries) + os.pathsep + os.environ["PATH"],
        "GITHUB_OUTPUT": str(output),
        "GITHUB_ENV": str(environment_file),
        "RUNNER_TEMP": str(private_root),
        "TEST_FAIL_PROBE": failed_probe,
        **dict(
            zip(
                ("TEST_OS", "TEST_OS_BUILD", "TEST_CPU", "TEST_SDK", "TEST_SDK_BUILD"),
                identity,
                strict=True,
            )
        ),
    }
    action = importlib.import_module("yaml").safe_load(ACTION.read_text())
    script = action["runs"]["steps"][0]["run"]
    result = subprocess.run(
        ["/bin/bash", "-e", "-o", "pipefail", "-c", script],
        env=environment,
        check=False,
    )
    if failed_probe != "none":
        assert result.returncode == 7
        assert not output.exists()
        assert not environment_file.exists()
        return
    assert result.returncode == 0
    assert output.read_text() == "identity=" + "-".join(identity) + "\n"
    assert environment_file.read_text() == (
        f"EML_SWIFT_TOOLS={private_root / 'eml-swift-quality-tools'}\n"
    )


def test_every_cache_usage_precedes_the_unchanged_installer() -> None:
    yaml = importlib.import_module("yaml")
    jobs = [
        job
        for path in (ROOT / ".github/workflows").glob("*.yml")
        for job in yaml.safe_load(path.read_text())["jobs"].values()
    ]
    installations = [
        (job["steps"], index)
        for job in jobs
        for index, step in enumerate(job.get("steps", []))
        if "install-quality-tools.sh" in step.get("run", "")
    ]
    assert installations
    for steps, index in installations:
        assert index > 0
        assert steps[index - 1]["uses"] == "./.github/actions/swift-quality-cache"
        assert steps[index - 1].get("if") == steps[index].get("if")
    assert sum(
        step.get("uses") == "./.github/actions/swift-quality-cache"
        for job in jobs
        for step in job.get("steps", [])
    ) == len(installations)
