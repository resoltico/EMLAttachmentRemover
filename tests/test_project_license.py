"""Keep the project grant and verbatim public license aligned with distributions."""

from __future__ import annotations

import hashlib
import tomllib
from pathlib import Path

import pytest
from tools.archive_surface_policy import DistributionArchiveError
from tools.verify_distribution_archives import verify_distribution_archives

from tests.distribution_archive_support import create_distribution, write_source_archive

ROOT = Path(__file__).resolve().parents[1]


def test_project_license_retains_official_text_and_source_notice() -> None:
    """The independent Mozilla reference digest protects the complete license text."""
    license_bytes = (ROOT / "LICENSE").read_bytes()
    start = license_bytes.index(b"Mozilla Public License Version 2.0\n")
    # Public reference digest of Mozilla's unmodified license text.
    assert hashlib.sha256(license_bytes[start:]).hexdigest() == (
        "3f3d9e0024b1921b067d6f7f88deb4a60"  # pragma: allowlist secret
        "cbe7a78e76c64e3f1d7fc3b779b9d04"  # pragma: allowlist secret
    )
    configuration = tomllib.loads((ROOT / "pyproject.toml").read_text())
    project = configuration["project"]
    assert project["dynamic"] == ["license"]
    assert configuration["tool"]["eml-attachment-remover"]["licenses"] == {
        "software": "MPL-2.0",
        "source": "MPL-2.0 AND LicenseRef-Proprietary-Artwork",
    }
    assert project["license-files"] == ["LICENSE"]
    notice = license_bytes[:start].decode()
    assert "This Source Code Form is subject to the terms" in notice
    assert "eml_attachment_remover-VERSION.tar.gz" in notice
    assert "https://github.com/resoltico/EMLAttachmentRemover/releases" in notice
    assert (
        "native Swift application, editable artwork and build configuration" in notice
    )
    assert "third-party components retain their own notices" in notice


def test_artwork_grant_keeps_code_and_prior_permissions_separate() -> None:
    """Mixed distributions grant code rights without silently relicensing artwork."""
    notice = (ROOT / "LICENSE").read_text()
    assert "These files and images are excluded from the MPL grant" in notice
    assert "including distributions with modified MPL-covered code" in notice
    assert "permissions granted in earlier distributions remain unaffected" in notice


def test_source_only_artwork_license_is_required_and_other_metadata_stays_strict(
    tmp_path: Path,
) -> None:
    """Mixed source licensing cannot relax version or other archive contracts."""
    distribution = create_distribution(tmp_path)
    configuration = distribution.config.read_text().replace(
        'license = "MIT"', 'dynamic = ["license"]'
    )
    configuration += (
        "\n[tool.eml-attachment-remover.licenses]\n"
        'software = "MIT"\n'
        'source = "MIT AND LicenseRef-Proprietary-Artwork"\n'
    )
    distribution.config.write_text(configuration)
    source_metadata = distribution.metadata.replace(
        b"License-Expression: MIT\n",
        b"License-Expression: MIT AND LicenseRef-Proprietary-Artwork\n"
        b"Dynamic: License-Expression\n",
    )
    write_source_archive(
        distribution.source_archive, distribution.contract, source_metadata
    )
    verify_distribution_archives(
        distribution.source_archive,
        distribution.wheel,
        distribution.root,
        distribution.config,
        distribution.public_files,
    )
    for corrupted in (
        source_metadata.replace(b"Dynamic: License-Expression\n", b""),
        source_metadata.replace(b"Version: 1.2.3", b"Version: 9.9.9"),
    ):
        write_source_archive(
            distribution.source_archive, distribution.contract, corrupted
        )
        with pytest.raises(DistributionArchiveError):
            verify_distribution_archives(
                distribution.source_archive,
                distribution.wheel,
                distribution.root,
                distribution.config,
                distribution.public_files,
            )
