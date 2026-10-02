#!/bin/sh
# Compile the editable icon once for a release candidate's native app builds.
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
OUTPUT=${1:?Supply a fresh icon output directory}
[ ! -e "$OUTPUT" ] && [ ! -L "$OUTPUT" ] || { printf '%s\n' 'Use a fresh icon output directory.' >&2; exit 4; }
if ! xcrun --find actool >/dev/null 2>&1 && [ -x /Applications/Xcode.app/Contents/Developer/usr/bin/actool ]; then
    DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
    export DEVELOPER_DIR
fi
ACTOOL=$(xcrun --find actool 2>/dev/null || true)
[ -n "$ACTOOL" ] || { printf '%s\n' 'Install full Xcode 26 or later for Icon Composer actool.' >&2; exit 3; }
mkdir "$OUTPUT"
"$ACTOOL" "$SCRIPT_DIR/EML.icon" \
    --compile "$OUTPUT" --output-format human-readable-text \
    --notices --warnings --errors --output-partial-info-plist "$OUTPUT/icon-partial.plist" \
    --app-icon EML --include-all-app-icons --enable-on-demand-resources NO \
    --development-region en --target-device mac \
    --minimum-deployment-target 14.0 --platform macosx
[ -s "$OUTPUT/Assets.car" ] && [ -s "$OUTPUT/EML.icns" ] && [ -s "$OUTPUT/icon-partial.plist" ] || {
    printf '%s\n' 'actool omitted Assets.car, EML.icns, or icon metadata.' >&2; exit 3;
}
