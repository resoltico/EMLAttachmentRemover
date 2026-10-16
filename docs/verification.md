# Verification in progress

The v6 workspace and product are still being implemented. The currently available
Rust campaign commands are below; this document does not claim the remaining
normal/native/archive/GUI gates exist or have passed yet.

`cargo xtask distribution` prints the native target policy and expected archive
identities derived from `xtask/distribution.toml` and actual locked Cargo metadata.
It creates no product archive or qualification record. The existing Unix and
Windows filesystem evidence workflows consume its target rows; their separate
process-probe subset is unchanged. This metadata task does not initialize the
Unix campaign controller or signal handlers. Its fixed read-only Cargo metadata
subprocess has ordinary Cargo lifecycle behavior, without a campaign cleanup or
execution-deadline guarantee. Windows uses the workflow's explicit native GNU
LLVM toolchain/target invocation; native execution remains separately evidenced.
Compiler pins stay in rust-toolchain.toml and xtask/tools.toml; deployment/runtime
floors in the distribution table are policies, not proof of OS execution.

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
120-second per-mutant builds and a 45-minute execution deadline. Baseline
build time is not capped by cargo-mutants' per-mutant option; it is bounded by
that outer execution deadline. Additional bounded cleanup grace is recorded
separately. It checks baseline, each admitted
mutant identity, execution completeness, per-outcome classifications and counts.
Missed/timeouts/incomplete/unclassified results fail; unviable is not caught and
requires retained compiler evidence/source-local review. No global digest-based
survivor registry or automatic equivalence waiver is used.

The single reviewed compiler-invalid structural case is source-local: eml-core
verify.rs authorize_deletions FnValue `Ok(Default::default())`, with its expected
authorization Result type, Build-only failure101 and actual local outcome log
showing only the specific Authorization:Default E0277 cause. Other mutations,
missing/ambiguous logs, another compiler cause, a Test phase, or more than one
unviable result fail. Reports retain separate caught/unviable/reviewed_unviable
counts; reviewed_unviable is never counted as a test catch.

## Bounded preparation campaign observations

Source `8e3ed3277dd7faaaee7b665e0901c4f294d38799`, native macOS ARM64:

- Fuzz run `run-aTbUer`: configured 60 seconds, actual 61 seconds,
  1,540,438 executions, peak RSS 552 MiB, normal exit, no artifacts. All twelve
  fixture and ten edge seeds were admitted; locked graphs and inputs were unchanged.
  Controller reports no cancellation/deadline/survivors and completed cleanup.
  Independent QA inspected the raw execution/build/control records and accepted
  this bounded pure-preparation run. It proves no other platform/product gate.
- Mutation run `run-CgbmO2`: baseline build/test passed 59 tests; all fifteen
  discovered mutations were attempted (ten edit-range, five authorization).
  Fourteen individually built and failed guard tests; zero missed or timed out.
  One authorization FnValue replacement, exactly `Ok(Default::default())`, failed
  its Build phase with E0277 because `Authorization: Default` is unsatisfied;
  it had no Test phase. Authorization requires independently derived bytes/proof.
  Independent QA reviewed the producer, diff, full compiler diagnostic and outcome
  accounting and classified this single mutation as compiler-unviable, not caught
  or equivalent. Every viable selected mutation was caught. Original automatic
  result `qualified:false` remains intact; this paragraph records the separate
  reviewed disposition and does not turn an unviable mutation into a test result.

Raw retained evidence lives in the owned clean campaign checkout under
`target/campaign-checkout.EuUzqu/source/target/{fuzz,mutations}/run-*` on this
implementation host. Those files are execution evidence, not reproducible inputs
or a replacement for final-head qualification. No full mutation or 900-second fuzz
campaign was run.

## Verification process lifetime

Private helpers in the developer xtask establish a fresh session and acknowledge
readiness before a worker starts. A root group pin is created and acknowledged
before the worker joins it. Additional process groups are discovered numerically,
rechecked through kernel session/group queries, and joined by owned pins; the
kernel refuses a cross-session join. No guessed group or process-name match
is signal authority. Pins remain unreaped through the last signal and post-stop
observations, even when an original group leader has exited.

Parent INT/TERM/HUP adapters only set cancellation state. Normal control code
closes the parent-only control writer; parent death also produces EOF. Workers
receive null stdin. Supervisor-owned pin writers are not inherited by workers or
other helpers. Pins validate their private session/group/nonce handshake, survive
HUP/INT/TERM and use EOF to terminate their own pinned group. Stopped pins rely on
the demonstrated orphaned-group HUP/CONT cascade under the reviewed group-parent
topology; EOF does not itself wake SIGSTOP.

Cleanup freezes the retained root first, then discovers/pins/freezes additional
groups, requires stable stopped/dead observations, escalates and observes no live
nonadministrative members. Failed/malformed inspection, unstable quiescence,
unsupported guardian-group membership, missing result, unexpected survivors or
unconfirmed cleanup cannot qualify. Worker status, deadline/cancellation and
cleanup evidence are separate; supervisor exit0 is insufficient. Observed elapsed
time is checked even when the worker has already exited between polls.

This controls the reviewed pinned tool topology, not arbitrary process escape or
an atomic universal process-table snapshot. Compiler-wrapper/target-runner hooks
must be absent or explicitly reviewed; the driver refuses unreviewed effective
hooks and captures configuration inputs. OS hangs or SIGKILL of the supervisor
can leave undiscovered groups unconfirmed; pin EOF covers only already pinned
groups. Missing supervisor completion remains failure. Helpers are developer
verification resources and are not introduced into the product CLI/app runtime.

The process integration harness isolates its outer scenarios with one test-local
mutex because they share a global process census and finite observation budget.
Each scenario retains its own real process concurrency and all negative controls.
The guard covers setup through final observations and resource cleanup, including
panic unwinding. Poison recovery permits later independent tests to run; it does
not catch a failed test or turn Drop into external-cleanup evidence. Normal Cargo
test invocation and native CI now use that same isolation without a separate
test-thread flag. This establishes isolated-scenario execution, not simultaneous
independent-controller or ambient-churn resilience. Production session inspection,
unknown/refusal handling and cleanup budgets are unchanged.
