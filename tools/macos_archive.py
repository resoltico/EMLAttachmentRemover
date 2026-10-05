"""Package and verify the native app's exact public archive surface."""

from __future__ import annotations

import os
import plistlib
import stat
import subprocess
import tempfile
import zipfile
from contextlib import ExitStack
from pathlib import Path, PurePosixPath
from typing import Final

from tools import macos_runtime_archive
from tools.build_timestamp import ZIP_TIME, zip_epoch, zip_extra
from tools.macos_metadata import EXECUTABLE_NAME, FILE_SERVICES, build_number
from tools.release_files import ReleaseQualificationError

ROOT: Final = Path(__file__).resolve().parents[1]
APP: Final = "EML Attachment Remover.app"
STAMP: Final = ZIP_TIME
MAX_TOTAL: Final = 128 * 1024 * 1024
MAX_BUNDLED_TOTAL: Final = 256 * 1024 * 1024
RUNTIME_PREFIX: Final = APP + "/Contents/Resources/Runtime"
MAX_BUNDLE_INFO: Final = 1024 * 1024
UTF8_FILENAME_FLAG: Final = 0x800
RUNTIME_MODES: Final = ("bundled", "external")
UNIX_SYSTEM: Final = 3
DOCUMENTS: Final = {
    "LICENSE": "LICENSE",
    "INSTALL.txt": "integrations/macos-ui/INSTALL.txt",
    "install.sh": "integrations/macos-ui/install.sh",
}

APP_FILES: Final = (
    "Contents/Info.plist",
    "Contents/MacOS/EMLAttachmentRemover",
    "Contents/Resources/remove-eml-attachments.pyz",
    "Contents/Resources/processing-launcher.sh",
    "Contents/Resources/EML.icns",
    "Contents/Resources/Assets.car",
    "Contents/Resources/LICENSE",
    "Contents/Resources/.eml-ui-installation",
    "Contents/_CodeSignature/CodeResources",
)


def archive_name(version: str, architecture: str, runtime_mode: str = "bundled") -> str:
    """Return the version-bound macOS archive basename.

    Returns:
        The macOS ZIP basename containing the supplied release version.

    Raises:
        ValueError: If the architecture is not a release target.

    """
    if architecture not in {"arm64", "x86_64"} or runtime_mode not in RUNTIME_MODES:
        message = "Unsupported macOS architecture"
        raise ValueError(message)
    suffix = "-external-python" if runtime_mode == "external" else ""
    return f"eml_attachment_remover-{version}-macos-{architecture}{suffix}.zip"


def instructions() -> bytes:
    """Return the prebuilt-user installation and trust instructions.

    Returns:
        UTF-8 installation instructions included verbatim in the release.

    """
    return (ROOT / DOCUMENTS["INSTALL.txt"]).read_bytes()


def _surface() -> dict[str, int]:
    files = dict.fromkeys(DOCUMENTS, 0o644)
    files.update({f"{APP}/{name}": 0o644 for name in APP_FILES})
    files[f"{APP}/Contents/MacOS/EMLAttachmentRemover"] = 0o755
    files[f"{APP}/Contents/Resources/processing-launcher.sh"] = 0o755
    directories = {
        str(parent) + "/"
        for name in files
        for parent in PurePosixPath(name).parents
        if str(parent) != "."
    }
    return {**files, **dict.fromkeys(directories, 0o755)}


def _runtime_identity(data: bytes) -> tuple[str, str]:
    """Read edition metadata before selecting a trusted runtime archive.

    Returns:
        The declared runtime mode and CPU.

    Raises:
        ReleaseQualificationError: If edition metadata is invalid.

    """
    value = plistlib.loads(data)
    mode = value.get("EMLRuntimeMode")
    architecture = value.get("EMLArchitecture")
    if mode not in {"bundled", "external"} or architecture not in {"arm64", "x86_64"}:
        message = "Native runtime edition metadata is invalid"
        raise ReleaseQualificationError(message)
    return mode, architecture


def _archive_identity(source: zipfile.ZipFile) -> tuple[str, str]:
    """Bound bundle metadata before selecting or reconstructing runtime resources.

    Returns:
        The validated edition and CPU declarations.

    Raises:
        ReleaseQualificationError: If bundle metadata exceeds its read budget.

    """
    name = f"{APP}/Contents/Info.plist"
    member = source.getinfo(name)
    if member.flag_bits:
        message = "Native ZIP metadata is not canonical"
        raise ReleaseQualificationError(message)
    if not 0 < member.file_size <= MAX_BUNDLE_INFO:
        message = "Native bundle metadata exceeds its read budget"
        raise ReleaseQualificationError(message)
    return _runtime_identity(source.read(member))


def _expanded_surface(runtime: Path | None) -> dict[str, int]:
    """Return complete UNIX modes for the source-derived approved archive tree.

    Returns:
        Every canonical path with its member type and permissions.

    """
    result = {
        name: mode | (stat.S_IFDIR if name.endswith("/") else stat.S_IFREG)
        for name, mode in _surface().items()
    }
    if runtime is not None:
        result.update(macos_runtime_archive.surface(runtime, RUNTIME_PREFIX))
    return result


