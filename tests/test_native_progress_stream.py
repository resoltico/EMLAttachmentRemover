"""Require live native progress from a pipe before its writer closes."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

UI = Path(__file__).resolve().parents[1] / "integrations/macos-ui"
PROBE = r"""import Foundation

@main struct ProgressPipeChecks {
  static func main() throws {
    let pipe = Pipe()
    let reader = pipe.fileHandleForReading
    let delivered = DispatchSemaphore(value: 0)
    let finished = DispatchSemaphore(value: 0)
    let diagnostic = Data("A diagnostic with Unicode: €\n".utf8)
    DispatchQueue.global().async {
      let captured = ProgressStream.read(reader) { update in
        if update.completed == 1 && update.total == 2 && update.stage == .processing {
          delivered.signal()
        }
      }
      if captured == diagnostic { finished.signal() }
    }
    let record = Data(
      ("EML_PROGRESS {\"schema\":1,\"stage\":\"processing\","
        + "\"completed\":1,\"total\":2}\n").utf8)
    try pipe.fileHandleForWriting.write(contentsOf: record.prefix(13))
    try pipe.fileHandleForWriting.write(contentsOf: record.dropFirst(13))
    let live = delivered.wait(timeout: .now() + 1) == .success
    try pipe.fileHandleForWriting.write(contentsOf: diagnostic)
    try pipe.fileHandleForWriting.close()
    let ended = finished.wait(timeout: .now() + 1) == .success
    try reader.close()
    guard live && ended else {
      print("Live progress: \(live); complete diagnostic after EOF: \(ended)")
      exit(1)
    }

    let unreadable = Pipe()
    let failure = ProgressStream.read(unreadable.fileHandleForWriting) { _ in exit(2) }
    try unreadable.fileHandleForReading.close()
    try unreadable.fileHandleForWriting.close()
    let expected = Data("The progress/diagnostic channel could not be read.\n".utf8)
    guard failure == expected else {
      print("Read error lost its bounded diagnostic")
      exit(3)
    }
    print("Live pipe progress and read-error checks passed.")
  }
}
"""


@pytest.mark.skipif(sys.platform != "darwin", reason="macOS native progress")
def test_native_progress_arrives_before_pipe_eof(tmp_path: Path) -> None:
    source = tmp_path / "ProgressPipeChecks.swift"
    source.write_text(PROBE)
    binary = tmp_path / "progress-pipe-checks"
    scratch = tmp_path / "compiler-scratch"
    scratch.mkdir()
    environment = {
        **os.environ,
        "EML_REMOVER_PYTHON": sys.executable,
        "PYTHONDONTWRITEBYTECODE": "1",
        "TMPDIR": str(scratch),
        "TMP": str(scratch),
        "TEMP": str(scratch),
        "CLANG_MODULE_CACHE_PATH": str(scratch / "clang-modules"),
    }
    try:
        subprocess.run(
            [
                "/bin/sh",
                str(UI / "swiftc.sh"),
                "-swift-version",
                "6",
                "-warnings-as-errors",
                "-O",
                "-parse-as-library",
                "-module-cache-path",
                str(scratch / "swift-modules"),
                str(UI / "App/ProgressStream.swift"),
                str(UI / "ReportModel.swift"),
                str(UI / "LauncherFailure.swift"),
                str(source),
                "-o",
                str(binary),
            ],
            env=environment,
            check=True,
            capture_output=True,
            text=True,
            timeout=120,
        )
        result = subprocess.run(
            [str(binary)],
            env=environment,
            capture_output=True,
            text=True,
            check=True,
            timeout=5,
        )
        assert (
            result.stdout.strip() == "Live pipe progress and read-error checks passed."
        )
    finally:
        shutil.rmtree(scratch)
