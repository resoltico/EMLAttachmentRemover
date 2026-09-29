"""Plan and run every CI gate that the current host can reproduce."""

from __future__ import annotations

import os
import shlex
import sys
import tempfile
import time
import tomllib
from contextlib import nullcontext
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping, Sequence
    from contextlib import AbstractContextManager

    type StepRunner = Callable[[Sequence[str], Mapping[str, str], float], None]

WORKFLOW_PYTHONS: Final = ("3.14.7", "3.14.7t")
# CI type-checks natively on each runner OS; mypy can check every target from here.
TYPE_PLATFORMS: Final = ("darwin", "linux", "win32")
CANONICAL_PYTHON: Final = "3.14.7"
MATRIX_PYTHON: Final = "${{ matrix.python }}"
# Each step's leader is itself a process-group owner; it needs time to stop its
# own group (task_process.DEFAULT_GRACE_SECONDS) before this owner escalates.
OWNER_GRACE_SECONDS: Final = 30.0
SHORT_TIMEOUT_SECONDS: Final = 600
LANE_TIMEOUT_SECONDS: Final = 1_800
THOROUGH_TIMEOUT_SECONDS: Final = 3_600
MUTATION_TIMEOUT_SECONDS: Final = 10_800
TEMPORARY_PREFIX: Final = "eml-attachment-remover-ci-"
# Normalized workflow ``run:`` texts; each local command is its mirror's words with
# CI-only values (matrix interpreter, tag, artifact directory, workers) substituted.
SYNC: Final = f"uv sync --locked --group dev --python {MATRIX_PYTHON}"
SHELL_CHECK: Final = "test -x /bin/sh"
QUALITY: Final = "uv run python tools/tasks.py quality"
QUALITY_NATIVE: Final = "uv run python tools/tasks.py quality --native"
CANONICAL_SYNC: Final = f"uv sync --locked --group dev --python {CANONICAL_PYTHON}"
TAG_CHECK: Final = 'uv run python tools/check_release_tag.py "$RELEASE_TAG"'
BUILD: Final = "uv run python tools/qualify_release.py --output-directory release-dist"
VERIFY: Final = (
    f"uv run --no-project --python {CANONICAL_PYTHON} python "
    "tools/qualify_release.py --verify-directory release-dist"
)
MUTATION: Final = 'uv run python tools/tasks.py mutation --workers "$MUTATION_WORKERS"'
THOROUGH: Final = (
    "uv run python tools/tasks.py thorough --observable --timeout-seconds 3300"
)
TYPE_CHECK: Final = ("uv", "run", "mypy", "--no-incremental")
FINALIZE: Final = "uv run python tools/finalize_hypothesis_artifacts.py --observations"
# Mutant IDs depend on which lines each OS covers, and the reviewed equivalents are
# Linux IDs; other POSIX hosts therefore run mutation in CI's runner image. It uses
# Docker's native architecture: the code has no architecture-specific branches, and
# emulated x86_64 campaigns were both slower and killed mid-run.
LINUX_IMAGE: Final = "eml-attachment-remover-ci:uv-0.12.5"
# Like GitHub's runner, the campaign runs unprivileged: as root, a defect could
# signal processes that CI's runner user never can.
LINUX_IMAGE_DEFINITION: Final = (
    "FROM ghcr.io/astral-sh/uv:0.12.5 AS uv\n"
    "FROM ubuntu:24.04\n"
    "RUN apt-get update && apt-get install -y --no-install-recommends "
    "ca-certificates && rm -rf /var/lib/apt/lists/* "
    "&& useradd --create-home --home-dir /ci --uid 1001 runner "
    "&& mkdir -p /ci/.cache/uv /ci/work "
    "&& chown -R runner:runner /ci\n"
    "COPY --from=uv /uv /usr/local/bin/uv\n"
    "USER runner\n"
    "WORKDIR /ci\n"
)
LINUX_HOME: Final = "/ci"
CACHE_VOLUME: Final = "eml-attachment-remover-ci-uv-cache"
LINUX_EVIDENCE: Final = Path("build") / "linux-mutation"
# Copy the read-only checkout (no host environments or caches), install it as CI
# does, then exec so the task receives the container's interruption directly.
LINUX_SCRIPT: Final = (
    "cd /src && tar --exclude=./.venv --exclude=./mutants --exclude=./build "
    f"--exclude=./.hypothesis -cf - . | tar -xf - -C {LINUX_HOME}/work "
    f"&& cd {LINUX_HOME}/work "
    "&& PYTHONWARNINGS=default uv sync --locked --group dev "
    f"--python {CANONICAL_PYTHON} "
    '&& exec uv run python tools/tasks.py mutation --workers "$MUTATION_WORKERS"'
)
CI_ONLY_NOTICE: Final = (
    "left to CI: Windows lanes, Linux-only behavior, artifact attestation, "
    "and GitHub release publication"
)


