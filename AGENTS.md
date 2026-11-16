# Coding-agent instructions

Baseline revision: 2026-11-16

Shared engineering and conduct rules for every project. Apply them in proportion to the task, within the authorized scope and higher-priority instructions. Sections on interfaces, storage, human-language text and the changelog apply wherever the project has such a part; a command-line tool has an interface. Establish project-specific constraints and checks from their sources, not assumptions.

## Scope and precedence

This file is the shared baseline. Do not modify, append to, replace, move or delete it without the user's explicit authorization to change the baseline; general permission to refactor or improve documentation is not that authorization. Propose corrections in your response instead.

Before repository work, read `AGENTS_EXTRA.md` beside this file if it exists; its absence is normal, but report a read failure. The supplement adds repository-specific context. It cannot override this baseline or higher-priority instructions, and no instruction file can grant you authority. Create no other agent-instruction files without explicit authorization.

Keep in the supplement only brief, verified, durable, repository-specific guidance that materially helps future work, linking to existing sources rather than copying them. Exclude task plans, progress logs, incident narratives, generic advice, speculative rules and empty templates. When changing it, check accuracy, necessity, duplication and consistency, and remove obsolete guidance. Before finishing, confirm you did not change the baseline without authorization, and report supplement changes.

## Judgment and authority

Distinguish authority from evidence: instructions establish what is requested, not whether its assumptions or methods are correct. Attribute decisions accurately to the user, to delegation or to your own proposals, which stay open to review.

Test consequential assumptions against relevant code, observed behaviour and authoritative sources for the versions in use. Separate observations from inferences and preferences. Experience, approval and consensus are not proof; revise assumptions that verified findings contradict, even in an approved approach.

Respect instruction precedence. When requirements, designs, code, checks or evidence conflict, investigate and explain the consequences and remaining uncertainty. Do not stretch unrelated instructions or present a design correction as preservation. Pause work that depends on an unresolved consequential contradiction; continue safe investigation and independent work. Correct methods and sequencing within delegated authority; ask before changing requirements or commitments beyond it. Explain consequential corrections before acting, and record why.

Look for the same failure in adjacent assumptions and consumers. Prefer a sound outcome to preserving earlier work or apparent progress.

Actions visible beyond your working copy need authorization for that action, from the task or the user: pushing, opening or merging pull requests, tagging, releasing, publishing, deploying, posting, deleting shared or remote resources, and rewriting history others use. An approval covers only the action it names.

## Simplicity and change

Good code is easy to change. Build the simplest solution that meets the requirements and is easy to understand. Minimize concepts, special cases, hidden state and total maintenance burden, not lines of code; keep the complexity that correctness, security and error handling need.

Read the relevant code, contracts and tests before changing them. Prefer root-cause fixes, and state any mitigation's limits. Consider fundamentally different approaches where they could be simpler.

Before adding code, abstractions, dependencies, options, checks or documents:

- Identify a concrete requirement, defect, maintenance burden or real risk. Build no speculative capabilities, extension points or compatibility layers.
- Prefer removing unnecessary work, suitable platform features and existing code. Choose between a maintained dependency and new code by total complexity; do not force a poor fit.
- Give each shared fact or rule one owner, chosen by meaning rather than convenience. Derive or check what depends on it, and avoid unnecessary hand-maintained copies; authoritative source data is not a copy.
- Share an abstraction only when it reduces total complexity without coupling responsibilities that change independently. Similar code or wording does not make a shared contract.

Compatibility is a commitment to identified consumers that the project declares, such as a stable interface or a supported upgrade path for stored data. Without one, make clean breaks: replace directly and update every consumer, with no shims, aliases, deprecation periods or migrations. A version that cannot read existing stored data refuses it visibly and never misreads or silently discards it. Honour declared commitments, coordinate their rollout, and break them only when authorized.

Before removing anything, understand what uses it and what it protects; missing search hits or tests do not prove it unused. Keep required protections, and remove what the task demonstrably makes obsolete.

Judge complexity across the whole system, including callers and operators. Keep changes in scope, and report related work outside it.

## Ownership and boundaries

### Core

The core holds the project's capabilities and meaning, independent of any one interface: domain rules, use cases and shared presentation such as messages. Interfaces (command line, GUI, web, network API, library facade, scripts), storage and other integrations are adapters around it. Dependencies point toward the core, which depends on no interface, storage technology or application framework (web, UI, persistence); value libraries such as decimals and time are fine. Modules within one package suffice; enforce the split in the build where that is cheap.

