"""Exercise real optimized fuzz assertions and the campaign failure boundaries."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Final

import pytest

ROOT: Final = Path(__file__).resolve().parents[1]
UI: Final = ROOT / "integrations/macos-ui"
MACOS: Final = pytest.mark.skipif(
    sys.platform != "darwin", reason="macOS Swift fuzzing"
)


def _environment() -> dict[str, str]:
    return {
        **{
            name: value
            for name, value in os.environ.items()
            if not name.startswith("COVERAGE_")
        },
        "EML_REMOVER_PYTHON": sys.executable,
        "PYTHONDONTWRITEBYTECODE": "1",
    }


def _run(ui: Path, output: Path, *options: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["/bin/sh", str(ui / "fuzz.sh"), str(output), *options],
        env=_environment(),
        capture_output=True,
        text=True,
        check=False,
        timeout=45,
    )


@pytest.fixture
def copied_ui(tmp_path: Path) -> Path:
    """Isolate deliberate failures without changing installed tools.

    Returns:
        The copied integration path.

    """
    project = tmp_path / "project"
    ui = project / "integrations/macos-ui"
    shutil.copytree(UI, ui)
    tools = project / "tools"
    tools.mkdir()
    for name in ("__init__.py", "task_process.py"):
        shutil.copyfile(ROOT / "tools" / name, tools / name)
    return ui


@pytest.mark.parametrize(
    "options",
    [("-artifact_prefix=/tmp/",), ("-runs=0",), ("-max_total_time=0",), ("-jobs=2",)],
)
@MACOS
def test_campaign_rejects_options_that_disable_bounds_or_redirect_evidence(
    tmp_path: Path, options: tuple[str, ...]
) -> None:
    output = tmp_path / "campaign"
    result = _run(UI, output, *options)
    assert result.returncode != 0
    assert not output.exists()


@pytest.mark.parametrize("symlink", [False, True])
@MACOS
def test_existing_campaign_evidence_is_never_overwritten(
    tmp_path: Path, *, symlink: bool
) -> None:
    prior = tmp_path / "prior"
    prior.mkdir()
    evidence = prior / "invocation.json"
    evidence.write_text("Prior evidence.\n")
    output = tmp_path / "campaign"
    if symlink:
        output.symlink_to(prior, target_is_directory=True)
    else:
        output = prior
    result = _run(UI, output, "-runs=1")
    assert result.returncode != 0
    assert evidence.read_text() == "Prior evidence.\n"
    assert sorted(path.name for path in prior.iterdir()) == ["invocation.json"]


@MACOS
def test_compile_failure_preserves_inputs_command_and_log(
    copied_ui: Path, tmp_path: Path
) -> None:
    compiler = copied_ui / "swiftc.sh"
    compiler.write_text(
        '#!/bin/sh\ncase "$1" in\n--version) echo fixture;;\n'
        "--toolchain-directory) echo /fixture;;\n"
        "*) echo deliberate-compile-failure; exit 7;;\nesac\n"
    )
    output = tmp_path / "campaign"
    result = _run(copied_ui, output)
    assert result.returncode != 0
    metadata = json.loads((output / "invocation.json").read_text())
    assert (metadata["stage"], metadata["outcome"]) == ("compile", "failed")
    assert "deliberate-compile-failure" in (output / "compile.log").read_text()
    assert (output / "sources/ReportModel.swift").read_bytes() == (
        UI / "ReportModel.swift"
    ).read_bytes()
    assert metadata["inputs"]
    assert len(metadata["stages"]) == 1


@MACOS
def test_sigterm_cleans_the_active_compile_group(
    copied_ui: Path, tmp_path: Path
) -> None:
    compiler = copied_ui / "swiftc.sh"
    compiler.write_text(
        '#!/bin/sh\ncase "$1" in\n--version) echo fixture;;\n'
        "--toolchain-directory) echo /fixture;;\n"
        '*) trap "" INT; sleep 30 & echo $!; wait;;\nesac\n'
    )
    output = tmp_path / "campaign"
    with subprocess.Popen(
        ["/bin/sh", str(copied_ui / "fuzz.sh"), str(output)],
        env=_environment(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as process:
        log = output / "compile.log"
        deadline = time.monotonic() + 15
        while not log.is_file() or not log.read_text().strip():
            assert time.monotonic() < deadline, "Compiler never started"
            time.sleep(0.05)
        child = log.read_text().strip()
        process.send_signal(signal.SIGTERM)
        process.communicate(timeout=15)
        assert process.returncode != 0
    state = subprocess.run(
        ["/bin/ps", "-p", child, "-o", "stat="],
        capture_output=True,
        text=True,
        check=False,
        timeout=5,
    )
    assert not state.stdout.strip() or state.stdout.strip().startswith("Z")
    metadata = json.loads((output / "invocation.json").read_text())
    assert metadata["outcome"] == "failed"
    assert "KeyboardInterrupt" in metadata["error"]


@pytest.fixture(scope="module")
def campaign(tmp_path_factory: pytest.TempPathFactory) -> Path:
    runner = os.environ.get("RUNNER_TEMP")
    if runner is None:
        parent = tmp_path_factory.mktemp("swift-fuzz")
    else:
        evidence = Path(runner) / "native-fuzz-tests"
        evidence.mkdir(mode=0o700, parents=True, exist_ok=True)
        parent = Path(tempfile.mkdtemp(prefix="receipt-", dir=evidence))
    output = parent / "campaign"
    result = _run(UI, output, "-runs=100", "-max_total_time=5")
    assert result.returncode == 0, result.stdout + result.stderr
    return output


@MACOS
def test_optimized_asan_campaign_replays_seeds_and_records_all_stages(
    campaign: Path,
) -> None:
    metadata = json.loads((campaign / "invocation.json").read_text())
    assert metadata["outcome"] == "passed"
    assert [stage["stage"] for stage in metadata["stages"]] == [
        "compile",
        "replay",
        "campaign",
    ]
    assert "-sanitize=fuzzer,address" in metadata["stages"][0]["command"]
    log = (campaign / "campaign.log").read_text()
    # libFuzzer can finish one execution beyond the requested run boundary.
    assert "Done 100 runs" in log or "Done 101 runs" in log
    assert "Executed" in (campaign / "replay.log").read_text()
    assert (campaign.stat().st_mode & 0o077) == 0


@pytest.mark.parametrize(
    ("before", "after", "input_bytes"),
    [
        ('var result = ""', "return text", b"\x1b"),
        (
            "receipt.processStatus == Int(status), receipt.report.version == version",
            "true",
            b"",
        ),
        (
            (
                'guard ["created", "existing_verified"].contains(status), '
                "publication?.addressVerified == true"
            ),
            "guard true",
            b"\x02\x01",
        ),
    ],
)
@MACOS
def test_optimized_harness_detects_broken_model_contracts(
    campaign: Path, tmp_path: Path, before: str, after: str, input_bytes: bytes
) -> None:
    model = tmp_path / "ReportModel.swift"
    source = (UI / "ReportModel.swift").read_text()
    assert before in source
    # Replace the entire sanitizer body to avoid an unreachable-code warning.
    if before == 'var result = ""':
        start = source.index('  var result = ""')
        end = source.index("\n}", start)
        source = source[:start] + "  return text" + source[end:]
    else:
        source = source.replace(before, after)
    model.write_text(source)
    binary = tmp_path / "broken-fuzzer"
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-g",
            "-O",
            "-parse-as-library",
            "-sanitize=fuzzer,address",
            str(model),
            str(UI / "LauncherFailure.swift"),
            str(UI / "App/ProgressStream.swift"),
            str(UI / "Fuzz/ReceiptFuzzer.swift"),
            "-o",
            str(binary),
        ],
        check=True,
        capture_output=True,
        timeout=45,
    )
    seed = tmp_path / "input"
    if not input_bytes:
        seed.write_bytes((campaign / "seeds/version-mismatch.json").read_bytes())
    else:
        seed.write_bytes(input_bytes)
    corpus = tmp_path / "corpus"
    corpus.mkdir()
    shutil.copyfile(seed, corpus / "input")
    result = subprocess.run(
        [
            str(binary),
            "-runs=1",
            "-artifact_prefix=" + str(tmp_path) + "/",
            str(corpus),
        ],
        env=_environment(),
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    assert result.returncode != 0
    assert "ERROR: libFuzzer" in result.stderr
    assert list(tmp_path.glob("crash-*"))


@MACOS
def test_replay_failure_keeps_the_exact_seed_and_stage(
    copied_ui: Path, tmp_path: Path
) -> None:
    (copied_ui / "swiftc.sh").write_text(
        '#!/bin/sh\ncase "$1" in\n--version) echo fixture;;\n'
        "--toolchain-directory) echo /fixture;;\n*)\n"
        'for argument in "$@"; do destination=$argument; done\n'
        "printf '#!/bin/sh\\necho deliberate-replay-failure\\nexit 7\\n' "
        '> "$destination"\n'
        'chmod 700 "$destination";;\nesac\n'
    )
    output = tmp_path / "campaign"
    result = _run(copied_ui, output)
    assert result.returncode != 0
    metadata = json.loads((output / "invocation.json").read_text())
    assert (metadata["stage"], metadata["outcome"]) == ("replay", "failed")
    assert "deliberate-replay-failure" in (output / "replay.log").read_text()
    assert (output / "seeds/version-mismatch.json").read_bytes() == (
        UI / "Fuzz/corpus/version-mismatch.json"
    ).read_bytes()
    assert not (output / "campaign.log").exists()


@MACOS
def test_compile_deadline_preserves_failure_and_stops_the_child(
    copied_ui: Path, tmp_path: Path
) -> None:
    configuration = copied_ui / "fuzzing.toml"
    configuration.write_text(
        configuration.read_text().replace(
            "compile_seconds = 300", "compile_seconds = 1"
        )
    )
    (copied_ui / "swiftc.sh").write_text(
        '#!/bin/sh\ncase "$1" in\n--version) echo fixture;;\n'
        "--toolchain-directory) echo /fixture;;\n"
        '*) trap "" INT; sleep 30 & echo $!; wait;;\nesac\n'
    )
    output = tmp_path / "campaign"
    result = _run(copied_ui, output)
    assert result.returncode != 0
    metadata = json.loads((output / "invocation.json").read_text())
    assert metadata["outcome"] == "failed"
    assert "TimeoutExpired" in metadata["error"]
    child = (output / "compile.log").read_text().strip()
    state = subprocess.run(
        ["/bin/ps", "-p", child, "-o", "stat="],
        capture_output=True,
        text=True,
        check=False,
        timeout=5,
    )
    assert not state.stdout.strip() or state.stdout.strip().startswith("Z")


@pytest.mark.parametrize("length", [0, 65536])
@MACOS
def test_empty_and_full_size_inputs_reach_the_optimized_harness(
    campaign: Path, tmp_path: Path, length: int
) -> None:
    seed = tmp_path / "boundary-input"
    seed.write_bytes(b"A" * max(0, length - 1) + (b"\0" if length else b""))
    result = subprocess.run(
        [str(campaign / "receipt-fuzzer"), str(seed)],
        env=_environment(),
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    assert result.returncode == 0, result.stderr
    assert "Executed" in result.stderr


@MACOS
def test_asan_detects_a_real_heap_overflow(tmp_path: Path) -> None:
    source = tmp_path / "MemoryFailureControl.swift"
    source.write_text(
        "import Foundation\n"
        '@_cdecl("LLVMFuzzerTestOneInput")\n'
        "public func fuzzMemory(_ bytes: UnsafePointer<UInt8>,\n"
        "  _ count: Int) -> Int32 {\n"
        "  let pointer = UnsafeMutablePointer<CChar>.allocate(capacity: 1)\n"
        '  strcpy(pointer, "Memory sanitizer failure control")\n'
        "  puts(pointer)\n  pointer.deallocate()\n  return 0\n}\n"
    )
    binary = tmp_path / "memory-fuzzer"
    subprocess.run(
        [
            "/bin/sh",
            str(UI / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-g",
            "-O",
            "-parse-as-library",
            "-sanitize=fuzzer,address",
            str(source),
            "-o",
            str(binary),
        ],
        env=_environment(),
        check=True,
        capture_output=True,
        timeout=45,
    )
    result = subprocess.run(
        [str(binary), "-runs=1", "-artifact_prefix=" + str(tmp_path) + "/"],
        env=_environment(),
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    assert result.returncode != 0
    assert "ERROR: AddressSanitizer: heap-buffer-overflow" in result.stderr


@MACOS
def test_invalid_utf8_json_key_is_rejected_by_optimized_model(
    campaign: Path, tmp_path: Path
) -> None:
    seed = (UI / "Fuzz/corpus/display-controls.json").read_bytes()
    needle = b"address_verified"
    positions = [index for index in range(len(seed)) if seed.startswith(needle, index)]
    assert len(positions) == 7
    data = bytearray(seed)
    data[positions[5] : positions[5] + 4] = bytes((0xA0, 0x9B, 0x9B, 0x8D))
    source = tmp_path / "invalid-utf8-key"
    source.write_bytes(data)
    result = subprocess.run(
        [str(campaign / "receipt-fuzzer"), str(source)],
        env=_environment(),
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    assert result.returncode == 0, result.stderr
    assert "Executed" in result.stderr
