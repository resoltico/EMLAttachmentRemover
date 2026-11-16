# Project rules

These rules apply the shared AGENTS.md principles to this repository. When they
conflict with a design pack or an agent's plan, these rules win unless the owner
changes them.

## Verification budget

1. Harness code stays smaller than product code. `scripts/check-budget.sh` measures
   it (product: `crates/*/src`, `crates/*/ui`; harness: `xtask/`, `install/`, `tests/`,
   `crates/*/tests/`, `scripts/`, `.github/`). CI fails when harness ≥ product.
2. The merge gate finishes in about 20 minutes or less with a warm cache.
3. Every merge-gate check runs locally with `cargo xtask ci` (Windows excepted).
4. A check that fails because of the environment (hosted VM UI state, timing,
   OS services outside our boundary) leaves the merge tier. It is not patched
   around with retries, longer waits or more observation code.

## Boundaries we test, and boundaries we do not

- We test our code at our boundary: core functions, the CLI process, the protocol
  stream, the GUI through Slint's testing backend, and the exact release archives.
- We do not test the operating system's own UI or services (Finder, Dock, file
  dialogs, accessibility of system apps).

## Adding machinery

Name the requirement or risk that a new check, tool, workflow, evidence record or
dependency serves in its PR, and remove machinery in the PR that removes its reason.

## Threat model

`docs/threat-model.md` defines what the filesystem code defends against.

## Product constraints

Slint is the GUI toolkit. Desktop library and headless development support macOS and Linux; Windows supports core and the CLI only. The native desktop GUI and its distribution target macOS only. Linux builds use an Ubuntu 26.04 baseline on both CPU architectures. Slint is used as published: no vendored or patched toolkit crates without an owner decision recorded here. Signing is ad hoc only. Intel Macs are supported. macOS CLI and app distributions ship separate Apple Silicon and Intel archives. macOS-only integration is limited to application-menu About routing, the Quit shim and document opening.

Front ends call core in process and render its events. Core owns admission.
