"""One native stop request reaches the processor once, through its launcher."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from tests.test_processing_launcher import _complete

ROOT = Path(__file__).resolve().parents[1]
DRIVER = """
import Foundation
@MainActor final class Outcome { var completed = false; var admitted = false }
@main struct Probe {
  @MainActor static func main() throws {
    let run = ProcessingRun()
    let outcome = Outcome()
    try run.start(paths: ["public.eml"], version: "4.0.0", preparing: {},
      completed: { result, _, status, _ in
        outcome.completed = true
        if case .success = result { outcome.admitted = status == 130 }
      })
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) { run.interrupt() }
    let deadline = Date().addingTimeInterval(8)
    while !outcome.completed && Date() < deadline {
      RunLoop.main.run(until: Date().addingTimeInterval(0.01))
    }
    precondition(outcome.admitted, "A single stop must retain an admitted report")
  }
}
"""
PROCESSOR = """
import json, os, signal, sys, time
from pathlib import Path
marker = Path(os.environ['PROBE_SIGNALS'])
count = 0
def interrupted(*args):
    global count
    count += 1
    marker.write_text(str(count))
    if count > 1:
        os._exit(130)
    time.sleep(0.2)
    print(os.environ['PROBE_REPORT'], flush=True)
    raise SystemExit(130)
signal.signal(signal.SIGINT, interrupted)
while True:
    time.sleep(0.05)
"""


@pytest.mark.skipif(sys.platform != "darwin", reason="native macOS process controller")
def test_native_stop_is_not_broadcast_then_forwarded_again(tmp_path: Path) -> None:
    app = tmp_path / "SignalProbe.app"
    resources = app / "Contents/Resources"
    executable = app / "Contents/MacOS/SignalProbe"
    resources.mkdir(parents=True)
    executable.parent.mkdir()
    (resources / "processing-launcher.sh").write_bytes(
        (ROOT / "integrations/macos-ui/processing-launcher.sh").read_bytes()
    )
    (resources / "remove-eml-attachments.pyz").write_text(PROCESSOR)
    driver = tmp_path / "Probe.swift"
    driver.write_text(DRIVER)
    sources = ROOT / "integrations/macos-ui"
    subprocess.run(
        [
            "/bin/sh",
            str(sources / "swiftc.sh"),
            "-swift-version",
            "6",
            "-warnings-as-errors",
            str(sources / "App/ProcessingRun.swift"),
            str(sources / "App/RuntimeConfiguration.swift"),
            str(sources / "App/UITrace.swift"),
            str(sources / "ReportModel.swift"),
            str(driver),
            "-o",
            str(executable),
        ],
        check=True,
        capture_output=True,
        timeout=60,
    )
    report = _complete(0)
    report["version"] = "4.0.0"
    marker = tmp_path / "signals"
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.startswith("COVERAGE_")
    }
    environment.update(
        EML_REMOVER_PYTHON=sys.executable,
        PROBE_REPORT=json.dumps(report),
        PROBE_SIGNALS=str(marker),
    )
    result = subprocess.run(
        [str(executable)], env=environment, capture_output=True, check=False, timeout=15
    )
    assert result.returncode == 0, result.stderr
    assert marker.read_text() == "1"
