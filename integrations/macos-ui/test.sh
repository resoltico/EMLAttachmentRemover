#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
STAGING=$(mktemp -d "${TMPDIR:-/tmp}/eml-ui-tests.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
CHECK_TRANSLATED_INTEL=0
case "$(uname -m):$(/usr/bin/sw_vers -productVersion)" in
    arm64:14.*) CHECK_TRANSLATED_INTEL=1 ;;
esac
for OPTIMIZATION in -Onone -O; do
    /bin/sh "$SCRIPT_DIR/swiftc.sh" -swift-version 6 -warnings-as-errors "$OPTIMIZATION" "$SCRIPT_DIR/ReportModel.swift" "$SCRIPT_DIR/LauncherFailure.swift" "$SCRIPT_DIR/test-model.swift" -o "$STAGING/model-tests"
    "$STAGING/model-tests"
    if [ "$CHECK_TRANSLATED_INTEL" = 1 ]; then
        [ "$(/usr/bin/arch -x86_64 /usr/bin/uname -m)" = x86_64 ] || { printf '%s\n' 'Intel translation is unavailable.' >&2; exit 1; }
        /bin/sh "$SCRIPT_DIR/swiftc.sh" -swift-version 6 -warnings-as-errors "$OPTIMIZATION" -target x86_64-apple-macosx14.0 "$SCRIPT_DIR/ReportModel.swift" "$SCRIPT_DIR/LauncherFailure.swift" "$SCRIPT_DIR/test-model.swift" -o "$STAGING/model-tests-intel"
        /usr/bin/arch -x86_64 "$STAGING/model-tests-intel"
    fi
done
