#!/usr/bin/env bash
# Release floor smoke for the macOS app: no UI automation.
# Proves: the bundle verifies (ad hoc), contains exactly the expected CPU architecture, launches on
# this OS with launch operands and quits on an idle Quit request without writing.
# Core-to-desktop Start is checked through Slint and the native checklist;
# the separate CLI archive smoke owns CLI copy verification.
# Usage: smoke-macos-app.sh APP_BUNDLE FIXTURE.eml PROBE_BINARY EXPECTED_ARCH
set -euo pipefail
[[ "$#" -eq 4 ]] || { echo "Usage: $0 APP_BUNDLE FIXTURE.eml PROBE_BINARY EXPECTED_ARCH" >&2; exit 2; }
expected_arch="$4"
case "${expected_arch}" in
  arm64|x86_64) ;;
  *) echo "unsupported macOS architecture: ${expected_arch}" >&2; exit 2 ;;
esac
app=$(cd "$1" && pwd)
fixture="$2"
probe="$3"
plist="${app}/Contents/Info.plist"
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${plist}")
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${plist}")

codesign --verify --deep --strict "${app}"
signature=$(codesign -dv "${app}" 2>&1)
[[ "${signature}" == *$'\nSignature=adhoc'* ]] || { echo "app is not ad hoc signed" >&2; exit 1; }
[[ ! -e "${app}/Contents/Helpers" ]] || { echo "app still contains Helpers" >&2; exit 1; }
binary="${app}/Contents/MacOS/${executable}"
for product in "${binary}" "${probe}"; do
  actual_arch=$(lipo -archs "${product}")
  [[ "${actual_arch}" == "${expected_arch}" ]] \
    || { echo "expected exactly ${expected_arch}, found ${actual_arch}: ${product}" >&2; exit 1; }
done
host_arch=$(uname -m)
[[ "${host_arch}" == "${expected_arch}" ]] \
  || { echo "floor smoke requires a native ${expected_arch} host" >&2; exit 1; }
translated=$(sysctl -in sysctl.proc_translated)
[[ "${translated}" != "1" ]] \
  || { echo "floor smoke cannot run under Rosetta" >&2; exit 1; }
gui_version=$("${app}/Contents/MacOS/${executable}" --version)
app_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "${plist}")
revision="${EML_SOURCE_REVISION:-$(git rev-parse --verify HEAD)}"
[[ "${gui_version}" == "emlattachmentremover-gui ${app_version} (${revision})" ]] \
  || { echo "GUI source identity differs from the checked revision" >&2; exit 1; }

work=$(mktemp -d)
trap 'osascript -e "tell application id \"$bundle_id\" to quit" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT
cp "${fixture}" "${work}/one.eml"
cp "${fixture}" "${work}/two.eml"

open -n -a "${app}" --args "${work}/one.eml" "${work}/two.eml"

deadline=$((SECONDS + 90))
until pgrep -f "${app}/Contents/MacOS/${executable}" >/dev/null; do
  [[ "${SECONDS}" -lt "${deadline}" ]] || { echo "app did not launch" >&2; exit 1; }
  sleep 1
done

osascript -e "tell application id \"${bundle_id}\" to quit"
deadline=$((SECONDS + 30))
while pgrep -f "${app}/Contents/MacOS/${executable}" >/dev/null; do
  [[ "${SECONDS}" -lt "${deadline}" ]] || { echo "app did not quit" >&2; exit 1; }
  sleep 1
done
for original in "${work}/one.eml" "${work}/two.eml"; do
  [[ ! -e "${original%.eml}.no-attachments.eml" ]] || { echo "app wrote a copy without Start" >&2; exit 1; }
  cmp -s "${fixture}" "${original}" || { echo "an original changed: ${original}" >&2; exit 1; }
done
os_version=$(sw_vers -productVersion)
architecture=$(uname -m)
echo "smoke ok: ${bundle_id} on ${os_version} ${architecture}"
bash "$(dirname "$0")/smoke-macos-document-event.sh" "${app}" "${fixture}" "${probe}"
