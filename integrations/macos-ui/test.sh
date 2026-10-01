#!/bin/sh
set -eu
SCRIPT_DIR=$(CDPATH='' cd -P "$(dirname "$0")" && pwd -P)
STAGING=$(mktemp -d "${TMPDIR:-/tmp}/eml-ui-tests.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
xcrun swiftc -swift-version 6 -warnings-as-errors "$SCRIPT_DIR/ReportModel.swift" "$SCRIPT_DIR/test-model.swift" -o "$STAGING/model-tests"
"$STAGING/model-tests"