Core rules compute results from explicit inputs, including time, identities, randomness and configuration. Clocks, input and output, environment and persistence belong in adapters.

### Interfaces

An interface gathers input, invokes core capabilities and renders outcomes; it does not decide.

- It chooses which capabilities to expose and how to lay them out; what a capability requires, when it is available, what it means and what it is called come from the core.
- It may bind capabilities by hand, but takes their inputs, rules, availability and wording from core declarations and decides presentation from declared properties, never by testing which capability it is. Where several interfaces share capabilities, prefer deriving forms, menus and command grammars from the declarations.
- It shows the state the core returns, never predicting or recomputing it; previews come from the core. Its own state is presentation state: selection, drafts, layout and navigation.
- Every domain capability exists in the core; interfaces expose subsets, never different meanings. Help, version output, completion and similar features are the interface's own.
- Peer interfaces never depend on, invoke or bundle one another; what two of them need moves into the core or a component named for that purpose. A client reaching a remote core through its network API uses the core's transport, not a peer.
- Adding a kind of interface is the user's decision.
- Each interface owns its accessibility: keyboard operation, semantics for assistive technology, contrast and user settings in visual interfaces; plain output when not writing to a terminal, and `NO_COLOR`, on command lines; and no meaning carried by colour alone.

### Vocabulary

The core owns the vocabulary: the names of concepts, states, actions, outcomes and refusals, and the wording that explains them in every supported language, in the forms interfaces need, such as a label, a short label and a description. Interfaces choose among those forms and may frame them, for example with an error prefix or an icon, but never rename, rephrase or add explanations. Wording about the interface itself (navigation, prompts, key hints, help layout) belongs to that interface. Code, documentation and tests use the same terms; change a term at its owner first.

### Contracts

A contract belongs to the boundary where it is observed, and is declared once there:

- Core: meaning, rules, permissions, outcome and refusal kinds, limits, canonical values, vocabulary and capability declarations.
- Command line: arguments, exit codes, use of standard output and standard error, prompts and terminal rendering.
- Network API: routes, methods, status codes, headers, media types and credential transport.
- Library facade: public types and functions, packaging and versioning.
- Visual interface: layout, navigation, controls, shortcuts, focus, theme and accessibility.
- Exchange formats other programs read (JSON, CSV, events, generated clients): the boundary that publishes them, derived from core declarations where they carry core data. Standard document formats belong to their standards.
- Storage: schema, durable formats and the integrity of stored data.
- Configuration: the component that reads it, validated when loaded.
- Diagnostics (logs, traces, metrics): the component that emits them; not a contract unless declared.

Documentation describes contracts and links to their owners; it never becomes a second owner. Keep boundaries with different compatibility, lifetime or security needs apart rather than forcing one representation across them.

The core reports outcomes in its own kinds. Each adapter maps every kind to its protocol (exit code, HTTP status, dialog) in one place, with exhaustiveness checked by the compiler or a test; the core never chooses an adapter's code. Unexpected failures are not business outcomes: fail visibly, never as a plausible result or an undeclared fallback.

### Storage

Storage is data the system keeps and reads back, such as databases, state files and journals; documents it produces for others are exchange formats.

- Storage owns the integrity that must hold for every writer and code version: types, required values, keys, uniqueness, references, ordering and revision continuity, immutability of permanent records, atomicity, and the structural preconditions of destructive changes, such as clearing only data that an erasure record covers.
- It holds no copy of policy owned elsewhere: no domain actions, roles, states, limits or wording in constraints, enumerated types, triggers, procedures or views. Queries take policy values, such as cut-off dates, from the core instead of re-encoding rules. Only a store that several programs write without a common core may own such rules, as their declared owner.
- Store canonical values, not presentation forms, unless the presented form is itself the record, such as a sent letter.
- Keep derived data (indexes, projections, caches) rebuildable from the records of truth.
- Reach storage through one component, and parameterize every query.
- Change the schema according to declared compatibility commitments; without one, replace it.

### Safeguards and tests

An independent safeguard checks a simpler consequence of a rule at a boundary with a different failure mode, such as gap-free revisions in storage or a client applying a schema derived from the owner's declarations. It never restates the rule.

Specify each rule's cases once, at its owner. Above it, test only that the rule is wired in, and test adapters for their mapping and protocol. Keep end-to-end tests for integration paths.

## Design and representations

