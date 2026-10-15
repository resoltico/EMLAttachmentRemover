# Verification in progress

The v6 workspace and product are still being implemented. The currently available
Rust campaign commands are below; this document does not claim the remaining
normal/native/archive/GUI gates exist or have passed yet.

From a committed clean checkout with the tools in xtask/tools.toml installed:

```sh
cargo fetch --locked --manifest-path fuzz/Cargo.toml
cargo xtask fuzz --seconds 60
cargo xtask mutants --initial
cargo xtask mutants --diff origin/main
```

Campaigns clone their exact clean source into owned target/fuzz or target/mutations
storage, retain diagnostics/results, and use an owned Unix process group for
bounded cleanup. Dirty or untracked workspace inputs fail with a commit requirement;
they are never silently omitted or called an empty diff. These bounded local
campaign commands require Unix. This is a tooling condition, not a restriction
on the six native product CLI targets. No full mutation campaign is provided.

Fuzz uses the pinned nightly and cargo-fuzz, all twelve synthetic inputs plus
structural seeds, a fresh writable first corpus, 4 MiB maximum input, five-second
case timeout and 1,024 MiB RSS limit. Runtime independent source authorization and
byte/structure checks run within every preparation. The harness rejects Internal
refusals, checks source subsequence/metadata bounds and independently invokes a
second preparation for fixed-point checks. Crash/timeout/RSS/abnormal exit,
artifacts, missing/zero final execution summary or changed admitted inputs fail.
No finite campaign establishes correctness for all byte sequences.

## Pinned cargo-fuzz build limitation

cargo-fuzz 0.13.2 cannot forward --locked to its internal Cargo build. Independent
QA reviewed its exact upstream source and accepted this explicit narrow amendment:

1. Resolve full metadata with the pinned nightly, --locked --offline and explicit
   host target; capture the admitted lock, manifests, configuration and graph.
2. Build offline using pinned cargo-fuzz and its normal address-sanitized mode.
   Do not add --build-std or copy its instrumentation flags into another build.
3. Before launching any harness, require unchanged inputs and the same full locked
   metadata. Drift fails visibly and is not restored or concealed.
4. Execute the resulting harness directly, with the required bounded arguments,
   owned corpus/artifact directories and cargo-fuzz's detect_odr_violation=0 default.
   Do not cargo-fuzz run after this admission, because it rebuilds.

The internal build does not receive --locked. Pre-execution checks reject graph
drift before fuzzing, but do not retroactively prevent build scripts executing
if unexpected resolution drift occurred during that build. All other repository
Cargo builds retain --locked. Byte identity is an input-control observation,
not proof of fuzz execution or review; the actual execution summary is separate.
See [pinned cargo-fuzz source](https://github.com/rust-fuzz/cargo-fuzz/blob/984c861c8dfea28055254c5f1d2659ab2cd63f76/src/project.rs).

## Scoped mutation selection

Only eml-core authorize_deletions and check_edit_ranges are selected. Discovery
must be positive in both, with at most 400 total. Initial execution covers the
entire selection. Diff mode inspects source hunks against the merge base and
selects whole changed guards, not a first-N subset; unchanged guard selection is
explicitly not applicable and does not cover other changed parser/workflow code.

The driver uses profile mutants, baseline run, one job, copy-target=false,
test-workspace=false, cap-lints=true, --locked Cargo arguments, 15-second tests,
120-second builds and a 45-minute total deadline. It checks baseline, each admitted
mutant identity, execution completeness, per-outcome classifications and counts.
Missed/timeouts/incomplete/unclassified results fail; unviable is not caught and
requires retained compiler evidence/source-local review. No global digest-based
survivor registry or automatic equivalence waiver is used.
