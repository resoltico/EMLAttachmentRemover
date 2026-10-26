#!/usr/bin/env bash
# Release floor smoke for the macOS app: no UI automation.
# Proves: the bundle verifies (ad hoc), contains both CPU slices, launches on
# this OS with launch operands and quits on an idle Quit request without writing.
# The bundled CLI helper produces verified copies; desktop Start is tested through
# Slint and the manual release checklist.
# Usage: smoke-macos-app.sh APP_BUNDLE FIXTURE.eml [expected_archs]
set -euo pipefail
app=$(cd "$1" && pwd)
fixture="$2"
fixture_dir=$(dirname "${fixture}")
fixture_name=$(basename "${fixture}")
golden="${fixture_dir}/../golden/${fixture_name}"
[[ -f "${golden}" ]] || { echo "fixture has no expected golden: ${golden}" >&2; exit 1; }
expected_archs="${3:-arm64 x86_64}"
plist="${app}/Contents/Info.plist"
bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${plist}")
executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "${plist}")

codesign --verify --deep --strict "${app}"
signature=$(codesign -dv "${app}" 2>&1)
[[ "${signature}" == *$'\nSignature=adhoc'* ]] || { echo "app is not ad hoc signed" >&2; exit 1; }
for binary in "${app}/Contents/MacOS/${executable}" "${app}/Contents/Helpers/emlattachmentremover"; do
  slices=" $(lipo -archs "${binary}") "
  for arch in ${expected_archs}; do
    [[ "${slices}" == *" ${arch} "* ]] || { echo "missing CPU slice ${arch}: ${binary}" >&2; exit 1; }
  done
done

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
"${app}/Contents/Helpers/emlattachmentremover" -- "${work}/one.eml" "${work}/two.eml"
for original in "${work}/one.eml" "${work}/two.eml"; do
  cmp -s "${golden}" "${original%.eml}.no-attachments.eml" || { echo "helper copy differs from golden" >&2; exit 1; }
  cmp -s "${fixture}" "${original}" || { echo "an original changed: ${original}" >&2; exit 1; }
done
os_version=$(sw_vers -productVersion)
architecture=$(uname -m)
echo "smoke ok: ${bundle_id} on ${os_version} ${architecture}"
