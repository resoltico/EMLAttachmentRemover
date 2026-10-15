#!/bin/sh
# The same ASan/libFuzzer target and corpus are used locally and in CI.
set -eu
export PYTHONDONTWRITEBYTECODE=1
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python}
[ "$#" -ge 1 ] || { printf '%s\n' 'Usage: fuzz.sh /absolute/output-directory [-runs=INTEGER] [-max_total_time=INTEGER]' >&2; exit 2; }
exec "$PYTHON" -B - "$SCRIPT_DIR" "$@" <<'PY'
import hashlib, json, os, resource, shutil, signal, subprocess, sys, tomllib
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
    if number < 1 or (name == 'runs' and number > campaign['max_runs']):
        raise RuntimeError('Runs must be positive and no greater than max_runs; duration must be positive')
    options[name] = number
if not 1 <= options['runs'] <= campaign['max_runs'] or campaign['log_bytes'] < 1:
    raise RuntimeError('Configured runs and log byte limit must respect the campaign bounds')
# Refuse existing paths (including dangling symlinks) before writing any evidence.
output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
output.mkdir(mode=0o700)
os.umask(0o077)
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
sys.path.insert(0, str(root.parents[1]))
from tools import task_process

def interrupted(_number, _frame):
    raise KeyboardInterrupt

signal.signal(signal.SIGTERM, interrupted)
metadata = {'stage': 'snapshot', 'options': options, 'inputs': {}, 'stages': [],
            'limits': {'log_bytes_per_stage': campaign['log_bytes'], 'core_bytes': 0,
                       'max_runs': campaign['max_runs'], 'max_input_bytes': campaign['max_length']}}
record = output / 'invocation.json'
scratch = output / 'scratch'

def save():
    record.write_text(json.dumps(metadata, indent=2) + '\n')

def run(stage, command, timeout):
    metadata.update(stage=stage, command=command, timeout_seconds=timeout, outcome='running')
    metadata['stages'].append({'stage': stage, 'command': command, 'timeout_seconds': timeout, 'outcome': 'running'})
    save()
    environment = {**os.environ, 'EML_FUZZ_LOG': str(output / (stage + '.log')),
                   'EML_FUZZ_LOGGER': str(output / 'sources/fuzz_log.py'),
                   'EML_FUZZ_PYTHON': sys.executable, 'EML_FUZZ_LOG_BYTES': str(campaign['log_bytes']),
                   'TMPDIR': str(scratch), 'TMP': str(scratch), 'TEMP': str(scratch),
                   'CLANG_MODULE_CACHE_PATH': str(scratch / 'clang-modules')}
    task_process.run(['/bin/bash', '-o', 'pipefail', '-c',
                     '"$@" 2>&1 | "$EML_FUZZ_PYTHON" -B "$EML_FUZZ_LOGGER" "$EML_FUZZ_LOG" "$EML_FUZZ_LOG_BYTES"',
                     stage, *command],
                     cwd=output, env=environment, timeout=timeout)
    metadata['outcome'] = 'passed'
    metadata['stages'][-1]['outcome'] = 'passed'
    save()

save()
try:
    scratch.mkdir()
    sources = output / 'sources'
    sources.mkdir()
    seeds = output / 'seeds'
    seeds.mkdir()
    corpus = output / 'corpus'
    corpus.mkdir()
    inputs = [root / 'ReportModel.swift', root / 'LauncherFailure.swift', root / 'App/ProgressStream.swift', root / 'Fuzz/ReceiptFuzzer.swift',
              root / 'fuzzing.toml', root / 'toolchain.toml', root / 'fuzz.sh', root / 'swiftc.sh',
              root.parents[1] / 'tools/task_process.py', root.parents[1] / 'tools/fuzz_log.py']
    seed_paths = sorted((root / 'Fuzz/corpus').iterdir())
    if not seed_paths:
        raise RuntimeError('No fuzz regression seeds found')
    for source in [*inputs, *seed_paths]:
        if source.is_symlink() or not source.is_file():
            raise RuntimeError('Fuzz inputs must be regular non-symbolic files')
        content = configuration if source.name == 'fuzzing.toml' else source.read_bytes()
        if source in seed_paths and len(content) > campaign['max_length']:
            raise RuntimeError('Fuzz seed exceeds configured max_length: ' + str(source))
        name = source.relative_to(root.parents[1]).as_posix()
        metadata['inputs'][name] = hashlib.sha256(content).hexdigest()
        destination = seeds if source in seed_paths else sources
        (destination / source.name).write_bytes(content)
        if source in seed_paths:
            (corpus / source.name).write_bytes(content)
    metadata['limits']['seed_count'] = len(seed_paths)
    metadata['limits']['seed_bytes'] = sum(path.stat().st_size for path in seeds.iterdir())
    # Pinned libFuzzer FuzzerLoop.cpp: only ReportNewCoverage writes corpus files;
    # it runs after MutateAndTestOne checks MaxNumberOfRuns. Seed/leak replay does
    # not call it. These are corpus payload bounds, not a filesystem quota.
    metadata['limits']['corpus_files'] = options['runs'] + len(seed_paths)
    metadata['limits']['corpus_payload_bytes'] = options['runs'] * campaign['max_length'] + metadata['limits']['seed_bytes']
    metadata['limits']['slow_input_files'] = options['runs'] + len(seed_paths)
    metadata['limits']['slow_input_payload_bytes'] = metadata['limits']['slow_input_files'] * campaign['max_length']
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
        '-parse-as-library', '-sanitize=fuzzer,address', '-module-cache-path', str(scratch / 'swift-modules'),
        str(sources / 'ReportModel.swift'), str(sources / 'LauncherFailure.swift'), str(sources / 'ProgressStream.swift'),
        str(sources / 'ReceiptFuzzer.swift'), '-o', str(binary)], campaign['compile_seconds'])
    # One owned worker already keeps its discoveries in memory; rereading its
    # output directory wastes I/O and can replay previously reduced inputs.
    base = [str(binary), '-reload=0', '-timeout=' + str(campaign['timeout_seconds']),
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
    try:
        shutil.rmtree(scratch)
    except OSError as error:
        metadata.update(outcome='failed', scratch_cleanup_error=repr(error))
        save()
        raise
    metadata['scratch_cleanup'] = 'passed'
    save()
    for stage in ('compile', 'replay', 'campaign'):
        log = output / (stage + '.log')
        if log.is_file():
            with log.open('rb') as stream:
                stream.seek(max(0, log.stat().st_size - 4000))
                print(stage + ':\n' + stream.read().decode('utf-8', errors='replace'))
PY
