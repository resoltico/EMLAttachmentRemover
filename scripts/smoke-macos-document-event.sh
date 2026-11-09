#!/usr/bin/env bash
# Manual/floor probe of our native document adapter, outside the merge gate.
# Requires macOS developer tools and Python 3. The exact archive is tested first
# by smoke-macos-app.sh; this probe re-signs an isolated copy with a native
# recording runner that calls real core in process and refuses writes.
# It does not automate OS UI.
# Usage: smoke-macos-document-event.sh APP_BUNDLE FIXTURE.eml PROBE_BINARY [--omit-delivery]
# --omit-delivery is a negative control: the probe must fail, not report success.
set -euo pipefail
python3 - "$@" <<'PY'
import argparse
import os
from pathlib import Path
import plistlib
import select
import signal
import subprocess
import sys
import tempfile
import uuid

parser = argparse.ArgumentParser()
parser.add_argument("app", type=Path)
parser.add_argument("fixture", type=Path)
parser.add_argument("probe", type=Path)
parser.add_argument("--omit-delivery", action="store_true")
args = parser.parse_args()
if sys.platform != "darwin":
    parser.error("native document-event probe requires macOS")
expected = subprocess.check_output([str(args.app.resolve() / "Contents/MacOS/EMLAttachmentRemover"), "--version"])
actual = subprocess.check_output([str(args.probe.resolve()), "--version"])
assert actual == expected, "Native probe and GUI source identities differ"
original = args.fixture.read_bytes()
with tempfile.TemporaryDirectory(prefix="eml-document-event-") as temporary:
    root = Path(temporary).resolve()
    app = root / "Document Probe.app"
    subprocess.run(["ditto", str(args.app.resolve()), str(app)], check=True)
    plist = app / "Contents/Info.plist"
    info = plistlib.loads(plist.read_bytes())
    # Keep registration and native delivery isolated from installed app copies.
    info["CFBundleIdentifier"] += ".documentprobe." + uuid.uuid4().hex
    plist.write_bytes(plistlib.dumps(info))
    executable = app / "Contents/MacOS" / info["CFBundleExecutable"]
    import shutil
    shutil.copyfile(args.probe.resolve(), executable)
    executable.chmod(0o755)
    request_file = root / "request.paths"
    owner_file = root / "owner.pid"
    arrival = root / "arrival"
    os.mkfifo(arrival)
    # RDWR opens our notification pipe without a reader/writer-open race.
    descriptor = os.open(arrival, os.O_RDWR | os.O_NONBLOCK)
    subprocess.run(["codesign", "--force", "--deep", "-s", "-", str(app)], check=True)
    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
    warmup, delivered = root / "Warmup.eml", root / "Delivered.eml"
    for path in (warmup, delivered):
        path.write_bytes(original)

    def observe(expected):
        # A finite deadline on one owned notification, not a delivery retry.
        readable, _, _ = select.select([descriptor], [], [], 15)
        if not readable:
            raise AssertionError("No core request followed the document operation")
        assert os.read(descriptor, 1) == b"1"
        assert (root / "mode").read_bytes() == b"dry-run"
        observed = [path for path in request_file.read_bytes().split(b"\0") if path]
        assert sorted(observed) == sorted(os.fsencode(path) for path in expected), observed
        return int(owner_file.read_text())

    with (root / "app.stdout").open("wb") as stdout, (root / "app.stderr").open("wb") as stderr:
        # Launch through the same service as users. A core request can precede
        # NSApplication's first event-loop turn; it does not prove registration
        # of a process started by directly executing the bundle's binary.
        process = subprocess.Popen([
            "open", "-n", "-W", "-a", str(app),
            "--stdout", str(root / "app.stdout"),
            "--stderr", str(root / "app.stderr"), "--args", str(warmup)
        ], stdout=stdout, stderr=stderr)
        try:
            # LaunchServices owns registration; this check proves that our
            # adapter installed and records the original GUI's process identity.
            owner = observe([warmup])
            if not args.omit_delivery:
                subprocess.run(["open", "-a", str(app), str(delivered)], check=True)
            assert observe([warmup, delivered]) == owner, "A second GUI received the document"
            assert process.poll() is None, "The original GUI must own both requests"
            for path in (warmup, delivered):
                assert path.read_bytes() == original, "Source bytes changed"
                assert not path.with_name(path.stem + ".no-attachments.eml").exists()
            print("native document event ok:", subprocess.check_output(["sw_vers", "-productVersion"], text=True).strip(), os.uname().machine)
        except BaseException:
            stderr.flush()
            print((root / "app.stderr").read_text(errors="replace"), file=sys.stderr)
            raise
        finally:
            # The open -W child waits for the application; terminate only our
            # uniquely named probe executable, then join its owned launcher.
            listing = subprocess.check_output(["ps", "-axo", "pid=,args="], text=True)
            for line in listing.splitlines():
                columns = line.strip().split(None, 1)
                if len(columns) == 2 and (
                    columns[1] == str(executable) or columns[1].startswith(str(executable) + " ")
                ):
                    try:
                        os.kill(int(columns[0]), signal.SIGTERM)
                    except ProcessLookupError:
                        pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            os.close(descriptor)
            subprocess.run([
                "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
                "-u", str(app)
            ], check=True)
PY
