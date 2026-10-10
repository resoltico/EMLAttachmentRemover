"""Reject invalid runtime sources and prove download and staging failure boundaries."""

from __future__ import annotations

import hashlib
import io
import json
import runpy
import shutil
import subprocess
import sys
import tarfile
import tomllib
import urllib.request
from pathlib import Path

import pytest
from tools import macos_runtime, macos_runtime_archive, macos_runtime_source

from tests.test_macos_runtime_source import runtime_archive_fixture


@pytest.mark.parametrize(
    "url",
    [
        "http://github.com/astral-sh/python-build-standalone/releases/download/x/y",
        "https://other.example/astral-sh/python-build-standalone/releases/download/x/y",
        "https://github.com/other/releases/download/x/y",
        "https://github.com/astral-sh/python-build-standalone/releases/download/x/y?q=1",
        "https://github.com/astral-sh/python-build-standalone/releases/download/x/y#part",
    ],
)
def test_runtime_pin_rejects_unapproved_source_urls(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, url: str
) -> None:
    """Artifact declarations cannot change the accepted source scheme or origin."""
    config = tmp_path / "runtime-source.toml"
    config.write_text(
        f'[arm64]\nurl={json.dumps(url)}\nsha256="digest"\nsize=1\ntarget="target"\n'
    )
    monkeypatch.setattr(macos_runtime_source, "CONFIG", config)
    with pytest.raises(
        ValueError, match=r"^runtime source must be an HTTPS upstream GitHub release$"
    ):
        macos_runtime_source.pin("arm64")


