"""Package and verify the native app's exact public archive surface."""

from __future__ import annotations

import plistlib
import stat
import subprocess
import tempfile
import zipfile
from pathlib import Path, PurePosixPath
from typing import Final

from tools.release_files import ReleaseQualificationError

ROOT: Final = Path(__file__).resolve().parents[1]
APP: Final = "EML Attachment Remover.app"
STAMP: Final = (2020, 2, 2, 0, 0, 0)
MAX_TOTAL: Final = 128 * 1024 * 1024
UNIX_SYSTEM: Final = 3
DOCUMENTS: Final = (
    "LICENSE",
    "QA.md",
    "integrations/macos-ui/README.md",
    "integrations/macos-ui/ARTWORK.md",
    "integrations/macos-ui/RELEASE.md",
    "integrations/macos-ui/INSTALL.txt",
    "integrations/macos-ui/install.sh",
    "integrations/macos-ui/processing-launcher.sh",
)
APP_FILES: Final = (
    "Contents/Info.plist",
    "Contents/MacOS/EMLAttachmentRemover",
    "Contents/Resources/remove-eml-attachments.pyz",
    "Contents/Resources/processing-launcher.sh",
    "Contents/Resources/EML.icns",
    "Contents/Resources/Assets.car",
    "Contents/Resources/LICENSE",
    "Contents/Resources/ARTWORK.md",
    "Contents/Resources/.eml-ui-installation",
    "Contents/_CodeSignature/CodeResources",
)


def archive_name(version: str, architecture: str) -> str:
    """Return the version-bound macOS archive basename.

    Returns:
        The macOS ZIP basename containing the supplied release version.

    Raises:
        ValueError: If the architecture is not a release target.

    """
    if architecture not in {"arm64", "x86_64"}:
        message = "Unsupported macOS architecture"
        raise ValueError(message)
    return f"eml_attachment_remover-{version}-macos-{architecture}.zip"


def instructions() -> bytes:
    """Return the prebuilt-user installation and trust instructions.

    Returns:
        UTF-8 installation instructions included verbatim in the release.

    """
    return (ROOT / "integrations/macos-ui/INSTALL.txt").read_bytes()


def _surface() -> dict[str, int]:
    files = dict.fromkeys((*DOCUMENTS, "INSTALL.txt"), 0o644)
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
        else ROOT / name
    )
    if path.is_symlink() or not path.is_file():
        message = "Unsafe or missing archive input"
        raise ReleaseQualificationError(message)
    return path.read_bytes()


def package(app: Path, target: Path) -> None:
    """Write only the approved app and installation files into a canonical ZIP."""
    surface = _surface()
    with zipfile.ZipFile(target, "w") as archive:
        for name, mode in sorted(surface.items()):
            info = zipfile.ZipInfo(name, STAMP)
            info.create_system = UNIX_SYSTEM
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (
                mode | (stat.S_IFDIR if name.endswith("/") else stat.S_IFREG)
            ) << 16
            archive.writestr(info, _member_bytes(app, name))


def _extract(archive: Path, destination: Path) -> None:
    """Validate all member metadata before writing any extracted files.

    Raises:
        ReleaseQualificationError: If the release contract is violated.

    """
    surface = _surface()
    with zipfile.ZipFile(archive) as source:
        entries = source.infolist()
        if [item.filename for item in entries] != sorted(surface):
            message = "Native ZIP has missing, extra, duplicate, or unordered members"
            raise ReleaseQualificationError(message)
        if source.comment:
            message = "Native ZIP archive comment is not canonical"
            raise ReleaseQualificationError(message)
        if sum(item.file_size for item in entries) > MAX_TOTAL:
            message = "Native ZIP exceeds the extraction budget"
            raise ReleaseQualificationError(message)
        for item in entries:
            mode = item.external_attr >> 16
            expected_type = stat.S_IFDIR if item.is_dir() else stat.S_IFREG
            if (
                item.create_system,
                item.date_time,
                stat.S_IFMT(mode),
                stat.S_IMODE(mode),
                item.compress_type,
            ) != (
                UNIX_SYSTEM,
                STAMP,
                expected_type,
                surface[item.filename],
                zipfile.ZIP_DEFLATED,
            ) or any((
                item.flag_bits,
                item.extra,
                item.comment,
                item.is_dir() and item.file_size,
            )):
                message = "Native ZIP metadata is not canonical"
                raise ReleaseQualificationError(message)
        destination.mkdir(exist_ok=True)
        for item in entries:
            path = destination / item.filename
            if item.is_dir():
                path.mkdir()
            else:
                path.write_bytes(source.read(item))
            path.chmod(surface[item.filename])


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


def verify(archive: Path, zipapp: Path, version: str, architecture: str) -> None:
    """Check extracted bytes, declared compatibility, installers, and signature.

    Raises:
        ReleaseQualificationError: If the release contract is violated.

    """
    with tempfile.TemporaryDirectory(prefix="eml-native-archive-") as temporary:
        directory = Path(temporary)
        _extract(archive, directory)
        for name in DOCUMENTS:
            if (directory / name).read_bytes() != (ROOT / name).read_bytes():
                message = "Native documentation/installer differs from source"
                raise ReleaseQualificationError(message)
        app = directory / APP
        info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
        expected = {
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
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
            f"{APP}/Contents/Resources/ARTWORK.md": (
                ROOT / "integrations/macos-ui/ARTWORK.md"
            ).read_bytes(),
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
