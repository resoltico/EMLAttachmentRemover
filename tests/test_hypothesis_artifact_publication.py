"""Verify public Hypothesis observation sanitization and input contracts."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import finalize_hypothesis_artifacts as artifacts

from tests.hypothesis_artifact_support import write_observations


class HypothesisArtifactPublicationTests(unittest.TestCase):
    """Verify that retained observations contain no machine-private metadata."""

    def test_finalize_removes_constants_and_sanitizes_every_record(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            home = root / "private-home"
            constants = root / ".hypothesis" / "constants"
            constants.mkdir(parents=True)
            patches = root / ".hypothesis" / "patches"
            patches.mkdir()
            (constants / "discovery").write_text(
                f"# file: {root / 'tests' / 'public.py'}\n",
                encoding="utf-8",
            )
            (patches / "suggestion.patch").write_text(
                "From: local@example.test\n",
                encoding="utf-8",
            )
            path = write_observations(
                root,
                [
                    {
                        "coverage": {str(root / "tests" / "public.py"): [1, 2]},
                        "metadata": {
                            "imported_at": 1.0,
                            "os.getpid": 123,
                            "os.getpid()": 456,
                            "public": "retained",
                            "sys.argv": ["pytest", str(root)],
                        },
                        "details": {
                            "home": str(home / "mailbox"),
                            "project": str(root / "source"),
                            "python": str(Path(sys.prefix) / "bin"),
                            "surrogate": "\ud82f",
                        },
                        "arguments": {"payload": "public generated value"},
                        "representation": (
                            "test_public(payload='public generated value')"
                        ),
                        "property": "tests/test_public.py::test_public",
                        f"key:{root}": "public",
                    },
                    {"coverage": {}, "status": "passed"},
                ],
            )
            with patch.object(Path, "home", return_value=home):
                result = artifacts.finalize(root, observations=True)
            content = path.read_text(encoding="utf-8")
            records = [json.loads(line) for line in content.splitlines()]

        self.assertEqual(result, (1, 2))
        self.assertFalse(constants.exists())
        self.assertFalse(patches.exists())
        self.assertNotIn(str(root), content)
        self.assertNotIn(str(home), content)
        self.assertNotIn(str(Path(sys.prefix)), content)
        self.assertIn("<project-root>", content)
        self.assertIn("<user-home>", content)
        self.assertIn(r"\ud82f", content)
        self.assertIsNone(records[0]["coverage"])
        self.assertEqual(records[0]["arguments"], {"payload": "<redacted>"})
        self.assertEqual(
            records[0]["representation"],
            "<redacted; structured argument names retained>",
        )
        self.assertEqual(records[0]["metadata"], {"public": "retained"})
        self.assertIsNone(records[1]["coverage"])

    def test_escaped_binary_renderings_are_redacted_before_path_validation(
        self,
    ) -> None:
        rendered = repr(b"\\\x8e\xd8?\x0b\x919\xc1\x9c)5lZCh")
        unknown_path = "/private/var/folders/private-generated-case"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(
                root,
                [
                    {
                        "arguments": {
                            "payload": rendered,
                            "source": unknown_path,
                        },
                        "representation": f"test_public(payload={rendered})",
                        "status": "passed",
                    }
                ],
            )

            self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))
            content = path.read_text(encoding="utf-8")
            record = json.loads(content)

        self.assertEqual(
            record["arguments"],
            {"payload": "<redacted>", "source": "<redacted>"},
        )
        self.assertNotIn(unknown_path, content)
        self.assertEqual(
            record["representation"],
            "<redacted; structured argument names retained>",
        )

    def test_unknown_absolute_posix_windows_and_unc_paths_block_publication(
        self,
    ) -> None:
        cases = (
            "/private/var/folders/public-case",
            "/" + "tmp/pytest-of-public/test_case",
            "C:\\Users\\private-user\\case",
            "\\\\server\\private-share\\case",
            "file:///private/var/public-case",
            "file://server/private-share/case",
        )
        for machine_path in cases:
            with (
                self.subTest(path=machine_path),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                path = write_observations(root, [{"argument": machine_path}])
                original = path.read_text(encoding="utf-8")
                with self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "machine-specific absolute path remains",
                ) as raised:
                    artifacts.finalize(root, observations=True)
                self.assertEqual(path.read_text(encoding="utf-8"), original)
                self.assertNotIn(machine_path, str(raised.exception))
                self.assertNotIn(str(root), str(raised.exception))

    def test_true_unc_path_reports_safe_record_and_field_without_the_value(
        self,
    ) -> None:
        machine_path = r"\\private-server\private-share\case"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            write_observations(
                root,
                [{"metadata": {"notes": [machine_path]}}],
            )

            with self.assertRaises(artifacts.HypothesisArtifactError) as raised:
                artifacts.finalize(root, observations=True)

        self.assertEqual(
            str(raised.exception),
            "machine-specific absolute path remains in Hypothesis observation "
            "'2026-01-01_testcases.jsonl':1 at $.metadata.notes[0]; "
            "value redacted",
        )
        self.assertNotIn(machine_path, str(raised.exception))
        self.assertNotIn(str(root), str(raised.exception))

    def test_retained_metadata_path_diagnostics_name_exact_nested_fields(self) -> None:
        machine_path = "/private/var/folders/private-observation"
        cases = (
            ({"metadata": {"traceback": machine_path}}, "$.metadata.traceback"),
            (
                {"metadata": {"interesting_origin": {"filename": machine_path}}},
                "$.metadata.interesting_origin.filename",
            ),
            (
                {
                    "metadata": {
                        "interesting_origin": {"context": {"filename": machine_path}}
                    }
                },
                "$.metadata.interesting_origin.context.filename",
            ),
            (
                {
                    "metadata": {
                        "interesting_origin": {
                            "group_elems": [{"filename": machine_path}]
                        }
                    }
                },
                "$.metadata.interesting_origin.group_elems[0].filename",
            ),
        )
        for record, field in cases:
            with (
                self.subTest(field=field),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                path = write_observations(root, [record])
                original = path.read_text(encoding="utf-8")

                with self.assertRaises(artifacts.HypothesisArtifactError) as raised:
                    artifacts.finalize(root, observations=True)

                self.assertEqual(
                    str(raised.exception),
                    "machine-specific absolute path remains in Hypothesis observation "
                    f"'2026-01-01_testcases.jsonl':1 at {field}; value redacted",
                )
                self.assertEqual(path.read_text(encoding="utf-8"), original)
                self.assertNotIn(machine_path, str(raised.exception))
                self.assertNotIn(str(root), str(raised.exception))

    def test_known_prefixes_in_retained_metadata_become_public_placeholders(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            home = root / "private-home"
            path = write_observations(
                root,
                [
                    {
                        "metadata": {
                            "interesting_origin": {"filename": str(home / "origin.py")},
                            "notes": [str(Path(sys.prefix) / "module.py")],
                            "traceback": str(root / "tests" / "test_public.py"),
                        }
                    }
                ],
            )
            with patch.object(Path, "home", return_value=home):
                self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))
            content = path.read_text(encoding="utf-8")

        self.assertIn("<project-root>/tests/test_public.py", content)
        self.assertIn("<user-home>/origin.py", content)
        self.assertIn("<python-prefix>/module.py", content)
        self.assertNotIn(str(root), content)
        self.assertNotIn(str(home), content)
        self.assertNotIn(str(Path(sys.prefix)), content)

    def test_public_urls_and_project_relative_test_ids_are_not_paths(self) -> None:
        public = (
            "https://example.test/path/to/resource?next=/relative/value",
            "tests/test_public.py::test_public",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [{"values": list(public)}])
            self.assertEqual(artifacts.finalize(root, observations=True), (1, 1))
            self.assertEqual(
                json.loads(path.read_text(encoding="utf-8"))["values"],
                list(public),
            )

    def test_absent_artifacts_and_empty_observation_file_are_valid(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            self.assertEqual(artifacts.finalize(root), (0, 0))
            self.assertEqual(artifacts.finalize(root, observations=True), (0, 0))
            path = write_observations(root, [])
            self.assertEqual(artifacts.finalize(root, observations=True), (1, 0))
            self.assertEqual(path.read_text(encoding="utf-8"), "")

    def test_observation_content_must_be_strict_json_objects(self) -> None:
        cases = (
            ("\n", "blank Hypothesis observation"),
            ("{invalid\n", "invalid Hypothesis observation"),
            ("[]\n", "must be a JSON object"),
        )
        for content, message in cases:
            with (
                self.subTest(message=message),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                path = write_observations(root, [])
                path.write_text(content, encoding="utf-8")
                with self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    message,
                ):
                    artifacts.finalize(root, observations=True)

    def test_observation_files_must_be_utf8_regular_jsonl(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            path = write_observations(root, [])
            path.write_bytes(b"\xff")
            with self.assertRaisesRegex(
                artifacts.HypothesisArtifactError,
                "cannot read Hypothesis observations",
            ):
                artifacts.finalize(root, observations=True)
        for name in ("unexpected.txt", "nested.jsonl"):
            with (
                self.subTest(name=name),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                observed = root / ".hypothesis" / "observed"
                observed.mkdir(parents=True)
                unexpected = observed / name
                if name.endswith(".txt"):
                    unexpected.write_text("{}\n", encoding="utf-8")
                else:
                    unexpected.mkdir()
                with self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "unexpected Hypothesis observation artifact",
                ):
                    artifacts.finalize(root, observations=True)

    def test_artifact_directories_must_not_be_files_or_links(self) -> None:
        for relative in (
            Path(".hypothesis/constants"),
            Path(".hypothesis/observed"),
        ):
            with (
                self.subTest(path=relative),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory).resolve()
                target = root / relative
                target.parent.mkdir(parents=True)
                target.write_text("unsafe", encoding="utf-8")
                with self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "must be a real directory",
                ):
                    artifacts.finalize(root, observations=True)
                target.unlink()
                target.symlink_to(root)
                with self.assertRaisesRegex(
                    artifacts.HypothesisArtifactError,
                    "must be a real directory",
                ):
                    artifacts.finalize(root, observations=True)


if __name__ == "__main__":
    unittest.main(verbosity=2)
