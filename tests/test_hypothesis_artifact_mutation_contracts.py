"""Mutation-resistant contracts for public Hypothesis artifact handling."""

from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import call, patch

from tools import finalize_hypothesis_artifacts as artifacts
from tools import hypothesis_observation_safety as observation_safety

from tests.hypothesis_artifact_support import write_observations


class ArtifactMutationContracts(unittest.TestCase):
    """Pin observable defaults, diagnostics, and public serialization."""

    def test_path_prefix_spellings_cover_posix_and_windows_forms(self) -> None:
        self.assertEqual(
            observation_safety.path_prefix_variants("/private/project"),
            ("/private/project",),
        )
        self.assertEqual(
            set(observation_safety.path_prefix_variants(r"C:\private\project")),
            {r"C:\private\project", "C:/private/project", r"C:\\private\\project"},
        )

    def test_public_path_replacement_has_explicit_platform_semantics(self) -> None:
        replacements = (
            ("/private/project", "<posix-root>"),
            (r"C:\Private\Project", "<windows-root>"),
        )
        value = (
            "/private/project\\child\n"
            "/PRIVATE/PROJECT\\case-sensitive\n"
            r"c:\private\PROJECT\\\\nested"
        )

        self.assertEqual(
            observation_safety.public_text(value, replacements),
            "<posix-root>/child\n"
            "/PRIVATE/PROJECT\\case-sensitive\n"
            "<windows-root>/nested",
        )
        self.assertTrue(
            observation_safety.private_prefix_remains(
                r"retained c:\private\PROJECT\mail.eml",
                ((r"C:\Private\Project", "<windows-root>"),),
            )
        )
        self.assertFalse(
            observation_safety.private_prefix_remains(
                "/PRIVATE/PROJECT/mail.eml",
                (("/private/project", "<posix-root>"),),
            )
        )

    def test_replacement_prefixes_omit_a_resolved_filesystem_root(self) -> None:
        with patch.object(
            Path,
            "resolve",
            autospec=True,
            return_value=Path(os.sep),
        ):
            self.assertEqual(
                observation_safety.replacement_prefixes(Path("project")),
                (),
            )

    def test_default_finalize_removes_local_data_without_publishing_observations(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            patches = root / ".hypothesis" / "patches"
            patches.mkdir(parents=True)
            observation = write_observations(root, [{"value": str(root)}])
            original = observation.read_text(encoding="utf-8")

            self.assertEqual(artifacts.finalize(root), (0, 0))

            self.assertFalse(patches.exists())
            self.assertEqual(observation.read_text(encoding="utf-8"), original)

    def test_observation_directory_error_retains_its_public_context(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            observed = root / ".hypothesis" / "observed"
            observed.parent.mkdir()
            observed.write_text("not a directory", encoding="utf-8")
            with self.assertRaises(artifacts.HypothesisArtifactError) as raised:
                artifacts.finalize(root, observations=True)
        self.assertEqual(
            str(raised.exception),
            "Hypothesis observations must be a real directory when present: "
            f"{observed}",
        )

    def test_replacement_map_has_exact_public_tokens_and_prefixes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            root = base / "project"
            home = base / "home"
            python_prefix = base / "python"
            base_prefix = base / "base-python"
            additional = base / "publication"
            for path in (root, home, python_prefix, base_prefix, additional):
                path.mkdir()
            with (
                patch.object(Path, "home", return_value=home),
                patch.object(sys, "prefix", str(python_prefix)),
                patch.object(sys, "base_prefix", str(base_prefix)),
            ):
                replacements = dict(
                    artifacts._replacement_prefixes(  # ruff: ignore[private-member-access]
                        root,
                        ((additional, "<publication-root>"),),
                    )
                )
        self.assertEqual(replacements[str(root)], "<project-root>")
        self.assertEqual(replacements[str(home)], "<user-home>")
        self.assertEqual(replacements[str(python_prefix)], "<python-prefix>")
        self.assertEqual(replacements[str(base_prefix)], "<python-base-prefix>")
        self.assertEqual(replacements[str(additional)], "<publication-root>")
        self.assertNotIn(os.sep, replacements)

    def test_replacement_map_normalizes_windows_style_prefixes(self) -> None:
        root = Path(r"C:\public-project")

        with patch.object(
            Path, "resolve", autospec=True, side_effect=lambda path: path
        ):
            replacement_items = artifacts._replacement_prefixes(  # ruff: ignore[private-member-access]
                root
            )
            replacements = dict(replacement_items)

        self.assertEqual(replacements[r"C:\public-project"], "<project-root>")
        self.assertEqual(replacements["C:/public-project"], "<project-root>")
        self.assertEqual(replacements[r"C:\\public-project"], "<project-root>")
        self.assertEqual(
            artifacts._public_value(  # ruff: ignore[private-member-access]
                r"c:\\PUBLIC-PROJECT\\tests\\test_public.py",
                replacement_items,
            ),
            "<project-root>/tests/test_public.py",
        )
        self.assertEqual(
            artifacts._public_value(  # ruff: ignore[private-member-access]
                r"public\literal",
                replacement_items,
            ),
            r"public\literal",
        )

    def test_nested_nonmetadata_is_retained_and_jsonl_is_canonical_ascii(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(
                root,
                [{"z": "☃", "a": {"sys.argv": "retained"}}],
            )

            self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))

            self.assertEqual(
                path.read_text(encoding="utf-8"),
                '{"a":{"sys.argv":"retained"},"z":"\\u2603"}\n',
            )

    def test_only_top_level_observation_schema_fields_receive_special_policy(
        self,
    ) -> None:
        nested = {
            "arguments": {"payload": "retained"},
            "coverage": "retained",
            "metadata": {"sys.argv": "retained"},
            "representation": "retained",
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(
                root,
                [
                    {
                        "arguments": {"payload": "private raw value"},
                        "coverage": {"private.py": [1]},
                        "features": nested,
                        "metadata": {"sys.argv": ["private"], "public": "retained"},
                        "representation": "private raw representation",
                    }
                ],
            )

            self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))
            record = json.loads(path.read_text(encoding="utf-8"))

        self.assertEqual(record["arguments"], {"payload": "<redacted>"})
        self.assertIsNone(record["coverage"])
        self.assertEqual(record["features"], nested)
        self.assertEqual(record["metadata"], {"public": "retained"})
        self.assertEqual(
            record["representation"],
            "<redacted; structured argument names retained>",
        )

    def test_public_value_preserves_every_json_scalar(self) -> None:
        for value in (None, True, False, 0, 1.5):
            with self.subTest(value=value):
                self.assertIs(
                    artifacts._public_value(value, ()),  # ruff: ignore[private-member-access]
                    value,
                )

    def test_nonmapping_top_level_arguments_are_bounded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [{"arguments": ["raw generated value"]}])

            self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))
            record = json.loads(path.read_text(encoding="utf-8"))

        self.assertEqual(record["arguments"], "<redacted>")

    def test_nested_absolute_key_reports_exact_source_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            write_observations(
                root,
                [{"outer": [{"/unknown/private/path": "public"}]}],
            )
            expected = (
                "machine-specific absolute path remains in Hypothesis observation "
                "'2026-01-01_testcases.jsonl':1 at $.outer[0].<object-key>; "
                "value redacted"
            )

            with self.assertRaises(artifacts.HypothesisArtifactError) as raised:
                artifacts.finalize(root, observations=True)

        self.assertEqual(str(raised.exception), expected)
        self.assertNotIn(str(root), str(raised.exception))

    def test_invalid_second_record_uses_one_based_line_number(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [])
            path.write_text("{}\n\n", encoding="utf-8")

            with self.assertRaises(artifacts.HypothesisArtifactError) as raised:
                artifacts.finalize(root, observations=True)

        self.assertEqual(
            str(raised.exception),
            f"blank Hypothesis observation at {path}:2",
        )

    def test_atomic_replacement_uses_explicit_portable_text_parameters(self) -> None:
        real_named_temporary = tempfile.NamedTemporaryFile
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "public.jsonl"
            path.write_text("stale\n", encoding="utf-8")
            with patch.object(
                tempfile,
                "NamedTemporaryFile",
                wraps=real_named_temporary,
            ) as reserve:
                artifacts._replace_with_temporary(  # ruff: ignore[private-member-access]
                    path,
                    "public\n",
                )
            self.assertEqual(path.read_text(encoding="utf-8"), "public\n")
        reserve.assert_called_once_with(
            mode="w",
            encoding="utf-8",
            newline="\n",
            prefix=".public-hypothesis-",
            dir=path.parent,
            delete=False,
        )

    def test_observation_reader_explicitly_requests_utf8(self) -> None:
        real_read_text = Path.read_text
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [{"status": "passed"}])
            with patch.object(
                Path,
                "read_text",
                autospec=True,
                side_effect=real_read_text,
            ) as read:
                artifacts.finalize(root, observations=True)
        self.assertIn(call(path, encoding="utf-8"), read.call_args_list)

    def test_run_wrapper_forwards_observation_mode_on_success_and_failure(
        self,
    ) -> None:
        root = Path("public-root")
        action_error = RuntimeError("test failed")
        finalization_error = artifacts.HypothesisArtifactError("finalization failed")

        with patch.object(artifacts, "finalize") as finalize:
            artifacts.run_and_finalize(lambda: None, root)
            artifacts.run_and_finalize(lambda: None, root, observations=True)
        self.assertEqual(
            finalize.call_args_list,
            [
                call(root, observations=False),
                call(root, observations=True),
            ],
        )

        def fail() -> None:
            raise action_error

        with (
            patch.object(artifacts, "finalize", side_effect=finalization_error) as end,
            self.assertRaises(ExceptionGroup) as raised,
        ):
            artifacts.run_and_finalize(fail, root, observations=True)
        end.assert_called_once_with(root, observations=True)
        self.assertEqual(
            str(raised.exception).split(" (2 sub-exceptions)", maxsplit=1)[0],
            "tests and Hypothesis artifact finalization both failed",
        )
        self.assertEqual(
            raised.exception.exceptions, (action_error, finalization_error)
        )

    def test_command_help_and_success_text_are_public_contracts(self) -> None:
        stdout = io.StringIO()
        with (
            patch("sys.stdout", stdout),
            self.assertRaises(SystemExit) as raised,
        ):
            artifacts.main(["--help"])
        self.assertEqual(raised.exception.code, 0)
        self.assertIn(
            "sanitize observation JSONL for public retention",
            stdout.getvalue(),
        )
        self.assertIn(
            "Make repository-local Hypothesis evidence safe for public retention",
            stdout.getvalue(),
        )

        with (
            patch.object(artifacts, "finalize", return_value=(2, 3)),
            patch("sys.stdout", new_callable=io.StringIO) as output,
        ):
            self.assertEqual(artifacts.main([]), 0)
        self.assertEqual(
            output.getvalue(),
            "Hypothesis artifacts finalized: files=2, records=3\n",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
