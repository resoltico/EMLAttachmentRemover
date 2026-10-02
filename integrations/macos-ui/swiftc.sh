#!/bin/sh
# Use one pinned compiler for production, tests, and fuzz instrumentation.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
"$PYTHON" -B - "$SCRIPT_DIR/toolchain.toml" "$@" <<'PY'
import os, subprocess, sys, tomllib
from pathlib import Path
pin=tomllib.loads(Path(sys.argv[1]).read_text())['swift']
root=Path.home()/'Library/Developer/Toolchains'/('swift-'+pin['version']+'-RELEASE.xctoolchain')
compiler=root/'usr/bin/swiftc'
if not compiler.is_file():raise RuntimeError('Run integrations/macos-ui/install-swift-toolchain.sh first')
version=subprocess.check_output([str(compiler),'--version'],text=True)
if pin['compiler_tag'] not in version:raise RuntimeError('Swift compiler version differs from pin')
if sys.argv[2:]==['--toolchain-directory']:
    print(root)
else:
    sdk=subprocess.check_output(['/usr/bin/xcrun','--show-sdk-path'],text=True).strip()
    os.execv(compiler,[str(compiler),'-sdk',sdk,*sys.argv[2:]])
PY