def _member_bytes(app: Path, name: str) -> bytes:
    """Read one approved input.

    Returns:
        Literal documentation or source bytes.

    Raises:
        ReleaseQualificationError: If an input is missing or symbolic.

    """
    if name.endswith("/"):
        return b""
    if name == "INSTALL.txt":
        return instructions()
    path = (
        app / name.removeprefix(APP + "/")
        if name.startswith(APP + "/")
        else ROOT / DOCUMENTS[name]
    )
    if path.is_symlink() and name.startswith(RUNTIME_PREFIX + "/"):
        return str(path.readlink()).encode("utf-8")
    if path.is_symlink() or not path.is_file():
        message = "Unsafe or missing archive input"
        raise ReleaseQualificationError(message)
    return path.read_bytes()


def package(app: Path, target: Path) -> None:
    """Write only the approved app and installation files into a canonical ZIP."""
    with ExitStack() as resources:
        mode, architecture = _runtime_identity(
            (app / "Contents/Info.plist").read_bytes()
        )
        runtime = (
            resources.enter_context(macos_runtime_archive.reference(architecture))
            if mode == "bundled"
            else None
        )
        surface = _expanded_surface(runtime)
        if runtime is not None:
            macos_runtime_archive.verify(
                app / "Contents/Resources/Runtime", runtime, architecture
            )
        archive = resources.enter_context(zipfile.ZipFile(target, "w"))
        for name, unix_mode in sorted(surface.items()):
            info = zipfile.ZipInfo(name, STAMP)
            info.extra = zip_extra()
            info.create_system = UNIX_SYSTEM
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = unix_mode << 16
            archive.writestr(info, _member_bytes(app, name))


def _extract(archive: Path, destination: Path) -> None:
    """Preflight the source-derived tree before writing extracted files."""
    with ExitStack() as resources:
        source = resources.enter_context(zipfile.ZipFile(archive))
        mode, architecture = _archive_identity(source)
        runtime = (
            resources.enter_context(macos_runtime_archive.reference(architecture))
            if mode == "bundled"
            else None
        )
        surface = _expanded_surface(runtime)
        budget = MAX_BUNDLED_TOTAL if runtime is not None else MAX_TOTAL
        entries, timestamp = _validate_entries(source, surface, budget)
        links = _validated_links(source, surface, runtime)
        _write_entries(source, destination, surface, links)
        _restore_times(destination, entries, timestamp)
        if runtime is not None:
            macos_runtime_archive.verify(
                destination / RUNTIME_PREFIX, runtime, architecture
            )


def _validate_entries(
    source: zipfile.ZipFile,
    surface: dict[str, int],
    budget: int,
) -> tuple[list[zipfile.ZipInfo], int]:
    """Preflight paths, ordering, budgets and canonical ZIP member metadata.

    Returns:
        Approved entries and their absolute build timestamp.

    Raises:
        ReleaseQualificationError: If any metadata is noncanonical.

    """
    entries = source.infolist()
    if [item.filename for item in entries] != sorted(surface):
        message = "Native ZIP has missing, extra, duplicate, or unordered members"
        raise ReleaseQualificationError(message)
    if source.comment:
        message = "Native ZIP archive comment is not canonical"
        raise ReleaseQualificationError(message)
    if sum(item.file_size for item in entries) > budget:
        message = "Native ZIP exceeds the extraction budget"
        raise ReleaseQualificationError(message)
    stamp = entries[0].date_time
    timestamp = _timestamp(entries[0].extra)
    for item in entries:
        if (
            item.create_system,
            item.date_time,
            item.external_attr >> 16,
            item.compress_type,
        ) != (UNIX_SYSTEM, stamp, surface[item.filename], zipfile.ZIP_DEFLATED) or any((
            item.flag_bits != (0 if item.filename.isascii() else UTF8_FILENAME_FLAG),
            item.extra != zip_extra(timestamp),
            item.comment,
            item.is_dir() and item.file_size,
        )):
            message = "Native ZIP metadata is not canonical"
            raise ReleaseQualificationError(message)
    return entries, timestamp


def _validated_links(
    source: zipfile.ZipFile,
    surface: dict[str, int],
    runtime: Path | None,
) -> dict[str, str]:
    """Require link targets from trusted upstream data before any extraction write.

    Returns:
        The exact approved relative targets.

    Raises:
        ReleaseQualificationError: If an archive link differs from the trusted tree.

    """
    result: dict[str, str] = {}
    for name, mode in surface.items():
        if stat.S_IFMT(mode) != stat.S_IFLNK:
            continue
        if runtime is None:
            message = "Native archive links require a trusted runtime reference"
            raise ReleaseQualificationError(message)
        relative = name.removeprefix(RUNTIME_PREFIX + "/")
        expected = str((runtime / relative).readlink())
        if source.read(name) != expected.encode("utf-8"):
            message = "Native runtime link target differs from source"
            raise ReleaseQualificationError(message)
        result[name] = expected
    return result


