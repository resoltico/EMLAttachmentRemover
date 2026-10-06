#!/bin/sh
# The same ASan/libFuzzer target and corpus are used locally and in CI.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python}
[ "$#" -ge 1 ] || { printf '%s\n' 'Usage: fuzz.sh /absolute/output-directory [-runs=INTEGER] [-max_total_time=INTEGER]' >&2; exit 2; }
exec "$PYTHON" -B - "$SCRIPT_DIR" "$@" <<'PY'
import hashlib, json, os, signal, subprocess, sys, tomllib
from pathlib import Path
root = Path(sys.argv[1]).resolve()
output = Path(sys.argv[2])
if not output.is_absolute():
    raise RuntimeError('Fuzz output directory must be absolute')
if output.resolve().is_relative_to(root.parents[1]):
    raise RuntimeError('Keep fuzz corpora and build products outside the checkout')
configuration = (root / 'fuzzing.toml').read_bytes()
campaign = tomllib.loads(configuration.decode('utf-8'))['campaign']
options = {'runs': campaign['runs'], 'max_total_time': campaign['total_seconds']}
for argument in sys.argv[3:]:
    name, separator, value = argument.removeprefix('-').partition('=')
    if not argument.startswith('-') or not separator or name not in options:
        raise RuntimeError('Only -runs=INTEGER and -max_total_time=INTEGER are supported')
    number = int(value)
    if (name == 'runs' and number != -1 and number < 1) or (name == 'max_total_time' and number < 1):
        raise RuntimeError('Runs must be positive or -1; campaign duration must be positive')
    options[name] = number
# Refuse existing paths (including dangling symlinks) before writing any evidence.
output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
output.mkdir(mode=0o700)
os.umask(0o077)
sys.path.insert(0, str(root.parents[1]))
from tools import task_process

def interrupted(_number, _frame):
    raise KeyboardInterrupt

signal.signal(signal.SIGTERM, interrupted)
metadata = {'stage': 'snapshot', 'options': options, 'inputs': {}, 'stages': []}
record = output / 'invocation.json'

def save():
    record.write_text(json.dumps(metadata, indent=2) + '\n')

def run(stage, command, timeout):
    metadata.update(stage=stage, command=command, timeout_seconds=timeout, outcome='running')
    metadata['stages'].append({'stage': stage, 'command': command, 'timeout_seconds': timeout, 'outcome': 'running'})
    save()
    environment = {**os.environ, 'EML_FUZZ_LOG': str(output / (stage + '.log'))}
    task_process.run(['/bin/sh', '-c', 'exec "$@" > "$EML_FUZZ_LOG" 2>&1', stage, *command],
                     cwd=output, env=environment, timeout=timeout)
    metadata['outcome'] = 'passed'
    metadata['stages'][-1]['outcome'] = 'passed'
    save()

save()
try:
    sources = output / 'sources'
    sources.mkdir()
    seeds = output / 'seeds'
    seeds.mkdir()
    corpus = output / 'corpus'
    corpus.mkdir()
    inputs = [root / 'ReportModel.swift', root / 'LauncherFailure.swift', root / 'App/ProgressStream.swift', root / 'Fuzz/ReceiptFuzzer.swift',
              root / 'fuzzing.toml', root / 'toolchain.toml', root / 'fuzz.sh', root / 'swiftc.sh',
              root.parents[1] / 'tools/task_process.py']
    seed_paths = sorted((root / 'Fuzz/corpus').iterdir())
    if not seed_paths:
        raise RuntimeError('No fuzz regression seeds found')
    for source in [*inputs, *seed_paths]:
        if source.is_symlink() or not source.is_file():
            raise RuntimeError('Fuzz inputs must be regular non-symbolic files')
        content = configuration if source.name == 'fuzzing.toml' else source.read_bytes()
        name = source.relative_to(root.parents[1]).as_posix()
        metadata['inputs'][name] = hashlib.sha256(content).hexdigest()
        destination = seeds if source in seed_paths else sources
        (destination / source.name).write_bytes(content)
        if source in seed_paths:
            (corpus / source.name).write_bytes(content)
    compiler = ['/bin/sh', str(sources / 'swiftc.sh')]
    metadata['compiler'] = subprocess.check_output([*compiler, '--version'], text=True, timeout=30)
    metadata['sdk'] = subprocess.check_output(['/usr/bin/xcrun', '--show-sdk-path'], text=True, timeout=30).strip()
    metadata['sdk_version'] = subprocess.check_output(['/usr/bin/xcrun', '--show-sdk-version'], text=True, timeout=30).strip()
    metadata['sdk_build'] = subprocess.check_output(['/usr/bin/xcrun', '--show-sdk-build-version'], text=True, timeout=30).strip()
    toolchain = subprocess.check_output([*compiler, '--toolchain-directory'], text=True, timeout=30).strip()
    os.environ['ASAN_OPTIONS'] = 'abort_on_error=1:halt_on_error=1'
    metadata['asan_options'] = os.environ['ASAN_OPTIONS']
    os.environ['ASAN_SYMBOLIZER_PATH'] = str(Path(toolchain) / 'usr/bin/llvm-symbolizer')
    binary = output / 'receipt-fuzzer'
    run('compile', [*compiler, '-swift-version', '6', '-warnings-as-errors', '-g', '-O',
        '-parse-as-library', '-sanitize=fuzzer,address', str(sources / 'ReportModel.swift'), str(sources / 'LauncherFailure.swift'), str(sources / 'ProgressStream.swift'),
        str(sources / 'ReceiptFuzzer.swift'), '-o', str(binary)], campaign['compile_seconds'])
    base = [str(binary), '-timeout=' + str(campaign['timeout_seconds']),
            '-rss_limit_mb=' + str(campaign['rss_limit_mb']), '-max_len=' + str(campaign['max_length']),
            '-artifact_prefix=' + str(output) + '/']
    run('replay', [*base, *map(str, sorted(seeds.iterdir()))], campaign['replay_seconds'])
    command = [*base, '-seed=' + str(campaign['seed']),
               *['-' + name + '=' + str(value) for name, value in options.items()], str(corpus)]
    run('campaign', command, options['max_total_time'] + campaign['shutdown_seconds'])
except BaseException as error:
    metadata.update(outcome='failed', error=repr(error))
    if metadata['stages'] and metadata['stages'][-1]['outcome'] == 'running':
        metadata['stages'][-1]['outcome'] = 'failed'
    save()
    raise
finally:
    for stage in ('compile', 'replay', 'campaign'):
        log = output / (stage + '.log')
        if log.is_file():
            with log.open('rb') as stream:
                stream.seek(max(0, log.stat().st_size - 4000))
                print(stage + ':\n' + stream.read().decode('utf-8', errors='replace'))
PY
