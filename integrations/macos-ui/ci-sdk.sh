#!/bin/sh
# Select and verify the delivery SDK on the pinned hosted runner.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
python -B - "$SCRIPT_DIR/toolchain.toml" <<'PYTHON'
import os, subprocess, sys, tomllib
from pathlib import Path
pin = tomllib.loads(Path(sys.argv[1]).read_text())['delivery']
developer = f"/Applications/Xcode_{pin['xcode-version']}.app/Contents/Developer"
result = subprocess.check_output(
    ['/usr/bin/xcodebuild', '-version'],
    env={**os.environ, 'DEVELOPER_DIR': developer}, text=True,
)
expected = f"Xcode {pin['xcode-version']}\nBuild version {pin['xcode-build']}\n"
if result != expected:
    raise RuntimeError('Delivery Xcode differs from toolchain.toml; review the hosted image and SDK pin')
print('DEVELOPER_DIR=' + developer)
PYTHON
