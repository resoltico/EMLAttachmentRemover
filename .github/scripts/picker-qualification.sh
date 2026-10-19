#!/usr/bin/env bash
# CI glue only. The real product consumer and shared evidence predicates decide
# qualification; neither a probe nor a measurement exit alone qualifies an app.
set -euo pipefail

mode=$1
archive=$2
reference=$3
picker_source=$(git rev-parse HEAD)
[[ "${picker_source}" =~ ^[0-9a-f]{40}$ ]]
[[ "${GITHUB_RUN_ID:?}" =~ ^[1-9][0-9]*$ ]]
[[ "${GITHUB_RUN_ATTEMPT:?}" =~ ^[1-9][0-9]*$ ]]

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
  test "${size}" -le "${2:-2097152}"
}
expected() {
  local archive_id reference_id
  archive_id=$(file_identity "${archive}")
  reference_id=$(file_identity "${reference}")
  jq -n --arg source "${picker_source}" --arg run "${GITHUB_RUN_ID}" --arg attempt "${GITHUB_RUN_ATTEMPT}" \
    --arg workflow_ref "${GITHUB_WORKFLOW_REF:?}" --arg workflow_sha "${GITHUB_WORKFLOW_SHA:?}" \
    --argjson archive "${archive_id}" --argjson reference "${reference_id}" \
    '{source:$source,target:"x86_64-apple-darwin",run:$run,attempt:$attempt,archive:$archive,reference:$reference,workflow_ref:$workflow_ref,workflow_sha:$workflow_sha}'
}
validate() {
  local maximum=2097152
  case "$1" in
    modified_qualification|qualification) maximum=16777216 ;;
    *) ;;
  esac
  bounded_document "$2" "${maximum}"
  jq -e -L .github/scripts --slurpfile e "${out}/expected.json" \
    "include \"picker-evidence\"; $1(\$e[0])" "$2" > /dev/null
}
current_product() {
  local records=(target/app-picker-compatibility/native-*/result.json)
  test "${#records[@]}" = 1
  validate product "${records[0]}"
  cat "${records[0]}"
}

