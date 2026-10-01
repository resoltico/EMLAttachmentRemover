# Coding-agent instructions

Apply these principles within the authorized task and governing instructions; resolve conflicts by instruction precedence and flag unresolved conflicts.

## Name by meaning, not incidental development history

Name files, directories and identifiers for their responsibility, behavior or domain concept, using the project's idioms. Keep terminology consistent within a context and make necessary distinctions between contexts explicit.

Prefer names that convey meaning in context. Broad names such as `utils`, `manager` or `data` deserve scrutiny when they hide responsibility or collect unrelated concerns. Clarify responsibility before renaming; refactor only when warranted and within scope.

Do not name things merely for implementation generations, task provenance, development status or unsupported superiority. `new`, `legacy`, `v2`, `temp` and `improved` are examples to examine, not banned words. Distinguish alternatives by meaningful properties: `StreamingParser` and `BufferedParser`, not `Parser` and `ImprovedParser`.

States, stages, ordering, lifetimes and versions are valid when they describe the domain, algorithm, contract, compatibility boundary or artifact. `api/v2`, `LegacyEncoding`, `TemporaryDirectory` and `FinalInvoice` can be precise. A versioned public contract need not lose its versioned name when older versions retire.

Apply this to comments, documentation, logs, errors and test descriptions too. Retain history where it explains current constraints or belongs in a historical record.

Review names introduced or affected by the task, including your prose. Fix misleading names within scope and update references. Respect external conventions, persisted formats and established compatibility commitments; use an authorized migration or deprecation path where needed.

## Simplicity

Build the simplest design that fully solves the stated problem: few concepts, special cases and hidden states, with one authoritative place for each fact. Simple does not mean short, clever or quick to write. Keep complexity required for correctness, security, error handling and stated requirements. Flag avoidable complexity in explicit requirements without silently overriding them.

**Understand first.** Read relevant code, contracts and tests; validate assumptions before changing them. Prefer root-cause fixes; identify necessary mitigations and their limits.

**Look wider than the matter at hand.** Examine related defects, duplicated mechanisms and downstream effects for shared causes. Consider fundamentally different approaches, including replacement, where simpler. Keep removals and refactors within scope; report related work outside it with a recommendation.

**Before adding anything** (code, file, abstraction, option, dependency, check, document):

- Name the concrete need: a requirement, defect or real risk. Without one, do not add it. Verification and documentation count when tied to required behavior or real risks.
- Do not build speculative options, extension points or compatibility layers. Establish compatibility commitments from instructions, supported interfaces, persisted data and consumers. Coordinate changes with consumers and rollout; break commitments only when authorized.
- Prefer removing unnecessary work, suitable platform features, existing project code, then a maintained dependency or new code according to total complexity. Do not force reuse or a dependency that fits poorly.
- Keep one authoritative source per fact; derive or check dependent copies. Avoid hand-maintained derived inventories, not necessary source data. Keep test oracles independent of the logic under test.
- Avoid duplicate mechanisms for the same responsibility. Do not merge distinct contracts merely because their code looks similar.

**Before removing anything:**

- Understand its users and the property it protects. If unsure, investigate; absence of search hits or test coverage is not sufficient evidence for deletion.
- Preserve required safety, security and correctness properties when replacing checks. Remove unnecessary mechanisms rather than adding tests or documentation to justify them.
- Delete what the task demonstrably makes obsolete, including your own superseded scaffolding.

**Judge the whole, not the part.** Improving a metric alone does not establish simplification; neither does shifting complexity to callers or users. Count concepts, moving parts, state and indirection across the system.

**Before finishing:** remove needless changes, then verify intended behavior and protection against regressions. Explain necessary complexity.

## Design before building

Scale design and review to risk, not line count: a one-line change to authorization or data deletion can be high-risk. Routine low-risk edits need only a brief sanity check; use distinct design and challenge passes for non-trivial changes to behavior, contracts or mechanisms.

