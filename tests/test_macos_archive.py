"""Adversarial contracts for the public native release archive."""

from __future__ import annotations

import os
import plistlib
import stat
import struct
import subprocess
import zipfile
from typing import TYPE_CHECKING
from unittest.mock import patch

import pytest
from tools import macos_archive as archive
from tools.release_files import ReleaseQualificationError

if TYPE_CHECKING:
    from pathlib import Path


def _bundle(root: Path) -> Path:
    app = root / "input.app"
    for name in archive.APP_FILES:
        path = app / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"public")
    (app / "Contents/Info.plist").write_bytes(
        plistlib.dumps({
            "CFBundleShortVersionString": "4.0.0",
            "CFBundleVersion": "4.0.0",
            "CFBundleIdentifier": "io.github.resoltico.emlattachmentremover",
            "NSHumanReadableCopyright": "Copyright © 2026 Ervins Strauhmanis",
            "LSMinimumSystemVersion": "14.0",
        })
    )
    resources = app / "Contents/Resources"
    (resources / "LICENSE").write_bytes((archive.ROOT / "LICENSE").read_bytes())
    (resources / "ARTWORK.md").write_bytes(
        (archive.ROOT / "integrations/macos-ui/ARTWORK.md").read_bytes()
    )
    (resources / "processing-launcher.sh").write_bytes(
        (archive.ROOT / "integrations/macos-ui/processing-launcher.sh").read_bytes()
    )
    (resources / ".eml-ui-installation").write_bytes(
        b"EML Attachment Remover native UI managed installation\n"
    )
    return app


def test_package_roundtrip_preserves_source_bytes_and_portable_permissions(
    tmp_path: Path,
) -> None:
    app = _bundle(tmp_path)
    first, second = tmp_path / "first.zip", tmp_path / "second.zip"
    archive.package(app, first)
    archive.package(app, second)
    assert first.read_bytes() == second.read_bytes()
    with patch.object(archive, "_signature") as signature:
        archive.verify(
            first, app / "Contents/Resources/remove-eml-attachments.pyz", "4.0.0"
        )
    assert signature.call_args.args[0].name == "EML Attachment Remover.app"
    extracted = tmp_path / "extracted"
    archive._extract(first, extracted)
    executables = {
        "EML Attachment Remover.app/Contents/MacOS/EMLAttachmentRemover",
        "EML Attachment Remover.app/Contents/Resources/processing-launcher.sh",
    }
    for name in archive._surface():
        mode = 0o755 if name.endswith("/") or name in executables else 0o644
        with zipfile.ZipFile(first) as metadata:
            assert stat.S_IMODE(metadata.getinfo(name).external_attr >> 16) == mode
        if os.name != "nt":
            assert stat.S_IMODE((extracted / name).stat().st_mode) == mode
    with zipfile.ZipFile(first) as packaged:
        expected = {
            *archive.DOCUMENTS,
            "INSTALL.txt",
            *("EML Attachment Remover.app/" + name for name in archive.APP_FILES),
        }
        parents = {
            "/".join(name.split("/")[:index]) + "/"
            for name in expected
            for index in range(1, len(name.split("/")))
        }
        assert set(packaged.namelist()) == expected | parents
    assert archive.instructions().startswith(
        "EML Attachment Remover — prebuilt macOS application\n\n".encode()
    )
    assert (extracted / "integrations/macos-ui/RELEASE.md").is_file()
    assert (
        archive.archive_name("4.0.0")
        == "eml_attachment_remover-4.0.0-macos-universal.zip"
    )


@pytest.mark.parametrize("symbolic", [False, True])
def test_package_refuses_missing_or_symbolic_sources(
    tmp_path: Path, *, symbolic: bool
) -> None:
    app = _bundle(tmp_path)
    executable = app / "Contents/MacOS/EMLAttachmentRemover"
    executable.unlink()
    if symbolic:
        executable.symlink_to(tmp_path / "missing")
    with pytest.raises(
        ReleaseQualificationError, match=r"^Unsafe or missing archive input$"
    ):
        archive.package(app, tmp_path / "unsafe.zip")


