#!/usr/bin/env bash
# Lint the shell sources that can be delivered from this working tree.
set -euo pipefail
listing=$(mktemp)
trap 'rm -f "${listing}"' EXIT
git ls-files -z --cached --others --exclude-standard -- '*.sh' > "${listing}"
files=()
while IFS= read -r -d '' path; do
  [[ ! -f "${path}" ]] || files+=("${path}")
done < "${listing}"
[[ ${#files[@]} -gt 0 ]] || { echo 'No shell inputs were discovered' >&2; exit 1; }
shellcheck -- "${files[@]}"