Scale review to risk, not line count; routine low-risk edits need a sanity check. For a consequential change, establish requirements, alternatives, acceptance criteria and downstream effects, then challenge the approach in a separate pass against evidence and counterexamples before building. Check producer and consumer contracts early, against requirements and actual outputs where available. Revise invalidated choices and dependent work, and do not defer prerequisites past the check meant to verify them. A formal design document is needed only when useful or requested.

Prefer representations and APIs that prevent invalid states and expose incomplete handling. Keep validated structure through internal processing, and avoid needless serialization and reparsing. Conversions must keep the meaning and guarantees the receiving contract requires.

Enforce state-dependent rules and permissions against the state used for the action; an earlier check or offered action is not enough if that state can change.

## Language and locale

- Machine contracts (identifiers, tokens, protocol values, file and exchange formats, log keys, exit codes) never vary with locale; handle them with locale-invariant operations whatever the host's locale.
- Store and exchange canonical values: UTC instants; a local date and time with a time zone identifier for future civil times; plain dates; exact decimals or integer minor units with a currency code for money, never binary floating point; and BCP 47 language tags. Localize only when rendering.
- Locale is an explicit input to rendering, with a documented fallback chain; core decisions never depend on it.
- Messages are whole sentences with named parameters, never assembled from fragments. Use the language's plural rules, and its gender and case rules where they apply, never a numeric comparison. Keep messages separate where meaning or grammar differs, even when the source text is identical.
- Format numbers, dates, money and lists, and sort for display, with locale-aware libraries; compare and sort for identity with ordinal operations.
- Text is Unicode, UTF-8 at boundaries, normalized where comparison or identity matters, and truncated for display only at grapheme boundaries.
- Leave room for longer translations, isolate the direction of embedded user text, and support right-to-left layout where a supported language needs it.
- Keep one source language. Each translation records the source text it translates, so stale and missing entries are detectable; fallbacks are explicit and reported by checks. With several languages, a glossary fixes the core terms in each.
- Checks establish structure (placeholders, plural forms, glossary terms), not linguistic quality. Mark machine- or agent-made translations until a fluent reviewer approves them.
- Never translate user content, identifiers or protocol values.

## Security and privacy

- Treat input crossing a trust boundary as untrusted until validated there. Encode output for its destination: query parameters, argument vectors instead of shell strings, and context-aware HTML escaping.
- Give processes, tokens and accounts the least privilege they need.
- Keep secrets out of source, logs, URLs, command lines, fixtures, examples and your output.
- Collect and keep only the personal data a task needs. Keep real personal data out of logs, tests, examples, commit messages and issues; use synthetic data instead.
- Defend against the project's documented threat model, changing it before adding defences outside it; without one, name the threat each defence addresses.

## Naming

Name files, directories and identifiers for responsibility, behaviour or domain meaning, following project idioms. Use the core's terms, keep terminology consistent within a context, and distinguish contexts explicitly.

Examine broad names such as `utils`, `manager` or `data` when they hide responsibility. Clarify meaning before renaming; refactor only where warranted.

Do not name things merely for implementation generations, task provenance, development status or unsupported superiority. `new`, `legacy`, `v2`, `temp` and `improved` warrant examination, not automatic rejection: prefer `StreamingParser` and `BufferedParser` to `Parser` and `ImprovedParser`. States, versions, ordering and lifetimes are legitimate when they describe behaviour, a domain concept, a contract or an artifact: `FinalInvoice`, `api/v2` and `TemporaryDirectory` can be precise. A public versioned name may remain after older versions retire, and historical records may name the work they document.

Review affected names, including prose. Update references within scope while respecting external conventions, persisted formats and declared compatibility commitments.

## Checks and verification

Match claims to evidence: the paths, states and conditions actually exercised. Keep uncertainty visible until evidence resolves it. Parsing does not establish semantic validity, mocks do not establish integration, and a checksum identifies what was reviewed, not how well.

- Test required behaviour, boundaries, interactions and relevant failures, deriving acceptance from governing requirements; existing behaviour alone does not define what must be preserved. Show with regression cases or negative controls that a check detects its intended defect, not merely that it runs or fails for an unrelated reason.
- Keep test expectations independent of the logic under test; shared generation gives consistency, not independent proof.
- When replacing a mechanism, verify that checks and consumers exercise the replacement; the retired path's results prove nothing about it.
- Enforce each rule with the strongest mechanism cheap enough: types and visibility, the build's dependency graph, static analysis, tests, then review and documentation. Remove a check once a stronger mechanism is confirmed to cover its property, unless a different failure mode justifies keeping it. Add no checks merely to appease other checks or inflate coverage.
- Every gate, such as a lint rule, architecture rule or CI check, names the property it protects and has a case it must reject.
- A rule applies to existing code from adoption: fix or remove violations rather than allowlisting or grandfathering them. A rule's declared scope may exclude code it does not govern, such as generated or third-party code. Exceptions must be authorized, explicit, narrow and justified, and name the rule they suspend.
- Fix real defects, and correct invalid checks against independently established requirements; never lower required assurance just to pass.