@dataclass(frozen=True, slots=True)
class Step:
    """One local command and the normalized workflow ``run:`` text it mirrors."""

    mirrors: str
    command: tuple[str, ...]
    environment: Mapping[str, str]
    timeout_seconds: float
    leased: bool = False


def project_tag(project_root: Path) -> str:
    """Return the release tag the current project version would be published as.

    Returns:
        The project version prefixed with ``v``.

    """
    with (project_root / "pyproject.toml").open("rb") as project_file:
        return f"v{tomllib.load(project_file)['project']['version']}"


def _environment(root: Path, python: str) -> dict[str, str]:
    """Select one interpreter as setup-uv does in CI, in a private environment.

    Returns:
        uv's project-environment and interpreter selection.

    """
    project_environment = str(root / f"venv-{python}")
    return {
        "UV_PROJECT_ENVIRONMENT": project_environment,
        "UV_PYTHON": python,
        # Replaces the invoking environment, which uv would otherwise warn about.
        "VIRTUAL_ENV": project_environment,
    }


def _lane(
    root: Path,
    python: str,
    values: Mapping[str, str],
    *entries: tuple[str, float],
) -> list[Step]:
    """Mirror workflow steps for one interpreter.

    Returns:
        Steps whose commands are their mirrors' words with local values.

    """
    environment = _environment(root, python)
    steps: list[Step] = []
    for mirrors, timeout in entries:
        words = mirrors.replace(MATRIX_PYTHON, python).split()
        command = tuple(values.get(word, word) for word in words)
        # CI relaxes warnings only for dependency installation.
        installer = {"PYTHONWARNINGS": "default"} if command[1] == "sync" else {}
        steps.append(Step(mirrors, command, environment | installer, timeout))
    return steps


def _linux_mutation(project_root: Path, root: Path, workers: str) -> list[Step]:
    """Mirror CI's mutation job in its runner image, from a non-Linux host.

    Returns:
        The image build and the containerized campaign, whose evidence lands in
        ``build/linux-mutation``.

    """
    build = (
        *("docker", "build", "--quiet"),
        *("--tag", LINUX_IMAGE, str(root / "image")),
    )
    mounts = (
        f"type=volume,source={CACHE_VOLUME},target={LINUX_HOME}/.cache/uv",
        f"type=bind,source={project_root},target=/src,readonly",
        (
            f"type=bind,source={project_root / LINUX_EVIDENCE},"
            f"target={LINUX_HOME}/work/build"
        ),
    )
    environment = (
        "PYTHONDEVMODE=1",
        "PYTHONDONTWRITEBYTECODE=1",
        "PYTHONNOUSERSITE=1",
        "PYTHONWARNINGS=error",
        f"MUTATION_WORKERS={workers}",
        f"UV_PROJECT_ENVIRONMENT={LINUX_HOME}/venv",
        f"UV_PYTHON={CANONICAL_PYTHON}",
    )
    run = (
        *("docker", "run", "--rm", "--init"),
        *(option for mount in mounts for option in ("--mount", mount)),
        *(option for value in environment for option in ("--env", value)),
        *(LINUX_IMAGE, "/bin/sh", "-c", LINUX_SCRIPT),
    )
    return [
        Step(MUTATION, build, {}, SHORT_TIMEOUT_SECONDS),
        # The evidence directory is shared with any other campaign in this checkout.
        Step(MUTATION, run, {}, MUTATION_TIMEOUT_SECONDS, leased=True),
    ]


