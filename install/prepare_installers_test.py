"""Release installer help links must identify the checked-out source revision."""
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("prepare_installers", Path(__file__).with_name("prepare-installers.py"))
prepare_installers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare_installers)


class PrepareInstallersTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "source"
        self.source.mkdir()
        self.original = {
            "install.sh": b"#!/bin/sh\ndocs_revision='main'\nprintf done\n",
            "install.ps1": b"$documentationRevision = 'main'\r\nWrite-Host done\r\n",
        }
        for name, contents in self.original.items():
            (self.source / name).write_bytes(contents)
        self.output = self.root / "staged"

    def test_immutable_links_and_line_endings_without_source_changes(self):
        for revision in ("a" * 40, "b" * 64, "main"):
            with self.subTest(revision=revision):
                prepare_installers.prepare(self.source, self.output, revision)
                for name, original in self.original.items():
                    self.assertEqual((self.source / name).read_bytes(), original)
                    self.assertEqual((self.output / name).read_bytes(), original.replace(b"main", revision.encode("ascii")))

    def test_revision_cannot_inject_a_path_or_script(self):
        for revision in ("", "HEAD", "v1.2.3", "A" * 40, "a" * 39, "a" * 41, "../main", "main'\nexit 0"):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                prepare_installers.prepare(self.source, self.output, revision)
        self.assertFalse(self.output.exists())

    def test_output_cannot_overwrite_sources(self):
        with self.assertRaises(ValueError):
            prepare_installers.prepare(self.source, self.source, "a" * 40)
        self.output.mkdir()
        (self.output / "install.sh").symlink_to(self.source / "install.sh")
        with self.assertRaises(ValueError):
            prepare_installers.prepare(self.source, self.output, "a" * 40)
        self.assertEqual((self.source / "install.sh").read_bytes(), self.original["install.sh"])
        (self.output / "install.sh").unlink()
        os.link(self.source / "install.sh", self.output / "install.sh")
        with self.assertRaises(ValueError):
            prepare_installers.prepare(self.source, self.output, "a" * 40)
        self.assertEqual((self.source / "install.sh").read_bytes(), self.original["install.sh"])

    def test_missing_or_duplicate_marker_refuses_partial_staging(self):
        for contents in (b"no revision", self.original["install.ps1"] * 2):
            (self.source / "install.ps1").write_bytes(contents)
            with self.assertRaises(ValueError):
                prepare_installers.prepare(self.source, self.output, "a" * 40)
            self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
