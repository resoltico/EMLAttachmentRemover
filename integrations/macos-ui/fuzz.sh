#!/bin/sh
# The same ASan/libFuzzer target and corpus are used locally and in CI.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python}
[ "$#" -ge 1 ] || { printf '%s\n' 'Usage: fuzz.sh /absolute/output-directory [libFuzzer options]' >&2; exit 2; }
"$PYTHON" -B - "$SCRIPT_DIR" "$@" <<'PY'
import json, shutil, subprocess, sys, tomllib
from pathlib import Path
root=Path(sys.argv[1]);output=Path(sys.argv[2])
if not output.is_absolute():raise RuntimeError('Fuzz output directory must be absolute')
if output.resolve().is_relative_to(root.resolve().parents[1]):
    raise RuntimeError('Keep fuzz corpora and build products outside the checkout')
output.mkdir(mode=0o700,parents=True,exist_ok=True)
campaign=tomllib.loads((root/'fuzzing.toml').read_text())['campaign']
compiler=['/bin/sh',str(root/'swiftc.sh')]
version=subprocess.check_output([*compiler,'--version'],text=True)
sdk=subprocess.check_output(['/usr/bin/xcrun','--show-sdk-path'],text=True).strip()
binary=output/'receipt-fuzzer'
subprocess.run([*compiler,'-swift-version','6','-warnings-as-errors','-g','-O',
    '-parse-as-library','-sanitize=fuzzer,address',str(root/'ReportModel.swift'),
    str(root/'Fuzz/ReceiptFuzzer.swift'),'-o',str(binary)],check=True)
corpus=output/'corpus';corpus.mkdir(exist_ok=True)
seeds=sorted((root/'Fuzz/corpus').iterdir())
if not seeds:raise RuntimeError('No fuzz regression seeds found')
for seed in seeds:shutil.copyfile(seed,corpus/seed.name)
base=[str(binary),'-timeout='+str(campaign['timeout_seconds']),'-rss_limit_mb='+str(campaign['rss_limit_mb']),
      '-max_len='+str(campaign['max_length']),'-artifact_prefix='+str(output)+'/']
with (output/'replay.log').open('w') as log:
    subprocess.run([*base,*map(str,seeds)],stdout=log,stderr=subprocess.STDOUT,check=True)
command=[*base,'-seed='+str(campaign['seed']),'-runs='+str(campaign['runs']),*sys.argv[3:],str(corpus)]
(output/'invocation.json').write_text(json.dumps({'compiler':version,'sdk':sdk,'command':command},indent=2)+'\n')
with (output/'campaign.log').open('w') as log:
    result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=False)
print((output/'campaign.log').read_text()[-4000:])
raise SystemExit(result.returncode)
PY