def plan(
    root: Path,
    *,
    host: str,
    project_root: Path,
    release_tag: str,
    workers: str,
) -> tuple[Step, ...]:
    """Return every reproducible CI step, cheapest failures first.

    Returns:
        Both quality lanes, type checks for every CI runner OS, the release build
        and verification, mutation (Linux natively, other POSIX hosts in CI's
        runner image, never Windows, as in CI), and both exploration lanes.

    """
    posix = host != "win32"
    values = {
        '"$RELEASE_TAG"': release_tag,
        "release-dist": str(root / "release-dist"),
        '"$MUTATION_WORKERS"': workers,
    }
    shell = ((SHELL_CHECK, SHORT_TIMEOUT_SECONDS),) if posix else ()
    steps: list[Step] = []
    for python in WORKFLOW_PYTHONS:
        # CI runs the portable static checks once, on the canonical interpreter's lane.
        quality = QUALITY if python == CANONICAL_PYTHON else QUALITY_NATIVE
        steps += _lane(
            root,
            python,
            values,
            (SYNC, SHORT_TIMEOUT_SECONDS),
            *shell,
            (quality, LANE_TIMEOUT_SECONDS),
        )
    steps += [
        Step(
            QUALITY,
            (*TYPE_CHECK, "--cache-dir", os.devnull, "--platform", platform),
            _environment(root, CANONICAL_PYTHON),
            SHORT_TIMEOUT_SECONDS,
        )
        for platform in TYPE_PLATFORMS
    ]
    steps += _lane(
        root,
        CANONICAL_PYTHON,
        values,
        (CANONICAL_SYNC, SHORT_TIMEOUT_SECONDS),
        (TAG_CHECK, SHORT_TIMEOUT_SECONDS),
        (BUILD, LANE_TIMEOUT_SECONDS),
        (VERIFY, LANE_TIMEOUT_SECONDS),
    )
    if host == "linux":
        mutation = (MUTATION, MUTATION_TIMEOUT_SECONDS)
        steps += _lane(root, CANONICAL_PYTHON, values, mutation)
    elif posix:
        steps += _linux_mutation(project_root, root, workers)
    for python in WORKFLOW_PYTHONS:
        steps += _lane(
            root,
            python,
            values,
            (THOROUGH, THOROUGH_TIMEOUT_SECONDS),
            (FINALIZE, SHORT_TIMEOUT_SECONDS),
        )
    return tuple(steps)


def run(
    steps: tuple[Step, ...],
    run_step: StepRunner,
    clock: Callable[[], float] = time.monotonic,
    lease: Callable[[], AbstractContextManager[None]] = nullcontext,
) -> None:
    """Run each step in order, stopping at the first failure."""
    for number, step in enumerate(steps, start=1):
        print(f"==> [{number}/{len(steps)}] {shlex.join(step.command)}", flush=True)
        started = clock()
        with lease() if step.leased else nullcontext():
            run_step(step.command, step.environment, step.timeout_seconds)
        print(f"<== {clock() - started:.1f}s", flush=True)
    print(CI_ONLY_NOTICE, flush=True)


def run_local_ci(
    project_root: Path,
    run_step: StepRunner,
    *,
    release_tag: str | None,
    workers: str,
    lease: Callable[[], AbstractContextManager[None]] = nullcontext,
) -> None:
    """Run the full local plan with private, always-removed environments."""
    tag = project_tag(project_root) if release_tag is None else release_tag
    # Bind-mount sources must exist before the container starts.
    (project_root / LINUX_EVIDENCE).mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=TEMPORARY_PREFIX) as directory:
        # CI's runner paths contain no symlinks; macOS temp directories do
        # (/var -> /private/var), which tools comparing resolved paths reject.
        root = Path(directory).resolve()
        (root / "image").mkdir()
        (root / "image" / "Dockerfile").write_bytes(LINUX_IMAGE_DEFINITION.encode())
        steps = plan(
            root,
            host=sys.platform,
            project_root=project_root,
            release_tag=tag,
            workers=workers,
        )
        run(steps, run_step, lease=lease)
