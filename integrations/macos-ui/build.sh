#!/bin/sh
# Build one self-contained macOS app for the selected CPU from the working tree.
set -eu
export PYTHONDONTWRITEBYTECODE=1
umask 077
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PROJECT_ROOT=$(CDPATH='' cd -P "$SCRIPT_DIR/../.." && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$("$PYTHON" -B -c 'import time; print(int(time.time()) // 2 * 2)')}
export SOURCE_DATE_EPOCH
TARGET=${1:-"$PROJECT_ROOT/build/EML Attachment Remover.app"}
ARCH=${2:-$(uname -m)}
case "$ARCH" in arm64|x86_64) ;; *) printf '%s\n' 'Choose arm64 or x86_64.' >&2; exit 3 ;; esac
[ "$(uname -s)" = Darwin ] || { printf '%s\n' 'The native UI builds on macOS only.' >&2; exit 3; }
[ ! -L "$TARGET" ] && [ ! -e "$TARGET" ] || { printf '%s\n' 'Use a fresh app build destination.' >&2; exit 4; }
SWIFT_TOOLCHAIN=$(/bin/sh "$SCRIPT_DIR/swiftc.sh" --toolchain-directory)
SWIFTC="$SWIFT_TOOLCHAIN/usr/bin/swiftc"
if [ -z "${EML_ICON_RESOURCES:-}" ] && ! xcrun --find actool >/dev/null 2>&1 && [ -x /Applications/Xcode.app/Contents/Developer/usr/bin/actool ]; then
    DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
    export DEVELOPER_DIR
fi
SDK=$(xcrun --show-sdk-path)
mkdir -p "$(dirname "$TARGET")"
STAGING=$(mktemp -d "$(dirname "$TARGET")/.eml-ui-build.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
APP="$STAGING/EML Attachment Remover.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
"$PYTHON" -B "$PROJECT_ROOT/tools/build_zipapp.py" --target "$APP/Contents/Resources/remove-eml-attachments.pyz"
cp "$SCRIPT_DIR/ARTWORK.md" "$PROJECT_ROOT/LICENSE" "$APP/Contents/Resources/"
cp "$PROJECT_ROOT/integrations/macos-ui/processing-launcher.sh" "$APP/Contents/Resources/processing-launcher.sh"
ICON_OUTPUT=${EML_ICON_RESOURCES:-"$STAGING/icon-assets"}
if [ -z "${EML_ICON_RESOURCES:-}" ]; then
    /bin/sh "$SCRIPT_DIR/compile-icon.sh" "$ICON_OUTPUT"
fi
[ -s "$ICON_OUTPUT/Assets.car" ] && [ -s "$ICON_OUTPUT/EML.icns" ] && [ -s "$ICON_OUTPUT/icon-partial.plist" ] || {
    printf '%s\n' 'Compiled icon resources are missing or empty.' >&2; exit 3;
}
cp "$ICON_OUTPUT/Assets.car" "$ICON_OUTPUT/EML.icns" "$APP/Contents/Resources/"
"$PYTHON" - "$PROJECT_ROOT/pyproject.toml" "$APP/Contents/Info.plist" "$ICON_OUTPUT/icon-partial.plist" <<'PY'
import plistlib, sys, tomllib
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]).parent))
from tools.macos_metadata import build_number
notice = 'Copyright © ' + next(line for line in (Path(sys.argv[1]).parent/'LICENSE').read_text().splitlines() if line.startswith('Copyright (c) ')).removeprefix('Copyright (c) ')
version = tomllib.loads(Path(sys.argv[1]).read_text())['project']['version']
with open(sys.argv[2], 'wb') as target:
    metadata = {
        'CFBundleIdentifier': 'io.github.resoltico.emlattachmentremover',
        'NSHumanReadableCopyright': notice,
        'CFBundleName': 'EML Attachment Remover',
        'CFBundleDisplayName': 'EML Attachment Remover',
        'CFBundleExecutable': 'EMLAttachmentRemover',
        'CFBundlePackageType': 'APPL',
        'CFBundleShortVersionString': version,
        'CFBundleVersion': build_number(Path(sys.argv[1])),
        'LSMinimumSystemVersion': '14.0',
        'NSHighResolutionCapable': True,
        'CFBundleDocumentTypes': [{'CFBundleTypeName': 'EML message', 'CFBundleTypeExtensions': ['eml'], 'CFBundleTypeRole': 'Viewer', 'LSHandlerRank': 'None'}],
    }
    with open(sys.argv[3], 'rb') as icon_source:
        metadata.update(plistlib.load(icon_source))
    metadata['CFBundleIconName'] = 'EML'
    plistlib.dump(metadata, target)
PY
# Compile an immutable source snapshot for the selected CPU.
cp "$SCRIPT_DIR/ReportModel.swift" "$SCRIPT_DIR/LauncherFailure.swift" "$SCRIPT_DIR/Artwork.swift" "$STAGING/"
mkdir "$STAGING/app"
cp "$SCRIPT_DIR"/App/*.swift "$STAGING/app/"
"$SWIFTC" -sdk "$SDK" -swift-version 6 -warnings-as-errors -O \
    -target "$ARCH-apple-macosx14.0" \
    "$STAGING/ReportModel.swift" "$STAGING/LauncherFailure.swift" "$STAGING/Artwork.swift" "$STAGING"/app/*.swift \
    -o "$APP/Contents/MacOS/EMLAttachmentRemover"
printf '%s\n' 'EML Attachment Remover native UI managed installation' > "$APP/Contents/Resources/.eml-ui-installation"
"$PYTHON" -B - "$APP" <<'PY'
import sys
from pathlib import Path
root = Path(sys.argv[1])
for path in [root, *root.rglob('*')]:
    if path.is_symlink(): raise OSError('Unexpected symbolic link in native app')
    path.chmod(0o755 if path.is_dir() else 0o644)
PY
chmod 755 "$APP/Contents/MacOS/EMLAttachmentRemover" "$APP/Contents/Resources/processing-launcher.sh"
# Local signing binds the executable and bundled processing resources. This is not notarization.
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
"$PYTHON" -B - "$PROJECT_ROOT" "$APP" <<'PYTHON'
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from tools.build_timestamp import stamp_tree
stamp_tree(Path(sys.argv[2]))
PYTHON
mv "$APP" "$TARGET"
printf 'Built: %s\n' "$TARGET"