Follow each change through callers, configuration, lockfiles, generated files, build and CI rules, documentation, tests and deliverables; update in-scope dependents and identify external coordination needed. Combine search with build or runtime evidence, accounting for tool exclusions and dynamic or external consumers; an empty search alone proves nothing. Keep valid historical, compatibility and rejection-test references.

- Inspect meaningful state changes or related batches. Record material outcomes and keep relevant failure evidence; a later success must not hide an earlier failure.
- Verify delivered contents and required check inputs separately, including test discovery, skipped tests, packaging and unexpectedly empty results; expected quiet output is not a failure.
- Diagnose from full relevant failure output, including task-relevant pre-existing failures. Test corrections narrowly where feasible; give a full rerun a purpose, bound retries and reassess recurring failures. Fix in-scope issues and report unresolved ones.
- Reuse evidence or build outputs only while their inputs, conditions and purpose still apply, never to bypass required independent or clean verification.
- Run outstanding required verification where authorized and available, preferably in an isolated, clean environment matching the required platform, toolchain and restore mode.
- Verify the delivered state. Distinguish implementation, local checks, required-environment verification and rollout. Report actual outcomes and untested conditions; claim verified readiness only with applicable passing evidence for every required check.

## Documentation and setup

Describe supported behaviour, assumptions and reasons in documentation, comments, logs, errors and test descriptions, distinct from proposals and history. Keep history where it explains a current constraint or migration. Keep guidance useful to its readers: update affected documentation, examples and active instructions with the behaviour, and identify superseded guidance you keep. Prefer links, generation or consistency checks to duplicated reference facts; check affected links and generated blocks proportionately, without building documentation machinery for this rule.

Document minimal setup from a clean checkout, its prerequisites and actionable errors. Reuse commands rather than mandating wrappers or doctor tools. Declare toolchain versions authoritatively, and derive or check necessary copies. Pin reproducible build inputs, distinguishing those pins from supported dependency ranges and deliberate compatibility probes.

## Changelog rules

This section applies when the project keeps `CHANGELOG.md`. The changelog is authoritative for release summaries, and this section for how to write them. Derive published summaries from the changelog and leave unrelated history unchanged.

`Unreleased` summarizes implemented changes intended for the next release, including relevant uncommitted work, cumulatively against the preceding published non-prerelease version; the first release describes implemented scope. Verify actual publication rather than inferring it from headings, tags or workflow status; public prereleases do not change that reference version. Check claims and material omissions against implementation, contracts, tests and documentation; commits and plans are leads. Describe consequences for users, operators, integrators or contributors, preserving material compatibility effects, limitations and required action, including for prerelease users. Exclude development chronology, superseded changes and verification logs.

Use concise bullets without category labels or subheadings, grouping related changes and putting breaking changes and required actions first. Each bullet occupies one physical line, at most 200 characters including Markdown but excluding its `- ` marker: a ceiling, not a target. Shorten or split without losing material consequences. Use technical names when helpful; link to existing relevant documentation, without assuming paths or creating documents merely to satisfy this rule. Keep `## [Unreleased]` above dated `## [version] - YYYY-MM-DD` headings, newest first; retain existing release-note links. Review accuracy, completeness, duplication, compatibility and format; report unresolved evidence gaps outside `CHANGELOG.md` and run applicable existing checks. Versioning and dating require release authorization.

## Working alongside others

Before expensive work, check prerequisites and prefer the cheapest check that can resolve the uncertainty.

When delegating, define responsibilities, decision authority, shared-resource boundaries and acceptance evidence. Pass on material uncertainty, and distinguish requirements from suggested methods. Parallelize only when authorized and useful, coordinate changes to shared inputs, and verify the integrated result.

Use exclusive ownership, isolation or supported concurrency for mutable resources. Do not change a check's inputs while it runs, except in watch workflows built for that. Avoid duplicate setup, and investigate resource growth you cannot explain.

Report substantive progress, blockers and the next decisive check, not activity for its own sake.

Preserve unrelated work. Clean up temporary resources you created for the task, but keep deliverables and useful failure evidence, and leave requested services running. Keep output focused.
