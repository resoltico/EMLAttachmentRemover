#!/bin/sh
# Install the pinned development-only compiler beside Apple's toolchain.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
"$PYTHON" -B - "$SCRIPT_DIR/toolchain.toml" <<'PY'
import hashlib, shutil, subprocess, sys, tempfile, tomllib, urllib.request
from pathlib import Path
pin=tomllib.loads(Path(sys.argv[1]).read_text())['swift']
destination=Path.home()/'Library/Developer/Toolchains'/('swift-'+pin['version']+'-RELEASE.xctoolchain')
compiler=destination/'usr/bin/swiftc'
if not compiler.is_file():
    with tempfile.TemporaryDirectory(prefix='swift-toolchain-install-') as temporary:
        package=Path(temporary)/'toolchain.pkg'
        with urllib.request.urlopen(pin['url'], timeout=60) as source, package.open('wb') as target:
            shutil.copyfileobj(source,target)
        with package.open('rb') as source:
            digest=hashlib.file_digest(source,'sha256').hexdigest()
        if digest!=pin['sha256']:raise RuntimeError('Swift toolchain download checksum differs')
        signature=subprocess.check_output(['/usr/sbin/pkgutil','--check-signature',str(package)],text=True)
        if pin['signer'] not in signature:
            raise RuntimeError('Swift toolchain installer signer differs')
        subprocess.run(['/usr/sbin/installer','-target','CurrentUserHomeDirectory','-pkg',str(package)],check=True)
subprocess.run(['/bin/sh',str(Path(sys.argv[1]).parent/'swiftc.sh'),'--version'],check=True)
print(compiler)
PY
