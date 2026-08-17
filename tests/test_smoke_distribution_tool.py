"""Behavioral tests for installed-distribution smoke validation."""

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from email import policy
from email.message import EmailMessage
from pathlib import Path
from unittest.mock import patch

from tools import smoke_distribution


def _message_bytes(
    *,
    subject: str = smoke_distribution.EXPECTED_SUBJECT,
    body: str | None = smoke_distribution.EXPECTED_BODY,
    attachment: bool = False,
    filename: bool = False,
    content_id: bool = False,
) -> bytes:
    """Return a configurable public EML for semantic-failure tests.

    Returns:
        Serialized public message bytes.

    """
    message = EmailMessage()
    message["Subject"] = subject
    if body is not None:
        message.set_content(body)
    if filename:
        message.set_param("name", "public.txt", header="Content-Type")
    if content_id:
        message["Content-ID"] = "<public-body@example.test>"
    if attachment:
        message.add_attachment(
            b"public attachment",
            maintype="application",
            subtype="octet-stream",
            filename="public.bin",
        )
    return message.as_bytes(policy=policy.SMTP)


class SmokeProcessTests(unittest.TestCase):
    """Exercise fixture generation, command discovery, and bounded execution."""

    def test_environment_removes_import_overrides_and_enables_strictness(self) -> None:
        with patch.dict(os.environ, {"PYTHONPATH": "private", "PUBLIC": "value"}):
            environment = smoke_distribution._environment()  # ruff: ignore[private-member-access]
        self.assertNotIn("PYTHONPATH", environment)
        self.assertEqual(environment["PUBLIC"], "value")
        self.assertEqual(environment["PYTHONDEVMODE"], "1")
        self.assertEqual(environment["PYTHONNOUSERSITE"], "1")
        self.assertEqual(environment["PYTHONWARNINGS"], "error")

    def test_environment_uses_exact_case_sensitive_strict_keys(self) -> None:
        source_environment: dict[str, str] = {}
        with patch.object(
            os.environ,
            "items",
            return_value=source_environment.items(),
        ):
            environment = smoke_distribution._environment()  # ruff: ignore[private-member-access]
        self.assertEqual(
            environment,
            {
                "PYTHONDEVMODE": "1",
                "PYTHONNOUSERSITE": "1",
                "PYTHONWARNINGS": "error",
            },
        )

    def test_installed_command_returns_the_resolved_console_script(self) -> None:
        with patch(
            "tools.smoke_distribution.shutil.which",
            return_value="/public/cmd",
        ) as which:
            command = smoke_distribution._installed_command()  # ruff: ignore[private-member-access]
        self.assertEqual(command, "/public/cmd")
        which.assert_called_once_with(smoke_distribution.COMMAND_NAME)

    def test_installed_command_rejects_a_missing_console_script(self) -> None:
        with (
            patch("tools.smoke_distribution.shutil.which", return_value=None),
            self.assertRaisesRegex(RuntimeError, "did not provide"),
        ):
            smoke_distribution._installed_command()  # ruff: ignore[private-member-access]

    def test_run_command_uses_strict_environment_and_exact_timeout(self) -> None:
        completed = subprocess.CompletedProcess(
            ["/public/cmd", "--version"],
            0,
            stdout="public output\n",
            stderr="",
        )
        environment = {"PUBLIC": "value"}
        with (
            patch.object(smoke_distribution, "_environment", return_value=environment),
            patch(
                "tools.smoke_distribution.subprocess.run",
                return_value=completed,
            ) as run,
        ):
            result = smoke_distribution._run_command(  # ruff: ignore[private-member-access]
                "/public/cmd",
                "--version",
            )
        self.assertIs(result, completed)
        run.assert_called_once_with(
            ["/public/cmd", "--version"],
            env=environment,
            text=True,
            capture_output=True,
            check=False,
            timeout=smoke_distribution.SUBPROCESS_TIMEOUT_SECONDS,
        )

    def test_run_command_reports_stderr_for_a_failed_process(self) -> None:
        completed = subprocess.CompletedProcess(
            ["/public/cmd"],
            7,
            stdout="public stdout",
            stderr="public stderr",
        )
        with (
            patch("tools.smoke_distribution.subprocess.run", return_value=completed),
            self.assertRaisesRegex(RuntimeError, "status 7: public stderr"),
        ):
            smoke_distribution._run_command("/public/cmd")  # ruff: ignore[private-member-access]

    def test_run_command_falls_back_to_stdout_for_a_failed_process(self) -> None:
        completed = subprocess.CompletedProcess(
            ["/public/cmd"],
            8,
            stdout="public stdout",
            stderr="",
        )
        with (
            patch("tools.smoke_distribution.subprocess.run", return_value=completed),
            self.assertRaisesRegex(RuntimeError, "status 8: public stdout"),
        ):
            smoke_distribution._run_command("/public/cmd")  # ruff: ignore[private-member-access]


