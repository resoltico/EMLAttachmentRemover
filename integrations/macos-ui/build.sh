#!/bin/sh
# Build a self-contained universal macOS app from the current working tree.
set -eu
export PYTHONDONTWRITEBYTECODE=1
umask 077
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
PROJECT_ROOT=$(CDPATH='' cd -P "$SCRIPT_DIR/../.." && pwd -P)
PYTHON=${EML_REMOVER_PYTHON:-python3.14}
TARGET=${1:-"$PROJECT_ROOT/build/EML Attachment Remover.app"}
[ "$(uname -s)" = Darwin ] || { printf '%s\n' 'The native UI builds on macOS only.' >&2; exit 3; }
[ ! -L "$TARGET" ] && [ ! -e "$TARGET" ] || { printf '%s\n' 'Use a fresh app build destination.' >&2; exit 4; }
SWIFT_TOOLCHAIN=$(/bin/sh "$SCRIPT_DIR/swiftc.sh" --toolchain-directory)
SWIFTC="$SWIFT_TOOLCHAIN/usr/bin/swiftc"
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
"$PYTHON" - "$PROJECT_ROOT/pyproject.toml" "$APP/Contents/Info.plist" <<'PY'
import plistlib, sys, tomllib
from pathlib import Path
notice = 'Copyright © ' + next(line for line in (Path(sys.argv[1]).parent/'LICENSE').read_text().splitlines() if line.startswith('Copyright (c) ')).removeprefix('Copyright (c) ')
version = tomllib.loads(Path(sys.argv[1]).read_text())['project']['version']
with open(sys.argv[2], 'wb') as target:
    plistlib.dump({
        'CFBundleIdentifier': 'io.github.resoltico.emlattachmentremover',
        'NSHumanReadableCopyright': notice,
        'CFBundleName': 'EML Attachment Remover',
        'CFBundleDisplayName': 'EML Attachment Remover',
        'CFBundleExecutable': 'EMLAttachmentRemover',
        'CFBundlePackageType': 'APPL',
        'CFBundleIconFile': 'EML.icns',
        'CFBundleShortVersionString': version,
        'CFBundleVersion': version,
        'LSMinimumSystemVersion': '14.0',
        'NSHighResolutionCapable': True,
        'CFBundleDocumentTypes': [{'CFBundleTypeName': 'EML message', 'CFBundleTypeExtensions': ['eml'], 'CFBundleTypeRole': 'Viewer', 'LSHandlerRank': 'None'}],
    }, target)
PY
# Compile an immutable source snapshot so both CPU slices use the same input.
cp "$SCRIPT_DIR/ReportModel.swift" "$SCRIPT_DIR/CreateIcon.swift" "$SCRIPT_DIR/Artwork.swift" "$STAGING/"
mkdir "$STAGING/app"
cp "$SCRIPT_DIR"/App/*.swift "$STAGING/app/"
"$SWIFTC" -sdk "$SDK" -parse-as-library -swift-version 6 -warnings-as-errors "$STAGING/Artwork.swift" "$STAGING/CreateIcon.swift" -o "$STAGING/create-icon"
"$STAGING/create-icon" "$STAGING/EML.iconset"
/usr/bin/iconutil -c icns "$STAGING/EML.iconset" -o "$APP/Contents/Resources/EML.icns"
for ARCH in arm64 x86_64; do
    "$SWIFTC" -sdk "$SDK" -swift-version 6 -warnings-as-errors -O \
        -target "$ARCH-apple-macosx14.0" \
        "$STAGING/ReportModel.swift" "$STAGING/Artwork.swift" "$STAGING"/app/*.swift \
        -o "$STAGING/ui-$ARCH"
done
xcrun lipo -create "$STAGING/ui-arm64" "$STAGING/ui-x86_64" -output "$APP/Contents/MacOS/EMLAttachmentRemover"
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
mv "$APP" "$TARGET"
printf 'Built: %s\n' "$TARGET"
