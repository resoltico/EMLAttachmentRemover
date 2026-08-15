"""Focused command test for downloaded release-set verification."""

from __future__ import annotations

import io
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import qualify_release


class ReleaseVerifierCommandTests(unittest.TestCase):
    """Ensure verifier mode cannot start a new artifact build."""

    def test_main_can_verify_without_building(self) -> None:
        paths = (Path("/public/alpha"), Path("/public/zeta"))
        directory = Path("/public/release-dist")
        with (
            patch.object(
                qualify_release,
                "verify_release_directory",
                return_value=paths,
            ) as verify,
            patch.object(qualify_release, "qualify_release") as build,
            patch("sys.stdout", new_callable=io.StringIO),
        ):
            status = qualify_release.main(["--verify-directory", str(directory)])
        self.assertEqual(status, 0)
        verify.assert_called_once_with(directory)
        build.assert_not_called()


if __name__ == "__main__":
    unittest.main(verbosity=2)
