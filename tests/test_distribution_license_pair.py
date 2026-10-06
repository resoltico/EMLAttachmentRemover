"""Source-only artwork licensing must not disguise metadata or description changes."""

from __future__ import annotations

from typing import TYPE_CHECKING

import pytest
from tools.distribution_archive_contract import DistributionArchiveError

from tests.distribution_archive_support import create_distribution

if TYPE_CHECKING:
    from pathlib import Path


def test_metadata_pair_without_a_source_only_license_is_unchanged(
    tmp_path: Path,
) -> None:
    distribution = create_distribution(tmp_path)
    distribution.contract.verify_pair_metadata(
        distribution.metadata, distribution.metadata, distribution.config
    )


@pytest.mark.parametrize("damaged", [False, True])
def test_source_license_normalization_preserves_multiline_description(
    tmp_path: Path, *, damaged: bool
) -> None:
    distribution = create_distribution(tmp_path)
    with distribution.config.open("a") as stream:
        stream.write(
            "\n[tool.eml-attachment-remover.licenses]\n"
            'software = "MIT"\nsource = "MIT AND LicenseRef-Artwork"\n'
        )
    description = b"Public synthetic README\n\nSecond paragraph."
    (distribution.root / "README.md").write_bytes(description + b"\n")
    wheel = distribution.metadata.replace(b"Public synthetic README", description)
    declaration = (
        b"License-Expression: MIT AND LicenseRef-Artwork\nDynamic: License-Expression\n"
    )
    source = wheel.replace(b"License-Expression: MIT\n", declaration)
    if damaged:
        source = source.replace(declaration, b"License-Expression: MIT\n")
        with pytest.raises(DistributionArchiveError) as caught:
            distribution.contract.verify_pair_metadata(
                source, wheel, distribution.config
            )
        assert str(caught.value) == (
            "sdist license declaration differs from project contract"
        )
    else:
        distribution.contract.verify_pair_metadata(source, wheel, distribution.config)


@pytest.mark.parametrize("license_last", [False, True])
def test_license_normalization_keeps_header_like_description_bytes(
    tmp_path: Path, *, license_last: bool
) -> None:
    distribution = create_distribution(tmp_path)
    with distribution.config.open("a") as stream:
        stream.write(
            "\n[tool.eml-attachment-remover.licenses]\n"
            'software = "MIT"\nsource = "MIT AND LicenseRef-Artwork"\n'
        )
    declaration = (
        b"License-Expression: MIT AND LicenseRef-Artwork\nDynamic: License-Expression\n"
    )
    description = b"Public synthetic README\n\n" + declaration + b"\nFinal paragraph.\n"
    (distribution.root / "README.md").write_bytes(description)
    header = distribution.metadata.partition(b"\n\n")[0]
    if license_last:
        header = header.replace(b"License-Expression: MIT\n", b"")
        header += b"\nLicense-Expression: MIT"
    wheel = header + b"\n\n" + description
    source = (header + b"\n").replace(b"License-Expression: MIT\n", declaration)
    source = source.removesuffix(b"\n") + b"\n\n" + description
    distribution.contract.verify_pair_metadata(source, wheel, distribution.config)
