#!/usr/bin/env bash
# Verification budget: harness lines must stay below product lines.
# Product: crates/*/src and crates/*/ui. Harness: xtask/, tests/, crates/*/tests/, .github/.
# Usage: check-budget.sh [git-ref]   (default: HEAD)
set -euo pipefail
ref="${1:-HEAD}"
product=$(git ls-tree -r --name-only "$ref" -- crates \
  | grep -E '^crates/[^/]+/(src|ui)/' | grep -E '\.(rs|slint)$' \
  | while IFS= read -r path; do git show "$ref:$path"; done | wc -l | tr -d ' ')
harness_paths=$(git ls-tree -r --name-only "$ref" -- xtask tests .github crates \
  | grep -E '^(xtask|tests|\.github)/|^crates/[^/]+/tests/' \
  | grep -E '\.(rs|slint|swift|sh|ps1|yml|yaml|toml|jq)$' || true)
harness=$(printf '%s\n' "$harness_paths" | while IFS= read -r path; do
  [ -n "$path" ] && git show "$ref:$path"; done | wc -l | tr -d ' ')
echo "product=$product harness=$harness ref=$ref"
if [ "$harness" -ge "$product" ]; then
  echo "verification budget exceeded: harness ($harness) >= product ($product)" >&2
  exit 1
fi
