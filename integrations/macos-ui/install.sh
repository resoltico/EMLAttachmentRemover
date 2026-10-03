#!/bin/sh
# Install only our owned native application; preserve the previous bundle.
set -eu
umask 077
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
APP=${1:?Supply the freshly built EML Attachment Remover.app path}
DESTINATION=${EML_REMOVER_UI_APP:-"$HOME/Applications/EML Attachment Remover.app"}
"$PYTHON" -c 'import platform, sys; raise SystemExit(platform.python_implementation() != "CPython" or sys.version_info[:2] != (3, 14))'     || { printf '%s\n' 'The native UI installer requires CPython 3.14.' >&2; exit 9; }
codesign --verify --deep --strict "$APP"
"$PYTHON" - "$APP" "$DESTINATION" <<'PY'
import json, os, plistlib, shutil, stat, struct, subprocess, sys, tempfile
from pathlib import Path
def install():
    source = Path(sys.argv[1]).absolute()
    destination = Path(sys.argv[2]).absolute()
    marker = 'EML Attachment Remover native UI managed installation\n'
    identifier = 'io.github.resoltico.emlattachmentremover'
    def owned_tree(root):
        for item in [root, *root.rglob('*')]:
            mode = item.lstat()
            if mode.st_uid != os.geteuid() or not (stat.S_ISDIR(mode.st_mode) or stat.S_ISREG(mode.st_mode)):
                raise OSError('Application contains an unowned or unsafe entry')
        if (root/'Contents/Resources/.eml-ui-installation').read_text() != marker:
            raise OSError('Application is not an owned native UI bundle')
        with (root/'Contents/Info.plist').open('rb') as stream:
            if plistlib.load(stream).get('CFBundleIdentifier') != identifier:
                raise OSError('Application identity differs')
    owned_tree(source)
    physical_arm = subprocess.run(
        ['/usr/sbin/sysctl', '-n', 'hw.optional.arm64'],
        capture_output=True, text=True, check=False
    ).stdout.strip() == '1'
    required_cpu = 'arm64' if physical_arm else 'x86_64'
    # Mach-O header_64: magic, CPU type, subtype, file type (Apple loader.h/machine.h).
    with (source/'Contents/MacOS/EMLAttachmentRemover').open('rb') as executable_file:
        header = executable_file.read(16)
    expected_cpu = (0x0100000c, 0) if physical_arm else (0x01000007, 3)
    if len(header) != 16:
        raise OSError('Application executable header is incomplete')
    magic, cpu_type, cpu_subtype, file_type = struct.unpack('<4I', header)
    if (magic, cpu_type, cpu_subtype & 0x00ffffff, file_type) != (0xfeedfacf, *expected_cpu, 2):
        raise OSError(f'Application CPU does not match this Mac ({required_cpu})')
    executable = str(destination / 'Contents/MacOS/EMLAttachmentRemover')
    processes = subprocess.check_output(['/bin/ps', '-axo', 'pid=,comm='], text=True)
    if any(line.strip().split(None, 1)[-1] == executable for line in processes.splitlines()):
        raise OSError('Quit EML Attachment Remover before updating its application bundle')
    for ancestor in [destination.parent, *destination.parent.parents]:
        if ancestor.is_symlink():
            raise OSError('Refusing a symbolic-link installation ancestor')
    destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    parent_stat = destination.parent.stat()
    if parent_stat.st_uid != os.geteuid() or stat.S_IMODE(parent_stat.st_mode) & 0o022:
        raise OSError('Application directory is not privately owned')
    if destination.exists() or destination.is_symlink():
        owned_tree(destination)
        if destination == source:
            raise OSError('Source and destination are identical')
    configuration = Path.home()/'Library/Application Support/EML Attachment Remover UI'
    for ancestor in [configuration, *configuration.parents]:
        if ancestor.is_symlink():
            raise OSError('Refusing a symbolic-link runtime configuration ancestor')
    configuration.mkdir(mode=0o700, parents=True, exist_ok=True)
    configuration_stat = configuration.stat()
    if configuration_stat.st_uid != os.geteuid() or stat.S_IMODE(configuration_stat.st_mode) & 0o077:
        raise OSError('Runtime configuration directory is not privately owned')
    runtime = configuration/'runtime.json'
    if runtime.exists() or runtime.is_symlink():
        runtime_stat = runtime.lstat()
        if not stat.S_ISREG(runtime_stat.st_mode) or runtime_stat.st_uid != os.geteuid():
            raise OSError('Refusing an unsafe runtime configuration file')
    stage = Path(tempfile.mkdtemp(prefix='.eml-ui-install-', dir=destination.parent))
    backup = None
    runtime_temp = None
    try:
        fd, runtime_temp = tempfile.mkstemp(prefix='.runtime-', dir=configuration)
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            json.dump({'python': str(Path(sys.executable).resolve())}, stream)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        prepared = stage/destination.name
        shutil.copytree(source, prepared, symlinks=False)
        owned_tree(prepared)
        subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(prepared)], check=True)
        if destination.exists():
            # The prior installation remains recoverable, outside the application path.
            backup_root = Path.home()/'Library/Application Support/EML Attachment Remover UI Backups'
            backup_root.mkdir(mode=0o700, parents=True, exist_ok=True)
            if backup_root.is_symlink() or backup_root.stat().st_uid != os.geteuid() or stat.S_IMODE(backup_root.stat().st_mode) & 0o077:
                raise OSError('Backup directory is not private')
            backup = Path(tempfile.mkdtemp(prefix='previous-', dir=backup_root))/(destination.stem + '.bundle-backup')
            os.rename(destination, backup)
        try:
            os.rename(prepared, destination)
            try:
                os.replace(runtime_temp, runtime)
            except BaseException:
                os.rename(destination, prepared)
                raise
        except BaseException:
            if backup is not None:
                os.rename(backup, destination)
            raise
    finally:
        if runtime_temp is not None and os.path.exists(runtime_temp): os.unlink(runtime_temp)
        shutil.rmtree(stage)
    print(f'Installed: {destination}')
    if backup is not None:
        print(f'Previous application retained: {backup}')

try:
    install()
except (OSError, ValueError, subprocess.CalledProcessError) as exc:
    print(f'Native UI installation refused: {exc}', file=sys.stderr)
    raise SystemExit(4)

PY
printf '%s\n' 'Finder Quick Action shell command:'
# shellcheck disable=SC2016  # Print literal variables for the Shortcuts action.
printf '%s\n' '/usr/bin/open -a "$HOME/Applications/EML Attachment Remover.app" -- "$@"'
printf '%s\n' 'Use this single action with Shortcut Input passed as arguments.'