@pytest.mark.parametrize(
    "attack",
    [
        "extra",
        "traversal",
        "duplicate",
        "missing",
        "mode",
        "symlink",
        "time",
        "system",
        "compression",
        "extra-field",
        "member-comment",
        "archive-comment",
        "budget",
        "directory-payload",
        "order",
    ],
)
def test_untrusted_metadata_is_rejected_before_extraction(
    tmp_path: Path, attack: str
) -> None:
    good, bad = tmp_path / "good.zip", tmp_path / "bad.zip"
    archive.package(_bundle(tmp_path), good)
    with zipfile.ZipFile(good) as source, zipfile.ZipFile(bad, "w") as target:
        entries = (
            list(reversed(source.infolist()))
            if attack == "order"
            else source.infolist()
        )
        for index, item in enumerate(entries):
            data = source.read(item)
            if index == 0 and attack == "missing":
                continue
            if index == 0:
                _change_metadata(item, attack)
                data = b"hidden" if attack == "directory-payload" else data
            target.writestr(item, data)
        if attack in {"extra", "traversal"}:
            target.writestr(
                "../escaped" if attack == "traversal" else "unexpected", b"public"
            )
        if attack == "duplicate":
            with pytest.warns(UserWarning, match="Duplicate name"):
                target.writestr(item, data)
        if attack == "archive-comment":
            target.comment = b"unapproved"
    destination = tmp_path / "untouched"
    with (
        patch.object(
            archive, "MAX_TOTAL", 0 if attack == "budget" else archive.MAX_TOTAL
        ),
        pytest.raises(ReleaseQualificationError, match=_diagnostic(attack)),
    ):
        archive._extract(bad, destination)
    assert not destination.exists()
    assert not (tmp_path / "escaped").exists()


@pytest.mark.parametrize(
    "changed",
    ["documentation", "version", "processor", "marker", "identifier", "copyright"],
)
def test_archive_contract_rejects_content_drift(tmp_path: Path, changed: str) -> None:
    app = _bundle(tmp_path)
    if changed == "version":
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps({}))
    if changed in {"identifier", "copyright"}:
        info_path = app / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        info[
            "CFBundleIdentifier"
            if changed == "identifier"
            else "NSHumanReadableCopyright"
        ] = "wrong"
        info_path.write_bytes(plistlib.dumps(info))
    if changed == "marker":
        (app / "Contents/Resources/.eml-ui-installation").write_bytes(b"invalid")
    target = tmp_path / "public.zip"
    archive.package(app, target)
    processor = tmp_path / "processor.pyz"
    processor.write_bytes(b"different" if changed == "processor" else b"public")
    if changed == "documentation":
        alternate = tmp_path / "altered.zip"
        with (
            zipfile.ZipFile(target) as source,
            zipfile.ZipFile(alternate, "w") as destination,
        ):
            for item in source.infolist():
                destination.writestr(
                    item,
                    b"changed" if item.filename == "LICENSE" else source.read(item),
                )
        target = alternate
    message = (
        "Native documentation/installer differs from source"
        if changed == "documentation"
        else "Native bundle/source contract failed"
    )
    with (
        patch.object(archive, "_signature"),
        pytest.raises(ReleaseQualificationError, match="^" + message + "$"),
    ):
        archive.verify(target, processor, "4.0.0")


