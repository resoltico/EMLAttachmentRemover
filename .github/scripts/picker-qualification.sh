#!/usr/bin/env bash
# CI glue only. Preparation never substitutes for the actual frozen-product case.
set -euo pipefail

archive=$1
reference=$2
picker_source=$(git rev-parse HEAD)
[[ "${picker_source}" =~ ^[0-9a-f]{40}$ ]]
[[ "${GITHUB_RUN_ID:?}" =~ ^[1-9][0-9]*$ ]]
[[ "${GITHUB_RUN_ATTEMPT:?}" =~ ^[1-9][0-9]*$ ]]
out=target/picker-qualification
mkdir "${out}"
cargo xtask distribution > "${out}/distribution.json"

file_identity() {
  test -f "$1" && test ! -L "$1" || return
  local digest size
  digest=$(shasum -a 256 "$1" | cut -d ' ' -f 1) || return
  size=$(wc -c < "$1" | tr -d '[:space:]') || return
  jq -n --arg sha "${digest}" --argjson size "${size}" '{sha256:$sha,byte_length:$size}'
}
bounded_document() {
  test -f "$1" && test ! -L "$1" || return
  local size
  size=$(wc -c < "$1" | tr -d '[:space:]') || return
  test "${size}" -le 16777216
}
expected() {
  local archive_id reference_id
  archive_id=$(file_identity "${archive}")
  reference_id=$(file_identity "${reference}")
  jq -n --arg source "${picker_source}" --arg run "${GITHUB_RUN_ID}" --arg attempt "${GITHUB_RUN_ATTEMPT}" \
    --arg workflow_ref "${GITHUB_WORKFLOW_REF:?}" --arg workflow_sha "${GITHUB_WORKFLOW_SHA:?}" \
    --argjson archive "${archive_id}" --argjson reference "${reference_id}" \
    --slurpfile distribution "${out}/distribution.json" \
    '{source:$source,target:"x86_64-apple-darwin",run:$run,attempt:$attempt,archive:$archive,reference:$reference,workflow_ref:$workflow_ref,workflow_sha:$workflow_sha,picker_host:$distribution[0].picker_host}'
}
validate() {
  bounded_document "$2"
  jq -e -L .github/scripts --slurpfile e "${out}/expected.json" \
    "include \"picker-evidence\"; $1(\$e[0])" "$2" > /dev/null
}
expected > "${out}/expected.json"
cargo xtask prepare-macos-picker-host --archive "${archive}" --reference "${reference}" \
  --out "${out}/host-preparation"
preparation=${out}/host-preparation/preparation.json
validate host_preparation "${preparation}"
expected > "${out}/expected-after-preparation.json"
cmp "${out}/expected.json" "${out}/expected-after-preparation.json"
jq -n --slurpfile e "${out}/expected.json" --slurpfile p "${preparation}" \
  '$e[0]+{stage:"picker_qualification_attempt",state:"prepared",preparation:$p[0]}' > "${out}/attempt.json"
cargo xtask check-macos-picker --archive "${archive}" --reference "${reference}"
records=(target/app-picker-compatibility/native-*/result.json)
test "${#records[@]}" = 1
validate product "${records[0]}"
cp "${records[0]}" "${out}/product.json"
cargo xtask verify-macos-picker-host --archive "${archive}" --reference "${reference}" \
  --preparation "${preparation}" --out "${out}/host-verification"
verification=${out}/host-verification/verification.json
validate host_verification "${verification}"
expected > "${out}/expected-after-product.json"
cmp "${out}/expected.json" "${out}/expected-after-product.json"
jq -n --slurpfile e "${out}/expected.json" --slurpfile prep "${preparation}" \
  --slurpfile product "${out}/product.json" --slurpfile verification "${verification}" \
  '$e[0]+{stage:"picker_qualification",mode:"modified_host",preparation:$prep[0],product:$product[0],postverification:$verification[0],scope:"actual same-frozen-product picker pass on the explicitly provisioned host; not unmodified-host compatibility or causal proof"}' > "${out}/candidate.json"
validate qualification "${out}/candidate.json"
mv "${out}/candidate.json" "${out}/result.json"
jq '{stage,mode,source,target,run,attempt,picker_host}' "${out}/result.json"