def _write_entries(
    source: zipfile.ZipFile,
    destination: Path,
    surface: dict[str, int],
    links: dict[str, str],
) -> None:
    """Materialize only the fully preflighted member set and source-derived links."""
    destination.mkdir(exist_ok=True)
    for name, mode in sorted(surface.items()):
        path = destination / name
        if stat.S_IFMT(mode) == stat.S_IFDIR:
            path.mkdir()
        elif name in links:
            path.symlink_to(links[name])
        else:
            path.write_bytes(source.read(name))
        if name not in links:
            path.chmod(stat.S_IMODE(mode))


def _timestamp(extra: bytes) -> int:
    """Reject malformed build time metadata before extraction.

    Returns:
        The archive's absolute build datetime.

    Raises:
        ReleaseQualificationError: If the field is not canonical.

    """
    try:
        return zip_epoch(extra)
    except ValueError as error:
        message = "Native ZIP metadata is not canonical"
        raise ReleaseQualificationError(message) from error


def _restore_times(
    destination: Path, entries: list[zipfile.ZipInfo], timestamp: int
) -> None:
    """Restore file times, then directory times after writing their children."""
    for item in reversed(entries):
        os.utime(
            destination / item.filename, (timestamp, timestamp), follow_symlinks=False
        )


def _signature(app: Path, architecture: str) -> None:
    """Require an intact ad-hoc signature and the selected CPU slice.

    Raises:
        ReleaseQualificationError: If the release contract is violated.

    """
    subprocess.run(
        ["/usr/bin/codesign", "--verify", "--deep", "--strict", str(app)],
        check=True,
        timeout=60,
    )
    description = subprocess.run(
        ["/usr/bin/codesign", "-dv", "--verbose=2", str(app)],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    ).stderr
    architectures = subprocess.check_output(
        [
            "/usr/bin/xcrun",
            "lipo",
            "-archs",
            str(app / "Contents/MacOS/EMLAttachmentRemover"),
        ],
        text=True,
        timeout=30,
    )
    if "Signature=adhoc" not in description or architectures.split() != [architecture]:
        message = "Native identity or architecture contract failed"
        raise ReleaseQualificationError(message)


def verify(
    archive: Path,
    zipapp: Path,
    version: str,
    architecture: str,
    runtime_mode: str = "external",
) -> None:
    """Check extracted bytes, declared compatibility, installers, and signature.

    Raises:
        ReleaseQualificationError: If the release contract is violated.

    """
    with tempfile.TemporaryDirectory(prefix="eml-native-archive-") as temporary:
        directory = Path(temporary)
        _extract(archive, directory)
        for name, source in DOCUMENTS.items():
            if (directory / name).read_bytes() != (ROOT / source).read_bytes():
                message = "Native documentation/installer differs from source"
                raise ReleaseQualificationError(message)
        app = directory / APP
        info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
        expected = {
            "NSServices": FILE_SERVICES,
            "CFBundleExecutable": EXECUTABLE_NAME,
            "EMLRuntimeMode": runtime_mode,
            "EMLArchitecture": architecture,
            "CFBundleShortVersionString": version,
            "CFBundleVersion": build_number(ROOT / "pyproject.toml"),
            "CFBundleIdentifier": "io.github.resoltico.emlattachmentremover",
            "NSHumanReadableCopyright": "Copyright © "
            + next(
                line
                for line in (ROOT / "LICENSE").read_text().splitlines()
                if line.startswith("Copyright (c) ")
            ).removeprefix("Copyright (c) "),
            "LSMinimumSystemVersion": "14.0",
        }
        required = {
            "INSTALL.txt": instructions(),
            f"{APP}/Contents/Resources/remove-eml-attachments.pyz": zipapp.read_bytes(),
            f"{APP}/Contents/Resources/LICENSE": (ROOT / "LICENSE").read_bytes(),
            f"{APP}/Contents/Resources/processing-launcher.sh": (
                ROOT / "integrations/macos-ui/processing-launcher.sh"
            ).read_bytes(),
            f"{APP}/Contents/Resources/.eml-ui-installation": (
                b"EML Attachment Remover native UI managed installation\n"
            ),
        }
        if any(info.get(key) != value for key, value in expected.items()) or any(
            (directory / name).read_bytes() != value for name, value in required.items()
        ):
            message = "Native bundle/source contract failed"
            raise ReleaseQualificationError(message)
        if not (app / "Contents/Resources/Assets.car").stat().st_size or not (
            app / "Contents/Resources/EML.icns"
        ).read_bytes().startswith(b"icns"):
            message = "Native icon resources are missing or invalid"
            raise ReleaseQualificationError(message)
        _signature(app, architecture)