class SmokeOutputTests(unittest.TestCase):
    """Exercise every source-preservation and output-validation outcome."""

    def test_verify_output_accepts_preserved_source_and_clean_semantics(self) -> None:
        original = _message_bytes()
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(original)
            destination.write_bytes(original)
            smoke_distribution._verify_output(  # ruff: ignore[private-member-access]
                source,
                destination,
                original,
            )
            self.assertEqual(destination.read_bytes(), original)

    def test_verify_output_rejects_a_modified_source(self) -> None:
        original = _message_bytes()
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(b"modified")
            with self.assertRaises(RuntimeError) as raised:
                smoke_distribution._verify_output(  # ruff: ignore[private-member-access]
                    source,
                    destination,
                    original,
                )
            self.assertEqual(
                str(raised.exception),
                "installed command modified its source EML",
            )

    def test_verify_output_rejects_a_missing_destination(self) -> None:
        original = _message_bytes()
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(original)
            with self.assertRaises(RuntimeError) as raised:
                smoke_distribution._verify_output(  # ruff: ignore[private-member-access]
                    source,
                    destination,
                    original,
                )
            self.assertEqual(
                str(raised.exception),
                "installed command did not create a non-empty output EML",
            )

    def test_verify_output_rejects_an_empty_destination(self) -> None:
        original = _message_bytes()
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.eml"
            destination = Path(directory) / "output.eml"
            source.write_bytes(original)
            destination.write_bytes(b"")
            with self.assertRaises(RuntimeError) as raised:
                smoke_distribution._verify_output(  # ruff: ignore[private-member-access]
                    source,
                    destination,
                    original,
                )
            self.assertEqual(
                str(raised.exception),
                "installed command did not create a non-empty output EML",
            )

    def test_verify_output_rejects_each_semantic_violation(self) -> None:
        variants = {
            "subject": _message_bytes(subject="Unexpected subject"),
            "body": _message_bytes(body="Unexpected body"),
            "missing_body": _message_bytes(body=None),
            "attachment": _message_bytes(attachment=True),
            "filename": _message_bytes(filename=True),
            "content_id": _message_bytes(content_id=True),
            "defect": (
                b"Subject: Installed distribution smoke test\r\n"
                b"Content-Type: multipart/mixed; boundary=public\r\n\r\n"
                b"--public\r\nContent-Type: text/plain\r\n\r\nPublic body\r\n"
            ),
        }
        for name, output in variants.items():
            with (
                self.subTest(violation=name),
                tempfile.TemporaryDirectory() as directory,
            ):
                source = Path(directory) / "source.eml"
                destination = Path(directory) / "output.eml"
                source.write_bytes(output)
                destination.write_bytes(output)
                with self.assertRaises(RuntimeError) as raised:
                    smoke_distribution._verify_output(  # ruff: ignore[private-member-access]
                        source,
                        destination,
                        output,
                    )
                self.assertEqual(
                    str(raised.exception),
                    "installed command did not produce the expected text-only EML",
                )