1. **Design.** Establish the problem, relevant constraints, alternatives and chosen approach. Consider a fundamentally different approach where it could be simpler, and effects on callers, data, tests, documentation and operations.
2. **Challenge it.** Review the design in a separate skeptical pass. Test assumptions against code, experiments or counterexamples; try to make it fail. Revise where the evidence disagrees, rather than restating the design.
3. **Build.** Implement the reviewed design. If implementation invalidates an assumption, revisit and challenge that part before proceeding.

## Finish the whole change

Follow the change through its dependents, not just the edited files.

- Find affected callers, configuration, lockfiles, generated files, build/CI rules, documentation, tests and external consumers. Update in-scope dependents; identify needed external coordination.
- Search relevant inputs, including hidden, ignored or binary files when needed. Check tool exclusions and stale references; retain valid history, compatibility and rejection-test references. No text matches does not prove completeness.
- Check that intended files reach deliverables and relevant files reach required checks, including test discovery. Inspect tracking, ignore and packaging rules; regenerate affected outputs per project policy.

Report implementation, verification and external rollout separately. Do not claim an unmet requirement is satisfied or widen scope without authorization.

## Checks must prove something

A check, test, gate or record must support a property someone relies on; match the strength of the claim to the evidence.

- Hashes can verify identity or integrity against a reference, not review quality. A completion claim does not establish execution; mocks alone do not demonstrate real-system integration. Use real-boundary verification where the property requires it.
- Test intended behavior, boundaries and relevant failures. Give guards a case they must reject. Use regression tests or negative controls to demonstrate detection, not just code execution.
- Keep controls proportionate. Do not build a check mainly to appease another check or inflate coverage; address the underlying risk. Independent checks of the same property can be justified.
- Fix real findings; correct invalid checks against established requirements. Do not lower required assurance without authorization. Exceptions must be explicit, narrow and justified.
- Investigate relevant failures, including pre-existing ones. Fix in-scope issues, continue independent safe work, and report unresolved failures or unavailable checks. Never report a failing or unrun gate as passing.

## Verification

An exit code establishes only what that command's success criteria mean. Check the intended result; local success does not establish success in the authoritative environment.

- After a meaningful state-changing operation or related batch, inspect its effects: changed files, commit contents, generated outputs or installed versions. Preserve failure visibility; do not let a later success mask an earlier failure.
- Check expected outputs and test discovery, including skipped tests and exclusions. Investigate unexpectedly empty, small or missing results before treating them as success; expected quiet output is not itself a failure.
- Diagnose from the full relevant failure output, retaining details without dumping sensitive or excessive content.
- Use targeted checks during iteration; run required verification where authorized and available. Prefer an isolated, clean environment matching the required platform, toolchain and restore mode without discarding user work.
- Verify the delivered state, not an earlier working tree. Report what ran, outcomes and untested platforms or conditions.

## Documentation and comments state the current truth

Describe supported behavior, assumptions and reasons. Keep history in change or decision records unless it explains a current constraint or migration; do not erase useful rationale. Prefer links, generated content or consistency checks over manually duplicated reference facts. The authoritative source may be code or configuration, not a document. Update affected documentation, examples and error text with the change, and check affected links and generated blocks proportionately. Do not create documentation machinery merely to satisfy this rule.

## Working hygiene

- Check inputs and prerequisites cheaply before long runs.
- Keep running checks' inputs stable or isolate them; intended watch/reload workflows are fine.
- Preserve unrelated work. Clean up your temporary resources, not deliverables or useful failure evidence. Leave requested services running.
- Keep output focused; do not dump large generated files or expose secrets.

## Easy to start

Document the shortest setup from a clean checkout, prerequisites and actionable errors. Reuse commands; do not mandate setup wrappers or doctor tools. Declare each toolchain version authoritatively; derive or check required copies. Pin inputs for reproducible builds; retain supported dependency ranges and deliberate compatibility probes.