case "${mode}" in
  normal)
    out=target/picker-normal
    mkdir "${out}"
    expected > "${out}/expected.json"
    if cargo xtask check-macos-picker --archive "${archive}" --reference "${reference}"; then
      current_product > "${out}/product.json"
      jq -n --slurpfile e "${out}/expected.json" --slurpfile p "${out}/product.json" \
        '$e[0]+{stage:"picker_normal_observation",outcome:"normal_pass",original_exit_code:0,product:$p[0]}' > "${out}/candidate.json"
    else
      original_status=$?
      normals=(target/app-picker-compatibility/native-*/gui-picker)
      test "${#normals[@]}" = 1
      normal=${normals[0]}
      for file in process.json owner-result.json xctest.log native-picker-owned-process.log; do
        bounded_document "${normal}/${file}"
      done
      retained=(target/app-picker-compatibility/native-*/artifact-*/inspected-input.zip)
      test "${#retained[@]}" = 1
      cmp "${retained[0]}" "${archive}"
      jq -n --slurpfile e "${out}/expected.json" --argjson status "${original_status}" \
        --slurpfile process "${normal}/process.json" --slurpfile owner "${normal}/owner-result.json" \
        --rawfile test_log "${normal}/xctest.log" --rawfile owned_log "${normal}/native-picker-owned-process.log" \
        '$e[0]+{stage:"picker_normal_observation",outcome:"unclassified_failure",original_exit_code:$status,process:$process[0],owner:$owner[0],xctest_log:$test_log,owned_log:$owned_log,scope:"failed normal product evidence; measurement is not compatibility acceptance"}' > "${out}/failure.json"
      jq -e -L .github/scripts --slurpfile e "${out}/expected.json" \
        'include "picker-evidence"; (.outcome="known_panel_failure") | known_failure($e[0])' "${out}/failure.json" > /dev/null
      jq '.outcome="known_panel_failure"' "${out}/failure.json" > "${out}/candidate.json"
    fi
    validate normal_observation "${out}/candidate.json"
    expected > "${out}/expected-after.json"
    cmp "${out}/expected.json" "${out}/expected-after.json"
    mv "${out}/candidate.json" "${out}/result.json"
    jq -er .outcome "${out}/result.json" > "${out}/outcome.txt"
    ;;
  modified)
    out=target/picker-modified
    mkdir "${out}"
    expected > "${out}/expected.json"
    normal_record=$4/picker-normal/result.json
    validate known_failure "${normal_record}"
    cargo xtask prepare-macos-picker-host --archive "${archive}" --reference "${reference}" \
      --normal-failure "${normal_record}" --out "${out}/panel-observation"
    preparation=${out}/panel-observation/icon-service-experiment/result.json
    bounded_document "${preparation}"
    jq -e -L .github/scripts --arg source "${picker_source}" \
      'include "picker-evidence"; preparation($source)' "${preparation}" > /dev/null
    expected > "${out}/expected-after-preparation.json"
    cmp "${out}/expected.json" "${out}/expected-after-preparation.json"
    jq -n --slurpfile e "${out}/expected.json" --slurpfile n "${normal_record}" --slurpfile p "${preparation}" \
      '$e[0]+{stage:"picker_modified_attempt",state:"prepared",normal:$n[0],preparation:$p[0]}' > "${out}/attempt.json"
    if cargo xtask check-macos-picker --archive "${archive}" --reference "${reference}"; then
      product_status=0
    else
      product_status=$?
    fi
    jq --argjson status "${product_status}" '.product_exit_code=$status' "${out}/attempt.json" > "${out}/attempt-after.json"
    mv "${out}/attempt-after.json" "${out}/attempt.json"
    test "${product_status}" = 0
    current_product > "${out}/product.json"
    expected > "${out}/expected-after-product.json"
    cmp "${out}/expected.json" "${out}/expected-after-product.json"
    jq -n --slurpfile e "${out}/expected.json" --slurpfile n "${normal_record}" \
      --slurpfile prep "${preparation}" --slurpfile p "${out}/product.json" \
      '$e[0]+{stage:"picker_modified_qualification",normal:$n[0],preparation:$prep[0],product:$p[0],scope:"actual same-frozen-product dedicated picker pass on an explicitly modified fresh host; original normal/probe failures retained; no healthy-normal-host claim"}' > "${out}/candidate.json"
    validate modified_qualification "${out}/candidate.json"
    mv "${out}/candidate.json" "${out}/result.json"
    ;;
  select)
    out=target/picker-qualification
    mkdir "${out}"
    expected > "${out}/expected.json"
    normal_record=$4/picker-normal/result.json
    validate normal_observation "${normal_record}"
    normal_outcome=$(jq -er .outcome "${normal_record}")
    if test "${normal_outcome}" = normal_pass; then
      test "${5:-}" = ""
      jq -n --slurpfile e "${out}/expected.json" --slurpfile n "${normal_record}" \
        '$e[0]+{stage:"picker_qualification",mode:"normal",normal:$n[0],modified:null,product:$n[0].product}' > "${out}/candidate.json"
    else
      modified_record=${5:?}/picker-modified/result.json
      validate modified_qualification "${modified_record}"
      jq -n --slurpfile e "${out}/expected.json" --slurpfile n "${normal_record}" --slurpfile m "${modified_record}" \
        '$e[0]+{stage:"picker_qualification",mode:"modified_host",normal:$n[0],modified:$m[0],product:$m[0].product}' > "${out}/candidate.json"
    fi
    validate qualification "${out}/candidate.json"
    mv "${out}/candidate.json" "${out}/result.json"
    ;;
  *) exit 2 ;;
esac
