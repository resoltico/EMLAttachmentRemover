"""Behavioral tests for isolated release subprocess orchestration."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import qualify_release


class ReleaseProcessTests(unittest.TestCase):
    """Exercise strict subprocesses and reproducible candidate construction."""

    def test_strict_environment_removes_import_overrides(self) -> None:
        with patch.dict(
            os.environ,
            {
                "PYTHONPATH": "private",
                "SOURCE_DATE_EPOCH": "private",
                "PUBLIC": "value",
            },
        ):
            environment = qualify_release._strict_environment()
        self.assertNotIn("PYTHONPATH", environment)
        self.assertNotIn("SOURCE_DATE_EPOCH", environment)
        self.assertEqual(environment["PUBLIC"], "value")
        self.assertEqual(environment["PYTHONDEVMODE"], "1")
        self.assertEqual(environment["PYTHONNOUSERSITE"], "1")
        self.assertEqual(environment["PYTHONWARNINGS"], "error")

    def test_run_uses_strict_environment_project_root_and_timeout(self) -> None:
        environment = {"PUBLIC": "value"}
        with (
            patch.object(
                qualify_release,
                "_strict_environment",
                return_value=environment,
            ),
            patch("tools.qualify_release.subprocess.run") as run,
        ):
            qualify_release._run(
                ("public-command", "argument"),
                timeout_seconds=17,
            )
        run.assert_called_once_with(
            ("public-command", "argument"),
            check=True,
            cwd=qualify_release.PROJECT_ROOT,
            env=environment,
            timeout=17,
        )

    def test_build_and_test_requires_exact_build_outputs_and_smokes_both(self) -> None:
        names = qualify_release._artifact_names(
            "public-project",
            "9.8.7",
        )
        wheel = next(name for name in names if name.endswith(".whl"))
        source = next(name for name in names if name.endswith(".tar.gz"))
        with tempfile.TemporaryDirectory() as directory:
            staging = Path(directory)
            build_count = 0

            def run(command: tuple[str, ...], *, timeout_seconds: float) -> None:
                nonlocal build_count
                self.assertEqual(
                    timeout_seconds,
                    qualify_release.COMMAND_TIMEOUT_SECONDS,
                )
                if command[0:2] == ("uv", "build"):
                    build_count += 1
                    output = Path(command[command.index("--out-dir") + 1])
                    (output / wheel).write_bytes(b"wheel")
                    (output / source).write_bytes(b"source")
                    (output / qualify_release.UV_MARKER_NAME).write_text(
                        "generated",
                        encoding="utf-8",
                    )
                if "--target" in command:
                    target = Path(command[command.index("--target") + 1])
                    target.write_bytes(b"zipapp")

            with (
                patch.object(qualify_release, "_run", side_effect=run) as run_mock,
                patch.object(
                    qualify_release.verify_distribution_archives,
                    "verify_distribution_archives",
                ) as verify_archives,
            ):
                qualify_release._build_and_test(
                    staging,
                    names,
                )
            commands = [entry.args[0] for entry in run_mock.call_args_list]
            self.assertEqual(build_count, 2)
            self.assertEqual(len(commands), 5)
            self.assertTrue(
                all(
                    command[0:4]
                    == ("uv", "build", "--no-build-isolation", "--no-sources")
                    for command in commands[:2]
                ),
            )
            self.assertEqual(
                [command[command.index("--with") + 1] for command in commands[2:4]],
                [str(staging / wheel), str(staging / source)],
            )
            self.assertTrue(all("--isolated" in command for command in commands[2:4]))
            self.assertFalse((staging / qualify_release.UV_MARKER_NAME).exists())
            self.assertTrue((staging / qualify_release.ZIPAPP_FILE_NAME).is_file())
            verify_archives.assert_called_once()

    def test_reproducibility_gate_rejects_byte_different_builds(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            staging = Path(directory) / "staging"
            staging.mkdir()
            build_count = 0

            def run(command: tuple[str, ...], *, timeout_seconds: float) -> None:
                nonlocal build_count
                self.assertEqual(
                    timeout_seconds,
                    qualify_release.COMMAND_TIMEOUT_SECONDS,
                )
                build_count += 1
                output = Path(command[command.index("--out-dir") + 1])
                (output / "public.whl").write_bytes(f"wheel-{build_count}".encode())
                (output / "public.tar.gz").write_bytes(b"stable source")

            with (
                patch.object(qualify_release, "_run", side_effect=run),
                self.assertRaisesRegex(
                    qualify_release.ReleaseQualificationError,
                    r"not byte reproducible: \['public\.whl'\]",
                ),
            ):
                qualify_release._build_reproducible_archives(
                    staging,
                    ("public.whl", "public.tar.gz"),
                )
            self.assertEqual(tuple(staging.iterdir()), ())


if __name__ == "__main__":
    unittest.main(verbosity=2)
