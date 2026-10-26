# Project rules

These rules apply the shared AGENTS.md principles to this repository. When they
conflict with a design pack or an agent's plan, these rules win unless the owner
changes them.

## Verification budget

1. Harness code stays smaller than product code. `scripts/check-budget.sh` measures
   it (product: `crates/*/src`, `crates/*/ui`; harness: `xtask/`, `tests/`,
   `crates/*/tests/`, `.github/`). CI fails when harness ≥ product.
2. The merge gate finishes in about 20 minutes or less with a warm cache.
3. Every gate names the property it protects and has a case it must reject
   (a negative control, regression test or guard test).
4. Every merge-gate check runs locally with `cargo xtask ci` (Windows excepted).
5. A check that fails because of the environment (hosted VM UI state, timing,
   OS services outside our boundary) leaves the merge tier. It is not patched
   around with retries, longer waits or more observation code.

## Boundaries we test, and boundaries we do not

- We test our code at our boundary: core functions, the CLI process, the protocol
  stream, the GUI through Slint's testing backend, and the exact release archives.
- We do not test the operating system's own UI or services (Finder, Dock, file
  dialogs, accessibility of system apps).

## Adding machinery

Before adding a check, tool, workflow, evidence record or dependency, name the
requirement or risk it serves in the PR. Prefer platform features and maintained
crates. Remove machinery that loses its reason in the same PR.

## Threat model

`docs/threat-model.md` defines what the filesystem code defends against. Do not add
defenses outside it without changing it first.

## Owner decisions in force

Slint is the GUI toolkit (a cross-platform GUI is planned). Signing is ad hoc only.
Intel Macs are supported. macOS-only integration is limited to application-menu About routing and the Quit shim.
