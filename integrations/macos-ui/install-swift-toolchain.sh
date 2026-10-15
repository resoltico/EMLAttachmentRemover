#!/bin/sh
# Install the pinned development-only compiler beside Apple's toolchain.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
"$PYTHON" -B - "$SCRIPT_DIR/toolchain.toml" <<'PY'
import subprocess, sys, tempfile, tomllib, urllib.request
from pathlib import Path
sys.path.insert(0,str(Path(sys.argv[1]).resolve().parents[2]))
from tools.pinned_download import copy_archive
pin=tomllib.loads(Path(sys.argv[1]).read_text())['swift']
destination=Path.home()/'Library/Developer/Toolchains'/('swift-'+pin['version']+'-RELEASE.xctoolchain')
compiler=destination/'usr/bin/swiftc'
if not compiler.is_file():
    with tempfile.TemporaryDirectory(prefix='swift-toolchain-install-') as temporary:
        package=Path(temporary)/'toolchain.pkg'
        with urllib.request.urlopen(pin['url'], timeout=60) as source:
            copy_archive(source,package,size=pin['size'],sha256=pin['sha256'])
        signature=subprocess.check_output(['/usr/sbin/pkgutil','--check-signature',str(package)],text=True)
        if pin['signer'] not in signature:
            raise RuntimeError('Swift toolchain installer signer differs')
        subprocess.run(['/usr/sbin/installer','-target','CurrentUserHomeDirectory','-pkg',str(package)],check=True)
subprocess.run(['/bin/sh',str(Path(sys.argv[1]).parent/'swiftc.sh'),'--version'],check=True)
print(compiler)
PY
