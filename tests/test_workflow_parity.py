"""Keep the workflows and the local CI plan in lockstep."""

from __future__ import annotations

import re
import tomllib
import unittest
from pathlib import Path
from typing import TYPE_CHECKING, Final

from tools import local_ci

if TYPE_CHECKING:
    from collections.abc import Iterator, Sequence

PROJECT_ROOT: Final = Path(__file__).resolve().parents[1]
WORKFLOWS: Final = PROJECT_ROOT / ".github" / "workflows"
RUN_KEY: Final = re.compile(r"( *(?:- )?)run: (.*)")
# Workflow steps with no local counterpart, and why.
CI_ONLY: Final = {
    # Authoritative tag eligibility requires the fetched release checkout; local
    # candidates remain version-only. Real Git regression tests cover the mechanism.
    (
        "uv run --no-project python tools/check_release_tag.py "
        '"$RELEASE_TAG" --require-main'
    ),
    # Hosted Xcode paths are image-specific; local SDKs are audited separately.
    'uv run /bin/sh integrations/macos-ui/ci-sdk.sh >> "$GITHUB_ENV"',
    # macOS 14 defaults to an older SDK; select its installed platform tools.
    "sudo xcode-select --switch /Applications/Xcode_16.2.app/Contents/Developer",
    # Extracts the checksum-manifest digest for GitHub artifact attestation.
    (
        "manifest_digest=$(shasum -a 256 release-dist/SHA256SUMS) "
        "manifest_digest=${manifest_digest%% *} "
        'if [[ ! "$manifest_digest" =~ ^[0-9a-f]{64}$ ]]; then exit 1 fi '
        'printf \'digest=sha256:%s\\n\' "$manifest_digest" >> "$GITHUB_OUTPUT"'
    ),
    # Publishes the GitHub release; needs the workflow's GH_TOKEN.
    (
        "uv run --no-project --python 3.14.8 python -B -m tools.publish_release "
        "--assets-directory release-dist"
    ),
}


def _block(lines: Sequence[str], indent: int) -> str:
    """Return a block scalar's lines, up to the first line not indented past it.

    Returns:
        The block's lines joined by spaces.

    """
    block: list[str] = []
    for line in lines:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        block.append(line)
    return " ".join(block)


def _run_steps(text: str) -> Iterator[str]:
    """Yield each ``run:`` value, with folded and literal blocks, normalized.

    Yields:
        The step's command text with all whitespace runs collapsed to one space.

    """
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = RUN_KEY.fullmatch(line)
        if match is None:
            continue
        value = match[2]
        if value in {">-", "|"}:
            value = _block(lines[index + 1 :], len(match[1]))
        yield " ".join(value.split())


def _workflow_steps() -> set[str]:
    return {
        step
        for path in sorted(WORKFLOWS.glob("*.yml"))
        for step in _run_steps(path.read_text(encoding="utf-8"))
    }


class WorkflowParityTests(unittest.TestCase):
    """Fail when a workflow gains, loses, or changes a step without a mirror."""

    def test_every_workflow_step_is_mirrored_locally_or_listed_as_ci_only(
        self,
    ) -> None:
        steps = [
            step
            for host in ("linux", "darwin")
            for step in local_ci.plan(
                Path("/host-ci"),
                host=host,
                project_root=Path("/project"),
                release_tag="v1.2.3",
                workers="auto",
            )
        ]
        mirrored = {step.mirrors for step in steps if step.mirrors is not None}
        candidates = [step for step in steps if step.mirrors is None]
        self.assertTrue(candidates)
        self.assertTrue(
            all("tools/check_release_tag.py" in step.command for step in candidates)
        )
        self.assertTrue(
            all("--require-main" not in step.command for step in candidates)
        )
        manual = {
            (
                'uv run /bin/sh integrations/macos-ui/fuzz.sh "$FUZZ_OUTPUT" '
                "-runs=-1 -max_total_time=1800"
            )
        }
        guide = (PROJECT_ROOT / "QA.md").read_text(encoding="utf-8")
        self.assertTrue(all(command in guide for command in manual))
        self.assertEqual(_workflow_steps(), mirrored | CI_ONLY | manual)
        self.assertFalse(mirrored & CI_ONLY)

    def test_linux_image_uses_the_workflow_uv_and_interpreter(self) -> None:
        workflows = [
            path.read_text(encoding="utf-8") for path in WORKFLOWS.glob("*.yml")
        ]
        uv_versions = {
            match
            for text in workflows
            for match in re.findall(r"^ +version: (\S+)$", text, re.MULTILINE)
        }
        configuration = tomllib.loads((PROJECT_ROOT / "pyproject.toml").read_text())
        required = configuration["tool"]["uv"]["required-version"]
        self.assertTrue(required.startswith("=="))
        self.assertEqual(uv_versions, {required.removeprefix("==")})
        (version,) = uv_versions
        self.assertIn(
            f"FROM ghcr.io/astral-sh/uv:{version}@sha256:",
            local_ci.LINUX_IMAGE_DEFINITION,
        )
        self.assertTrue(local_ci.LINUX_IMAGE.endswith(f":uv-{version}"))
        self.assertIn(
            f"python-version: {local_ci.CANONICAL_PYTHON}\n",
            (WORKFLOWS / "mutation.yml").read_text(encoding="utf-8"),
        )

    def test_interpreter_pin_and_local_ci_lanes_are_consistent(self) -> None:
        pinned = (PROJECT_ROOT / ".python-version").read_text().strip()
        self.assertEqual(local_ci.CANONICAL_PYTHON, pinned)
        self.assertEqual(local_ci.WORKFLOW_PYTHONS, (pinned, pinned + "t"))

    def test_setup_uv_preserves_safe_cache_defaults(self) -> None:
        for path in WORKFLOWS.glob("*.yml"):
            content = path.read_text()
            self.assertNotIn("enable-cache: true", content)
            self.assertIn("enable-cache: auto", content)
            self.assertIn("cache-dependency-glob: uv.lock", content)
            for action in re.findall(r"uses: (\S+)", content):
                if action.startswith("./"):
                    self.assertIn(
                        action,
                        {
                            "./.github/workflows/swift-fuzz.yml",
                            "./.github/actions/runtime-source",
                        },
                    )
                    target = PROJECT_ROOT / action
                    target = target / "action.yml" if target.is_dir() else target
                    self.assertTrue(target.is_file())
                    continue
                self.assertRegex(action, r"^[\w.-]+/[\w.-]+@[0-9a-f]{40}$")

    def test_runtime_cache_action_rechecks_pinned_inputs(self) -> None:
        content = (
            PROJECT_ROOT / ".github/actions/runtime-source/action.yml"
        ).read_text()
        for action in re.findall(r"uses: (\S+)", content):
            self.assertRegex(action, r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")
        self.assertIn("runtime-source.toml", content)
        self.assertIn("tools.macos_runtime_source --directory", content)
        self.assertIn("EML_RUNTIME_SOURCE_DIRECTORY", content)
        self.assertNotIn("restore-keys:", content)

    def test_block_scalars_are_read_until_their_indentation_ends(self) -> None:
        text = (
            "      - run: >-\n"
            "          first\n"
            "\n"
            "          second\n"
            "        shell: bash\n"
            "        run: |\n"
            "          literal   line\n"
            "      - run: inline  text\n"
        )
        self.assertEqual(
            list(_run_steps(text)),
            ["first second", "literal line", "inline text"],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
