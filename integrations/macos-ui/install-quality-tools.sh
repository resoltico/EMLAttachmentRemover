#!/bin/sh
# Development tools only: never copied into the distributed application.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python}
TOOLS=${EML_SWIFT_TOOLS:-"$HOME/Library/Application Support/EML Attachment Remover Toolchains"}
/bin/sh "$SCRIPT_DIR/install-swift-toolchain.sh"
mkdir -p "$TOOLS"
"$PYTHON" -B - "$SCRIPT_DIR/toolchain.toml" "$TOOLS" <<'PY'
import tomllib, os, shutil, subprocess, sys, tempfile, urllib.request, zipfile
from pathlib import Path
sys.path.insert(0,str(Path(sys.argv[1]).resolve().parents[2]))
from tools.pinned_download import copy_archive
pins=tomllib.loads(Path(sys.argv[1]).read_text())
root=Path(sys.argv[2])
for tool in ('swiftlint', 'swift-format'):
    pin=pins[tool]
    destination=root/(tool+'-'+pin['version'])
    binary=destination/tool
    if binary.is_file():
        version=subprocess.check_output([str(binary), 'version' if tool=='swiftlint' else '--version'], text=True).strip()
        if version != pin['version']: raise RuntimeError('Installed Swift tool version differs from pin')
        continue
    with tempfile.TemporaryDirectory(prefix='.swift-tools-', dir=root) as scratch:
        scratch=Path(scratch)
        prepared=scratch/'prepared'
        prepared.mkdir()
        if tool=='swiftlint':
            archive=scratch/'release.zip'
            with urllib.request.urlopen(pin['url'], timeout=60) as source:
                copy_archive(source,archive,size=pin['size'],sha256=pin['sha256'])
            # Copy the single required executable; no archive paths are extracted.
            with zipfile.ZipFile(archive) as zipped:
                (prepared/tool).write_bytes(zipped.read(tool))
            (prepared/tool).chmod(0o755)
        else:
            source=scratch/'source'
            subprocess.run(['git','init','--quiet',str(source)],check=True)
            subprocess.run(['git','-C',str(source),'fetch','--quiet','--depth=1',pin['repository'],pin['revision']],check=True)
            subprocess.run(['git','-C',str(source),'checkout','--quiet','--detach','FETCH_HEAD'],check=True)
            actual=subprocess.check_output(['git','-C',str(source),'rev-parse','HEAD'],text=True).strip()
            if actual!=pin['revision']:raise RuntimeError('swift-format source revision differs')
            toolchain=Path(subprocess.check_output(['/bin/sh',str(Path(sys.argv[1]).parent/'swiftc.sh'),'--toolchain-directory'],text=True).strip())
            environment={**os.environ,'SDKROOT':subprocess.check_output(['/usr/bin/xcrun','--show-sdk-path'],text=True).strip()}
            subprocess.run([str(toolchain/'usr/bin/swift'),'build','--package-path',str(source),'--scratch-path',str(scratch/'build'),'--cache-path',str(scratch/'cache'),'--disable-dependency-cache','--manifest-cache','local','-c','release','--disable-index-store','--product','swift-format'],env=environment,check=True)
            shutil.copy2(scratch/'build/release/swift-format',prepared/tool)
        version=subprocess.check_output([str(prepared/tool),'version' if tool=='swiftlint' else '--version'],text=True).strip()
        if version!=pin['version']:raise RuntimeError('Built Swift tool version differs from pin')
        os.rename(prepared,destination)
print(root)
PY
