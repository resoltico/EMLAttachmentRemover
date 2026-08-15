"""Define exact portable distribution-archive contracts and shared checks."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING

try:
    from .archive_surface_policy import (
        DistributionArchiveError,
        allowed_directories,
        require_exact_set,
        safe_parts,
        same_content,
    )
except ImportError:
    from archive_surface_policy import (  # type: ignore[import-not-found,no-redef]
        DistributionArchiveError,
        allowed_directories,
        require_exact_set,
        safe_parts,
        same_content,
    )

try:
    from . import distribution_project_metadata
except ImportError:
    import distribution_project_metadata  # type: ignore[import-not-found,no-redef]

__all__ = [
    "ArchiveContract",
    "DistributionArchiveError",
    "allowed_directories",
    "require_exact_set",
    "safe_parts",
    "same_content",
]

if TYPE_CHECKING:
    from collections.abc import Mapping, Sequence
    from email.message import Message
    from pathlib import Path, PurePosixPath


@dataclass(frozen=True, slots=True)
class ArchiveContract:
    """Describe the exact source and installed distribution surfaces."""

    repository_root: Path
    project_name: str
    version: str
    normalized_name: str
    package_roots: tuple[PurePosixPath, ...]
    license_files: tuple[PurePosixPath, ...]
    has_scripts: bool
    summary: str
    requires_python: str
    wheel_tag: str
    wheel_generator: str
    license_expression: str
    authors: tuple[str, ...]
    keywords: tuple[str, ...]
    classifiers: tuple[str, ...]
    dependencies: tuple[str, ...]
    urls: Mapping[str, str]
    readme: PurePosixPath
    scripts: Mapping[str, str]
    public_sources: Mapping[str, Path]

    @classmethod
    def from_project(
        cls,
        repository_root: Path,
        project_config: Path,
        public_files: Sequence[Path],
    ) -> ArchiveContract:
        """Build an archive contract from canonical project metadata.

        Returns:
            The exact normalized release-archive contract.

        """
        public_sources = {
            path.relative_to(repository_root).as_posix(): path for path in public_files
        }
        project = distribution_project_metadata.load_project_metadata(
            project_config,
            public_sources,
        )
        return cls(
            repository_root=repository_root,
            project_name=project.name,
            version=project.version,
            normalized_name=project.normalized_name,
            package_roots=project.package_roots,
            license_files=project.license_files,
            has_scripts=bool(project.scripts),
            summary=project.summary,
            requires_python=project.requires_python,
            wheel_tag=project.wheel_tag,
            wheel_generator=project.wheel_generator,
            license_expression=project.license_expression,
            authors=project.authors,
            keywords=project.keywords,
            classifiers=project.classifiers,
            dependencies=project.dependencies,
            urls=project.urls,
            readme=project.readme,
            scripts=project.scripts,
            public_sources=public_sources,
        )

    @property
    def source_root(self) -> str:
        """The one permitted source-archive root.

        Returns:
            The normalized, versioned source root name.

        """
        return f"{self.normalized_name}-{self.version}"

    @property
    def dist_info(self) -> str:
        """The one permitted wheel metadata directory.

        Returns:
            The normalized, versioned dist-info directory name.

        """
        return f"{self.normalized_name}-{self.version}.dist-info"

    def wheel_sources(self) -> dict[str, Path]:
        """Return wheel member names mapped to authoritative source files.

        Returns:
            Every installed package and license file with its repository source.

        Raises:
            DistributionArchiveError: If build configuration names unsafe or
                non-public source paths.

        """
        sources: dict[str, Path] = {}
        for package_root in self.package_roots:
            safe_parts(package_root.as_posix(), directory=False)
            prefix = f"{package_root.as_posix()}/"
            package_sources = {
                f"{package_root.name}/{name.removeprefix(prefix)}": source
                for name, source in self.public_sources.items()
                if name.startswith(prefix)
            }
            if not package_sources:
                message = f"wheel package root has no audited files: {package_root}"
                raise DistributionArchiveError(message)
            overlap = sources.keys() & package_sources.keys()
            if overlap:
                message = f"wheel package roots overlap: {sorted(overlap)}"
                raise DistributionArchiveError(message)
            sources.update(package_sources)
        self._add_license_sources(sources)
        return sources

    def _add_license_sources(self, sources: dict[str, Path]) -> None:
        """Add every configured audited license to a wheel source mapping.

        Raises:
            DistributionArchiveError: If a configured license is not public.

        """
        for license_file in self.license_files:
            source = self.public_sources.get(license_file.as_posix())
            if source is None:
                message = f"wheel license is not an audited public file: {license_file}"
                raise DistributionArchiveError(message)
            sources[f"{self.dist_info}/licenses/{license_file.as_posix()}"] = source

    def wheel_generated_files(self) -> frozenset[str]:
        """Return the exact generated dist-info member set.

        Returns:
            Mandatory metadata members and the configured entry-points member.

        """
        names = {
            f"{self.dist_info}/METADATA",
            f"{self.dist_info}/RECORD",
            f"{self.dist_info}/WHEEL",
        }
        if self.has_scripts:
            names.add(f"{self.dist_info}/entry_points.txt")
        return frozenset(names)

    def expected_metadata(self) -> dict[str, tuple[str, ...]]:
        """Return canonical core-metadata values derived from pyproject.

        Returns:
            Every release-critical metadata field and its ordered values.

        """
        return {
            "Metadata-Version": ("2.5",),
            "Name": (self.project_name,),
            "Version": (self.version,),
            "Summary": (self.summary,),
            "Requires-Python": (self.requires_python,),
            "License-Expression": (self.license_expression,),
            "License-File": tuple(path.as_posix() for path in self.license_files),
            "Author": self.authors,
            "Author-email": (),
            "Keywords": ((",".join(sorted(self.keywords)),) if self.keywords else ()),
            "Classifier": self.classifiers,
            "Requires-Dist": self.dependencies,
            "Provides-Extra": (),
            "Project-URL": tuple(f"{label}, {url}" for label, url in self.urls.items()),
            "Dynamic": (),
            "Description-Content-Type": ("text/markdown",),
        }

    def verify_metadata(self, metadata: Message) -> None:
        """Require generated core metadata to match canonical project values.

        Raises:
            DistributionArchiveError: If a release-critical field drifts.

        """
        expected_metadata = self.expected_metadata()
        expected_fields = {field.casefold() for field in expected_metadata}
        metadata_fields = metadata.keys()
        unsupported = sorted(
            {
                field
                for field in metadata_fields
                if field.casefold() not in expected_fields
            },
            key=str.casefold,
        )
        if unsupported:
            message = f"generated metadata contains unsupported fields: {unsupported}"
            raise DistributionArchiveError(message)
        for field, expected in expected_metadata.items():
            actual = tuple(metadata.get_all(field, []))
            if field == "Requires-Python":
                actual = tuple(
                    distribution_project_metadata.normalized_requirement(value)
                    for value in actual
                )
                normalized_expected = tuple(
                    distribution_project_metadata.normalized_requirement(value)
                    for value in expected
                )
            else:
                normalized_expected = expected
            if actual != normalized_expected:
                message = (
                    f"generated metadata {field} mismatch: "
                    f"expected={normalized_expected!r}; actual={actual!r}"
                )
                raise DistributionArchiveError(message)
        expected_description = self.public_sources[self.readme.as_posix()].read_bytes()
        actual_description = metadata.get_payload(decode=True)
        if actual_description != expected_description:
            message = "generated metadata description does not match the public README"
            raise DistributionArchiveError(message)

    def expected_entry_points(self) -> str:
        """Return canonical wheel console-entry-point content.

        Returns:
            Deterministically sorted ``entry_points.txt`` text.

        """
        entries = "".join(
            f"{name} = {target}\n" for name, target in sorted(self.scripts.items())
        )
        return f"[console_scripts]\n{entries}"

    def expected_wheel_metadata(self) -> str:
        """Return the exact generated WHEEL metadata contract.

        Returns:
            Backend identity, purelib marker, and precise compatibility tag.

        """
        return (
            "Wheel-Version: 1.0\n"
            f"Generator: {self.wheel_generator}\n"
            "Root-Is-Purelib: true\n"
            f"Tag: {self.wheel_tag}\n"
        )
