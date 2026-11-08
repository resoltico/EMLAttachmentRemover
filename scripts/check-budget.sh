#!/usr/bin/env bash
# Compare maintained product and verification source, never generated build output.
# Default: current working tree, including non-ignored untracked files.
# An optional Git revision measures historical/delivered inputs instead.
set -euo pipefail
[[ $# -le 1 ]] || { echo 'usage: check-budget.sh [git-ref]' >&2; exit 2; }
listing=$(mktemp)
trap 'rm -f "${listing}"' EXIT
if [[ $# -eq 1 ]]; then
  tree=$(git rev-parse --verify --end-of-options "$1^{tree}")
  git ls-tree -rz --name-only "${tree}" > "${listing}"
  inputs="ref=$1"
else
  git ls-files -z --cached --others --exclude-standard > "${listing}"
  inputs=worktree
fi
product=0
harness=0
while IFS= read -r -d '' path; do
  category=
  if [[ "${path}" =~ ^crates/[^/]+/(src|ui)/.*\.(rs|slint)$ ]]; then
    category=product
  elif [[ "${path}" =~ ^(xtask|tests|scripts|\.github)/|^crates/[^/]+/tests/ ]] \
    && [[ "${path}" =~ \.(rs|slint|swift|sh|ps1|yml|yaml|toml|jq)$ ]]; then
    category=harness
  fi
  [[ -n "${category}" ]] || continue
  if [[ $# -eq 1 ]]; then
    lines=$(git show "${tree}:${path}" | awk 'END {print NR+0}')
  else
    [[ -f "${path}" ]] || continue # Deleted tracked paths are not build inputs.
    lines=$(awk 'END {print NR+0}' "${path}")
  fi
  if [[ "${category}" == product ]]; then product=$((product + lines));
  else harness=$((harness + lines)); fi
done < "${listing}"
echo "product=${product} harness=${harness} inputs=${inputs}"
if [[ ${product} -eq 0 || ${harness} -ge ${product} ]]; then
  echo "verification budget exceeded or empty: harness (${harness}) must be below product (${product})" >&2
  exit 1
fi
