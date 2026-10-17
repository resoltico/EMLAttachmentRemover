# Repository tool build inputs

The top-level tool versions remain authoritative in ../tools.toml. These complete
Cargo lockfiles select compatible maintained dependencies for those unchanged
published tool sources. They include dev/platform edges. They are not product
lockfiles or permission to ship tool dependencies in CLI/app archives.

The published locks fail current advisory checks. Compatible replacements update
rustls/webpki in cargo-deny/about, anyhow/crossbeam-epoch in cargo-fuzz/mutants,
and nutmeg/insta in mutants. The latter parent updates remove the atty and
serde_yaml/yaml-rust branches. The original unsoundness/vulnerability and
unmaintained-package verdicts remain recorded in the implementation evidence;
no advisory is ignored and no original published-lock pass is claimed.

build-inputs.json binds the unchanged published crate, manifest and original lock
to each selected build lock. Independent input review checked complete source
trees, compatible manifest ranges, precise lock changes and all new registry
checksums. Hashes identify those reviewed inputs; they do not establish review
quality or execution. All four candidate graphs passed actual advisory/source
checks. The local Mac ARM locked rebuild, owned binary identity and actual consumer
checks subsequently passed their scoped gates; complete source/native/app
qualification remains separate. See ../../docs/v6-implementation-evidence.md.

Repository dependency policy also admits the actual tooling license requirements
ISC (ring/untrusted/webpki), Zlib (foldhash), Apache 2.0 WITH LLVM-exception
(target-lexicon), and CDLA-Permissive 2.0 (webpki root certificate data), based on
the exact upstream package texts. libfuzzer-sys 0.4.13 has a narrow NCSA exception
for its declared vendored libfuzzer. Preserve required notices when redistributing
any such tool/data; these build tools are not v6 release assets. Shipped normal
dependency materials retain their separate reviewed target closure and stricter
notice configuration. This repository policy is not a legal certification.