class SmokeMainTests(unittest.TestCase):
    """Exercise installed-artifact orchestration and temporary cleanup."""

    def test_main_checks_version_processes_fixture_and_cleans_temporary_files(
        self,
    ) -> None:
        version_result = subprocess.CompletedProcess(
            ["/public/cmd", "--version"],
            0,
            stdout="remove-eml-attachments 9.8.7\n",
            stderr="",
        )
        process_result = subprocess.CompletedProcess(
            ["/public/cmd"],
            0,
            stdout="",
            stderr="",
        )
        with (
            patch.object(
                smoke_distribution, "_installed_command", return_value="/public/cmd"
            ),
            patch.object(
                smoke_distribution, "distribution_version", return_value="9.8.7"
            ),
            patch.object(
                smoke_distribution,
                "_run_command",
                side_effect=[version_result, process_result],
            ) as run,
            patch.object(smoke_distribution, "_verify_output") as verify_output,
        ):
            status = smoke_distribution.main()
        self.assertEqual(status, 0)
        self.assertEqual(run.call_args_list[0].args, ("/public/cmd", "--version"))
        process_arguments = run.call_args_list[1].args
        self.assertEqual(process_arguments[0:2], ("/public/cmd", "--output"))
        source = Path(process_arguments[-1])
        destination = Path(process_arguments[2])
        self.assertEqual(process_arguments[-2], "--")
        self.assertFalse(source.parent.exists())
        verified_source, verified_destination, original = verify_output.call_args.args
        self.assertEqual(verified_source, source)
        self.assertEqual(verified_destination, destination)
        self.assertIsInstance(original, bytes)

    def test_main_rejects_a_version_that_disagrees_with_metadata(self) -> None:
        version_result = subprocess.CompletedProcess(
            ["/public/cmd", "--version"],
            0,
            stdout="remove-eml-attachments 9.8.6\n",
            stderr="",
        )
        with (
            patch.object(
                smoke_distribution, "_installed_command", return_value="/public/cmd"
            ),
            patch.object(
                smoke_distribution, "distribution_version", return_value="9.8.7"
            ),
            patch.object(
                smoke_distribution, "_run_command", return_value=version_result
            ),
            self.assertRaisesRegex(RuntimeError, "does not match package metadata"),
        ):
            smoke_distribution.main()

    def test_main_looks_up_exact_distribution_and_uses_portable_eml_names(self) -> None:
        version_result = subprocess.CompletedProcess(
            ["/public/cmd", "--version"],
            0,
            stdout="remove-eml-attachments 9.8.7\n",
            stderr="",
        )
        with (
            patch.object(
                smoke_distribution,
                "_installed_command",
                return_value="/public/cmd",
            ),
            patch.object(
                smoke_distribution,
                "distribution_version",
                return_value="9.8.7",
            ) as version,
            patch.object(
                smoke_distribution,
                "_run_command",
                side_effect=[version_result, subprocess.CompletedProcess([], 0)],
            ) as run,
            patch.object(smoke_distribution, "_verify_output"),
        ):
            self.assertEqual(smoke_distribution.main(), 0)
        version.assert_called_once_with(smoke_distribution.DISTRIBUTION_NAME)
        process_arguments = run.call_args_list[1].args
        self.assertEqual(Path(process_arguments[2]).name, "output.eml")
        self.assertEqual(Path(process_arguments[-1]).name, "source.eml")

    def test_main_cleans_temporary_files_when_processing_fails(self) -> None:
        version_result = subprocess.CompletedProcess(
            ["/public/cmd", "--version"],
            0,
            stdout="remove-eml-attachments 9.8.7\n",
            stderr="",
        )
        temporary_parent: Path | None = None

        def fail_processing(
            command: str, *arguments: str
        ) -> subprocess.CompletedProcess[str]:
            nonlocal temporary_parent
            if arguments == ("--version",):
                return version_result
            temporary_parent = Path(arguments[-1]).parent
            raise RuntimeError(f"public failure from {command}")

        with (
            patch.object(
                smoke_distribution, "_installed_command", return_value="/public/cmd"
            ),
            patch.object(
                smoke_distribution, "distribution_version", return_value="9.8.7"
            ),
            patch.object(
                smoke_distribution, "_run_command", side_effect=fail_processing
            ),
            self.assertRaisesRegex(RuntimeError, "public failure"),
        ):
            smoke_distribution.main()
        self.assertIsNotNone(temporary_parent)
        assert temporary_parent is not None
        self.assertFalse(temporary_parent.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
