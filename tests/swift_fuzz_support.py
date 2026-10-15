"""Capture native fuzz runner diagnostics while preserving owned-process cleanup."""

from __future__ import annotations

import subprocess
import tempfile
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING

from tools import task_process

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence


def run_campaign(
    ui: Path,
    output: Path,
    options: Sequence[str],
    *,
    environment: Mapping[str, str],
    timeout_seconds: float | None = None,
) -> subprocess.CompletedProcess[str]:
    """Capture one runner with enough time to enforce its own stage deadlines.

    Returns:
        The runner status and captured diagnostics, including ordinary failures.

    """
    configuration = tomllib.loads((ui / "fuzzing.toml").read_text())["campaign"]
    duration = configuration["total_seconds"]
    for option in options:
        if option.startswith("-max_total_time="):
            duration = int(option.partition("=")[2])
    if timeout_seconds is None:
        # Identity preflights and startup have a separate aggregate allowance.
        # Stage deadlines stay authoritative; this outer deadline is a backstop.
        setup_seconds = 180
        timeout_seconds = (
            setup_seconds
            + configuration["compile_seconds"]
            + configuration["replay_seconds"]
            + duration
            + configuration["shutdown_seconds"]
            + 4 * task_process.DEFAULT_GRACE_SECONDS
        )
    command = ["/bin/sh", str(ui / "fuzz.sh"), str(output), *options]
    with tempfile.TemporaryDirectory(prefix="eml-fuzz-test-capture-") as temporary:
        captured = Path(temporary)
        stdout = captured / "stdout"
        stderr = captured / "stderr"
        process_environment = {
            **environment,
            "EML_FUZZ_TEST_STDOUT": str(stdout),
            "EML_FUZZ_TEST_STDERR": str(stderr),
        }
        returncode = 0
        try:
            task_process.run(
                [
                    "/bin/sh",
                    "-c",
                    'exec "$@" >"$EML_FUZZ_TEST_STDOUT" 2>"$EML_FUZZ_TEST_STDERR"',
                    "fuzz-test",
                    *command,
                ],
                cwd=ui,
                env=process_environment,
                timeout=timeout_seconds,
                # The runner needs its own grace interval to sweep its stage group.
                grace_seconds=2 * task_process.DEFAULT_GRACE_SECONDS,
            )
        except subprocess.CalledProcessError as error:
            returncode = error.returncode
        return subprocess.CompletedProcess(
            command,
            returncode,
            stdout.read_text(encoding="utf-8", errors="replace"),
            stderr.read_text(encoding="utf-8", errors="replace"),
        )