@pytest.mark.parametrize(
    ("identity", "architectures", "accepted"),
    [
        ("Signature=adhoc", "arm64 x86_64", True),
        ("Authority=Developer ID", "arm64 x86_64", False),
        ("Signature=adhoc", "arm64", False),
    ],
)
def test_signature_and_cpu_contract(
    tmp_path: Path, identity: str, architectures: str, *, accepted: bool
) -> None:
    with (
        patch.object(
            subprocess,
            "run",
            return_value=subprocess.CompletedProcess([], 0, stderr=identity),
        ) as run,
        patch.object(subprocess, "check_output", return_value=architectures) as slices,
    ):
        if accepted:
            archive._signature(tmp_path)
        else:
            with pytest.raises(
                ReleaseQualificationError,
                match=r"^Native identity or architecture contract failed$",
            ):
                archive._signature(tmp_path)
    slices.assert_called_once_with(
        [
            "/usr/bin/xcrun",
            "lipo",
            "-archs",
            str(tmp_path / "Contents/MacOS/EMLAttachmentRemover"),
        ],
        text=True,
        timeout=30,
    )
    assert run.call_args_list[0].args[0] == [
        "/usr/bin/codesign",
        "--verify",
        "--deep",
        "--strict",
        str(tmp_path),
    ]
    assert run.call_args_list[0].kwargs == {"check": True, "timeout": 60}
    assert run.call_args_list[1].args[0] == [
        "/usr/bin/codesign",
        "-dv",
        "--verbose=2",
        str(tmp_path),
    ]
    assert run.call_args_list[1].kwargs == {
        "check": True,
        "capture_output": True,
        "text": True,
        "timeout": 30,
    }


def _change_metadata(item: zipfile.ZipInfo, attack: str) -> None:
    updates = {
        "mode": ("external_attr", (stat.S_IFDIR | 0o777) << 16),
        "symlink": ("external_attr", (stat.S_IFLNK | 0o755) << 16),
        "time": ("date_time", (2021, 1, 1, 0, 0, 0)),
        "system": ("create_system", 0),
        "compression": ("compress_type", zipfile.ZIP_STORED),
        "extra-field": ("extra", b"\x01\x00\x00\x00"),
        "member-comment": ("comment", b"unapproved"),
    }
    if attack in updates:
        setattr(item, *updates[attack])


def _diagnostic(attack: str) -> str:
    messages = {
        "extra": "Native ZIP has missing, extra, duplicate, or unordered members",
        "missing": "Native ZIP has missing, extra, duplicate, or unordered members",
        "duplicate": "Native ZIP has missing, extra, duplicate, or unordered members",
        "traversal": "Native ZIP has missing, extra, duplicate, or unordered members",
        "order": "Native ZIP has missing, extra, duplicate, or unordered members",
        "budget": "Native ZIP exceeds the extraction budget",
        "archive-comment": "Native ZIP archive comment is not canonical",
    }
    return "^" + messages.get(attack, "Native ZIP metadata is not canonical") + "$"


def test_extraction_budget_accepts_exact_limit_and_rejects_one_byte_over(
    tmp_path: Path,
) -> None:
    target = tmp_path / "public.zip"
    archive.package(_bundle(tmp_path), target)
    with zipfile.ZipFile(target) as packaged:
        total = sum(info.file_size for info in packaged.infolist())
    with patch.object(archive, "MAX_TOTAL", total):
        archive._extract(target, tmp_path / "accepted")
    with (
        patch.object(archive, "MAX_TOTAL", total - 1),
        pytest.raises(
            ReleaseQualificationError,
            match=r"^Native ZIP exceeds the extraction budget$",
        ),
    ):
        archive._extract(target, tmp_path / "rejected")
    assert not (tmp_path / "rejected").exists()


def test_encrypted_flags_are_rejected_before_creating_any_destination(
    tmp_path: Path,
) -> None:
    target = tmp_path / "public.zip"
    archive.package(_bundle(tmp_path), target)
    data = bytearray(target.read_bytes())
    with zipfile.ZipFile(target) as packaged:
        offset = packaged.start_dir
        for info in packaged.infolist():
            struct.pack_into("<H", data, info.header_offset + 6, 1)
            struct.pack_into("<H", data, offset + 8, 1)
            name, extra, comment = struct.unpack_from("<HHH", data, offset + 28)
            offset += 46 + name + extra + comment
    target.write_bytes(data)
    with pytest.raises(
        ReleaseQualificationError, match=r"^Native ZIP metadata is not canonical$"
    ):
        archive._extract(target, tmp_path / "rejected")
    assert not (tmp_path / "rejected").exists()
