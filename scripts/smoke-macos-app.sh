#!/usr/bin/env bash
# Release floor smoke for the macOS app: no UI automation.
# Proves: the bundle verifies (ad hoc), contains both CPU slices, launches on
# this OS with launch operands and quits on an idle Quit request without writing.
# Core-to-desktop Start is checked through Slint and the native checklist;
# the separate CLI archive smoke owns CLI copy verification.
# Usage: smoke-macos-app.sh APP_BUNDLE FIXTURE.eml PROBE_BINARY [expected_archs]
set -euo pipefail
app=$(cd "$1" && pwd)
fixture="$2"
probe="$3"
expected_archs="${4:-arm64 x86_64}"
plist="${app}/Contents/Info.plist"
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${plist}")
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${plist}")

codesign --verify --deep --strict "${app}"
signature=$(codesign -dv "${app}" 2>&1)
[[ "${signature}" == *$'\nSignature=adhoc'* ]] || { echo "app is not ad hoc signed" >&2; exit 1; }
[[ ! -e "${app}/Contents/Helpers" ]] || { echo "app still contains Helpers" >&2; exit 1; }
binary="${app}/Contents/MacOS/${executable}"
slices=" $(lipo -archs "${binary}") "
for arch in ${expected_archs}; do
  [[ "${slices}" == *" ${arch} "* ]] || { echo "missing CPU slice ${arch}: ${binary}" >&2; exit 1; }
done
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