def test_runtime_pin_requires_complete_typed_fields(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Missing architecture declarations fail before any filesystem preparation."""
    config = tmp_path / "runtime-source.toml"
    config.write_text("[arm64]\nsize=true\n")
    monkeypatch.setattr(macos_runtime_source, "CONFIG", config)
    with pytest.raises(ValueError, match=r"^invalid runtime archive pin$"):
        macos_runtime_source.pin("arm64")


def test_runtime_size_and_symbolic_source_are_refused(tmp_path: Path) -> None:
    """The recorded digest is insufficient when size or source file type differs."""
    archive, pin = runtime_archive_fixture(tmp_path)
    wrong_size = pin.copy()
    wrong_size["size"] += 1
    with pytest.raises(
        ValueError, match=r"^runtime archive size or file type differs from pin$"
    ):
        macos_runtime_source.verify(archive, wrong_size)
    link = tmp_path / "linked.tar.zst"
    link.symlink_to(archive.name)
    with pytest.raises(
        ValueError, match=r"^runtime archive size or file type differs from pin$"
    ):
        macos_runtime_source.verify(link, pin)


@pytest.mark.parametrize("overflow", [False, True])
def test_download_is_bounded_and_checked_against_the_pin(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, overflow: bool
) -> None:
    """A valid response is verified; a larger response stops without accepting it."""
    archive, pin = runtime_archive_fixture(tmp_path)
    original = archive.read_bytes()
    received = original + b"extra" if overflow else original
    requests: list[tuple[str, int]] = []

    def response(url: str, *, timeout: int) -> io.BytesIO:
        requests.append((url, timeout))
        return io.BytesIO(received)

    monkeypatch.setattr(urllib.request, "urlopen", response)
    destination = tmp_path / "download.tar.zst"
    if overflow:
        with pytest.raises(ValueError, match="exceeds pinned"):
            macos_runtime_source.download(destination, pin)
        assert destination.stat().st_size == 0
    else:
        macos_runtime_source.download(destination, pin)
        assert destination.read_bytes() == original
    assert requests == [(pin["url"], 30)]


def test_runtime_preparation_requires_a_fresh_destination(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An existing runtime remains untouched rather than becoming a staging tree."""
    target = tmp_path / "Runtime"
    target.mkdir()
    sentinel = target / "keep"
    sentinel.write_bytes(b"existing")

    def unexpected_pin(_architecture: str) -> macos_runtime_source.RuntimePin:
        pytest.fail("Existing destinations must be refused before runtime acquisition")

    monkeypatch.setattr(macos_runtime_source, "pin", unexpected_pin)
    with pytest.raises(ValueError, match=r"^use a fresh private runtime destination$"):
        macos_runtime.prepare(target, "arm64")
    assert sentinel.read_bytes() == b"existing"


def test_empty_or_wrong_cpu_native_code_is_not_approved(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Native identity requires code and the selected CPU, beyond ordinary resources."""
    root = tmp_path / "Runtime"
    root.mkdir()
    with pytest.raises(ValueError, match=r"^runtime has no native executable code$"):
        macos_runtime.require_native(root, "arm64")
    (root / "program").write_bytes(b"\xcf\xfa\xed\xfe" + b"fake-code")
    monkeypatch.setattr(macos_runtime, "_command", lambda _args: "x86_64")
    with pytest.raises(
        ValueError, match=r"^runtime binary architecture differs from selected CPU$"
    ):
        macos_runtime.require_native(root, "arm64")


def test_signing_visits_deeper_components_first_and_verifies_every_file(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    paths = [
        tmp_path / "z/program",
        tmp_path / "a/deep/program",
        tmp_path / "program",
    ]
    commands: list[list[str]] = []

    def command(arguments: list[str]) -> str:
        commands.append(arguments)
        return ""

    monkeypatch.setattr(macos_runtime, "_command", command)
    macos_runtime.__dict__["_sign"](paths)
    assert commands == [
        ["/usr/bin/codesign", "--force", "--sign", "-", str(paths[1])],
        ["/usr/bin/codesign", "--verify", "--strict", str(paths[1])],
        ["/usr/bin/codesign", "--force", "--sign", "-", str(paths[0])],
        ["/usr/bin/codesign", "--verify", "--strict", str(paths[0])],
        ["/usr/bin/codesign", "--force", "--sign", "-", str(paths[2])],
        ["/usr/bin/codesign", "--verify", "--strict", str(paths[2])],
    ]


def test_copy_removes_static_files_but_preserves_directory_names(
    tmp_path: Path,
) -> None:
    """Only static archives are omitted; similarly named directories are retained."""
    archive, pin = runtime_archive_fixture(tmp_path)
    source = macos_runtime_source.extract(archive, tmp_path / "extracted", pin)
    (source / "install/unused.a").write_bytes(b"static build archive")
    (source / "install/kept.a").mkdir()
    target = tmp_path / "Runtime"
    try:
        macos_runtime.copy_install(source, target, Path(sys.executable))
        assert not (target / "unused.a").exists()
        assert (target / "kept.a").is_dir()
        assert (
            target / "licenses/LICENSE.cpython.txt"
        ).read_bytes() == b"original notice"
    finally:
        # Permission-changing mutants must not strand these known private directories.
        for path in (target, target / "bin", target / "licenses", target / "kept.a"):
            if path.is_dir() and not path.is_symlink():
                path.chmod(0o755)


@pytest.mark.parametrize("explicit", [False, True])
def test_fresh_download_reference_and_incomplete_native_runtime_refusal(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, explicit: bool
) -> None:
    """Fresh acquisition retains notices but cannot approve a code-free runtime."""
    archive, pin = runtime_archive_fixture(tmp_path)
    content = archive.read_bytes()
    monkeypatch.delenv("EML_RUNTIME_SOURCE_DIRECTORY", raising=False)
    monkeypatch.setattr(macos_runtime_source, "pin", lambda _cpu: pin)
    monkeypatch.setattr(
        urllib.request, "urlopen", lambda *_args, **_kw: io.BytesIO(content)
    )
    with macos_runtime_archive.reference("arm64") as reference:
        assert (
            reference / "licenses/LICENSE.cpython.txt"
        ).read_bytes() == b"original notice"
    target = tmp_path / "Runtime"
    with pytest.raises(ValueError, match=r"^runtime has no native executable code$"):
        macos_runtime.prepare(target, "arm64", archive if explicit else None)
    assert not target.exists()
    assert not list(tmp_path.glob(".eml-runtime-build-*"))


def test_unavailable_metadata_stream_refuses_an_authenticated_archive(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An unexpected archive-reader response cannot become an accepted source tree."""
    archive, pin = runtime_archive_fixture(tmp_path)
    monkeypatch.setattr(tarfile.TarFile, "extractfile", lambda *_args: None)
    destination = tmp_path / "extracted"
    with pytest.raises(ValueError, match=r"^runtime metadata is unavailable$"):
        macos_runtime_source.extract(archive, destination, pin)
    assert not destination.exists()


def test_signature_tool_without_a_full_code_hash_cannot_approve_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A successful tool status alone does not prove native-code identity."""
    source = tmp_path / "program"
    source.write_bytes(b"public synthetic tool-failure fixture")
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda args, **_kw: subprocess.CompletedProcess(args, 0, "", ""),
    )
    with pytest.raises(
        ValueError, match=r"^runtime native code identity is unavailable$"
    ):
        macos_runtime_archive.__dict__["_code_hash"](source, tmp_path / "comparison")


def test_valid_pin_declares_the_expected_runtime_origin_and_cpu() -> None:
    """Read a real complete pin; malformed-source tests cover its rejection controls."""
    selected = macos_runtime_source.pin("arm64")
    with macos_runtime_source.CONFIG.open("rb") as stream:
        assert selected == tomllib.load(stream)["arm64"]
    assert selected["target"] == "aarch64-apple-darwin"
    assert selected["size"] > 0
    assert selected["url"].startswith(
        "https://github.com/astral-sh/python-build-standalone/releases/download/"
    )


def native_command_archive_fixture(
    tmp_path: Path,
    architecture: str,
) -> tuple[Path, macos_runtime_source.RuntimePin]:
    """Create a Mach-O-marked command fixture; its bytes are not executable code.

    Returns:
        The synthetic archive and independent source-integrity pin.

    """
    archive, selected = runtime_archive_fixture(tmp_path)
    native_archive = tmp_path / "native.tar.zst"
    selected["target"] = {
        "arm64": "aarch64-apple-darwin",
        "x86_64": "x86_64-apple-darwin",
    }[architecture]
    with (
        tarfile.open(archive, "r:zst") as source,
        tarfile.open(native_archive, "w:zst") as output,
    ):
        for member in source.getmembers():
            stream = source.extractfile(member)
            if member.name == "python/PYTHON.json":
                assert stream is not None
                metadata = json.loads(stream.read())
                stream.close()
                metadata["target_triple"] = selected["target"]
                content = json.dumps(metadata).encode()
                member.size = len(content)
                stream = io.BytesIO(content)
            if member.name == "python/install/bin/python3.14":
                assert stream is not None
                stream.close()
                content = b"\xcf\xfa\xed\xfe" + b"synthetic native command fixture"
                member.size = len(content)
                stream = io.BytesIO(content)
            output.addfile(member, stream)
            if stream is not None:
                stream.close()
        alias = tarfile.TarInfo("python/install/bin/python3")
        alias.type = tarfile.SYMTYPE
        alias.linkname = "python3.14"
        output.addfile(alias)
    selected["size"] = native_archive.stat().st_size
    selected["sha256"] = hashlib.sha256(native_archive.read_bytes()).hexdigest()
    return native_archive, selected


@pytest.mark.parametrize("architecture", ["arm64", "x86_64"])
def test_runtime_entrypoint_publishes_only_after_ordered_native_checks(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    architecture: str,
) -> None:
    """Portable command controls supplement the actual macOS signing integration."""
    native_archive, selected = native_command_archive_fixture(tmp_path, architecture)

    def selected_pin(cpu: str) -> macos_runtime_source.RuntimePin:
        assert cpu == architecture
        return selected

    monkeypatch.setattr(macos_runtime_source, "pin", selected_pin)
    commands: list[list[str]] = []

    def command(args: list[str], **kwargs: object) -> str:
        assert kwargs == {"text": True, "stderr": subprocess.STDOUT, "timeout": 30}
        commands.append(args)
        if args[0] == "/usr/bin/lipo":
            return architecture + "\n"
        if args[0] == "/usr/bin/otool":
            return "cmd LC_BUILD_VERSION\nplatform 1\nminos 14.0\n"
        assert args[0] == "/usr/bin/codesign"
        return ""

    monkeypatch.setattr(subprocess, "check_output", command)
    target = tmp_path / "missing" / "nested" / "prepared"
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "macos_runtime.py",
            "--target",
            str(target),
            "--architecture",
            architecture,
            "--archive",
            str(native_archive),
        ],
    )
    assert macos_runtime.main() == 0
    native = commands[0][2]
    assert Path(native).parts[-3:] == ("Runtime", "bin", "python3.14")
    assert commands == [
        ["/usr/bin/lipo", "-archs", native],
        ["/usr/bin/otool", "-l", native],
        ["/usr/bin/codesign", "--force", "--sign", "-", native],
        ["/usr/bin/codesign", "--verify", "--strict", native],
    ]
    assert (target / "bin/python3").readlink().as_posix() == "python3.14"
    assert (target / "licenses/LICENSE.cpython.txt").read_bytes() == b"original notice"
    assert json.loads(capsys.readouterr().out) == {
        "runtime": str(target),
        "architecture": architecture,
    }
    monkeypatch.setattr(sys, "argv", ["macos_runtime.py", "--help"])
    with pytest.raises(SystemExit) as help_result:
        macos_runtime.main()
    assert help_result.value.code == 0
    assert (
        "Prepare a relocated, notice-complete and ad-hoc-signed "
        "private CPython runtime." in " ".join(capsys.readouterr().out.split())
    )
    with pytest.raises(SystemExit) as completed:
        runpy.run_path(str(Path(macos_runtime.__file__)), run_name="__main__")
    assert completed.value.code == 0


def test_native_code_comparison_requires_matching_full_hashes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Full code hashes must agree; actual signatures are separately tested on Mac."""
    expected = tmp_path / "expected"
    expected.mkdir()
    program = expected / "program"
    program.write_bytes(b"\xcf\xfa\xed\xfe" + b"native identity fixture")
    actual = tmp_path / "actual"
    shutil.copytree(expected, actual)
    monkeypatch.setattr(
        macos_runtime, "require_native", lambda root, _cpu: [root / "program"]
    )

    def signature(
        args: list[str], **kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        destination = args[-1]
        if "--force" in args:
            assert args == [
                "/usr/bin/codesign",
                "--force",
                "--sign",
                "-",
                "--identifier",
                "io.github.resoltico.eml.runtime.verification",
                destination,
            ]
            assert kwargs == {"check": True, "capture_output": True, "timeout": 30}
        else:
            assert args == ["/usr/bin/codesign", "-dv", "--verbose=4", destination]
            assert kwargs == {
                "check": True,
                "capture_output": True,
                "text": True,
                "timeout": 30,
            }
        digest = hashlib.sha256(Path(args[-1]).read_bytes()).hexdigest()
        return subprocess.CompletedProcess(
            args, 0, "", "CandidateCDHashFull sha256=" + digest
        )

    monkeypatch.setattr(subprocess, "run", signature)
    macos_runtime_archive.verify(actual, expected, "arm64")
    (actual / "program").write_bytes(program.read_bytes() + b"changed")
    with pytest.raises(ValueError, match="native code differs"):
        macos_runtime_archive.verify(actual, expected, "arm64")


@pytest.mark.parametrize(
    ("field", "value"),
    [("url", 1), ("sha256", False), ("target", 1), ("size", True), ("size", 0)],
)
def test_complete_runtime_pin_refuses_wrong_field_types_and_nonpositive_size(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, field: str, value: object
) -> None:
    """A complete key set cannot hide a malformed value behind the missing-key check."""
    values: dict[str, object] = {
        "url": "https://github.com/astral-sh/python-build-standalone/releases/download/x/y",
        "sha256": "public-synthetic-digest",
        "target": "public-synthetic-target",
        "size": 1,
    }
    values[field] = value
    configuration = tmp_path / "runtime-source.toml"
    configuration.write_text(
        "[arm64]\n"
        + "\n".join(f"{key}={json.dumps(item)}" for key, item in values.items())
    )
    monkeypatch.setattr(macos_runtime_source, "CONFIG", configuration)
    with pytest.raises(ValueError, match=r"^invalid runtime archive pin$"):
        macos_runtime_source.pin("arm64")
