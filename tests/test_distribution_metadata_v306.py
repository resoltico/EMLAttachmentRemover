"""Complete v3.0.6 distribution metadata contracts."""

from __future__ import annotations

import tempfile
from pathlib import Path

from tests.distribution_archive_support import create_distribution


def test_distribution_metadata_and_wheel_text_are_complete() -> None:
    """Every public metadata key and generated wheel line is exact."""
    with tempfile.TemporaryDirectory() as directory:
        declared = create_distribution(Path(directory)).contract
    assert declared.expected_metadata() == {
        "Metadata-Version": ("2.5",),
        "Name": ("public-project",),
        "Version": ("1.2.3",),
        "Summary": ("Public synthetic distribution",),
        "Requires-Python": (">=3.14,<3.15",),
        "License-Expression": ("MIT",),
        "License-File": ("LICENSE",),
        "Author": ("Public Example",),
        "Author-email": (),
        "Keywords": ("public,synthetic",),
        "Classifier": ("Topic :: Utilities",),
        "Requires-Dist": (),
        "Provides-Extra": (),
        "Project-URL": (
            "Homepage, https://example.test/public-project",
            "Repository, https://example.test/public-project.git",
        ),
        "Dynamic": (),
        "Description-Content-Type": ("text/markdown",),
    }
    assert declared.expected_wheel_metadata() == (
        "Wheel-Version: 1.0\n"
        "Generator: hatchling 1.32.4\n"
        "Root-Is-Purelib: true\n"
        "Tag: cp314-none-any\n"
    )
