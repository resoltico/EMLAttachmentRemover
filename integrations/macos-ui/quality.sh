#!/bin/sh
# One local/CI Swift gate; pinned tools, explicit source discovery, no lint cache.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PROJECT_ROOT=$(CDPATH='' cd -P "$SCRIPT_DIR/../.." && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python}
TOOLS=${EML_SWIFT_TOOLS:-"$HOME/Library/Application Support/EML Attachment Remover Toolchains"}
"$PYTHON" -B - "$SCRIPT_DIR" "$TOOLS" <<'PY'
import json, tomllib, os, subprocess, sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]).resolve().parents[1]))
from tools.lint_exceptions import swift_findings
root=Path(sys.argv[1]);tools=Path(sys.argv[2]);pins=tomllib.loads((root/'toolchain.toml').read_text())
paths=sorted(str(path) for path in root.rglob('*.swift'))
if not paths:raise RuntimeError('No Swift sources discovered')
for name in ('swiftlint','swift-format'):
    pin=pins[name]
    binary=tools/(name+'-'+pin['version'])/name
    if not binary.is_file():raise RuntimeError('Install the pinned Swift tools with integrations/macos-ui/install-quality-tools.sh')
    actual=subprocess.check_output([str(binary),'version' if name=='swiftlint' else '--version'],text=True).strip()
    if actual!=pin['version']:raise RuntimeError('Swift tool version differs from pin')
formatter=tools/('swift-format-'+pins['swift-format']['version'])/'swift-format'
linter=tools/('swiftlint-'+pins['swiftlint']['version'])/'swiftlint'
subprocess.run([str(formatter),'lint','--strict','--configuration',str(root/'swift-format.json'),*paths],check=True)
environment=dict(os.environ)
toolchain=subprocess.check_output(['/bin/sh',str(root/'swiftc.sh'),'--toolchain-directory'],text=True).strip()
environment['TOOLCHAIN_DIR']=toolchain
result=subprocess.run([str(linter),'lint','--strict','--no-cache','--reporter','json','--config',str(root/'swiftlint.yml'),*paths],env=environment,capture_output=True,text=True)
if result.returncode not in (0,2):
    sys.stderr.write(result.stderr); raise SystemExit(result.returncode)
errors=swift_findings(json.loads(result.stdout))
if errors:
    sys.stderr.write('\n'.join(errors)+'\n');raise SystemExit(1)
PY
"$PYTHON" -B "$PROJECT_ROOT/tools/lint_exceptions.py"
