#!/usr/bin/env bash
# Release smoke for an extracted CLI: every MIME contract input produces its
# golden copy byte for byte, inputs without a golden are refused without a copy,
# and originals stay unchanged. Usage: smoke-cli.sh CLI_EXECUTABLE CONTRACT_DIR
set -euo pipefail
cli="$1"
contract="$2"
"${cli}" --version
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0
inputs=0
for input in "${contract}"/inputs/*.eml; do
  inputs=$((inputs + 1))
  name=$(basename "${input}" .eml)
  cp "${input}" "${work}/${name}.eml"
  status=0
  "${cli}" -- "${work}/${name}.eml" >"${work}/${name}.log" 2>&1 || status=$?
  copy="${work}/${name}.no-attachments.eml"
  golden="${contract}/golden/${name}.eml"
  if [[ -f "${golden}" ]]; then
    if [[ "${status}" -ne 0 ]] || ! cmp -s "${golden}" "${copy}"; then
      echo "FAIL ${name}: exit ${status} or copy differs from golden" >&2; failures=$((failures + 1))
    fi
  elif [[ "${status}" -eq 0 ]] || [[ -e "${copy}" ]]; then
    echo "FAIL ${name}: expected refusal without a copy (exit ${status})" >&2; failures=$((failures + 1))
  fi
  cmp -s "${input}" "${work}/${name}.eml" || { echo "FAIL ${name}: original changed" >&2; failures=$((failures + 1)); }
done
[[ "${failures}" -eq 0 ]] || { echo "${failures} CLI smoke failure(s)" >&2; exit 1; }
[[ "${inputs}" -gt 0 ]] || { echo "no contract inputs" >&2; exit 1; }
echo "cli smoke ok: ${inputs} inputs"
