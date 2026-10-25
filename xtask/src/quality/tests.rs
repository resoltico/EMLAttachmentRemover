//! Negative controls exercise the policy boundary rather than its implementation inventory.
use super::{Exception, Policy};
use std::collections::{BTreeMap, BTreeSet};

fn policy() -> Policy {
    Policy {
        schema: 1,
        max_source_lines: 1000,
        vendor: Vec::new(),
        command_exceptions: Vec::new(),
        exceptions: vec![Exception {
            id: "native".into(),
            file: "fixture.rs".into(),
            scope: "crate/fn:boundary".into(),
            lints: vec!["unsafe_code".into()],
            rationale: "Only this fixture's explicit native boundary.".into(),
        }],
    }
}
fn inspect(source: &str, policy: &Policy) -> crate::Result<BTreeMap<String, usize>> {
    let root = tempfile::tempdir()?;
    let mut matched = BTreeMap::new();
    super::rust::check(
        root.path(),
        "fixture.rs",
        source,
        &BTreeSet::from(["fixture.rs".into()]),
        policy,
        &mut matched,
    )?;
    Ok(matched)
}
#[test]
fn scoped_registered_boundary_is_accepted_once() {
    let source = "#[allow(unsafe_code, reason = \"quality:native\")] fn boundary() {}";
    assert_eq!(inspect(source, &policy()).unwrap().get("native"), Some(&1));
}
#[test]
fn unregistered_missing_reason_changed_scope_and_lint_are_rejected() {
    for source in [
        "#[allow(unsafe_code)] fn boundary() {}",
        "#[allow(unsafe_code, reason = \"quality:unknown\")] fn boundary() {}",
        "#[allow(unsafe_code, reason = \"quality:native\")] fn widened() {}",
        "#[allow(dead_code, reason = \"quality:native\")] fn boundary() {}",
        "#[allow(unsafe_code, dead_code, reason = \"quality:native\")] fn boundary() {}",
        "#[warn(unsafe_code, reason = \"quality:unknown\")] fn boundary() {}",
    ] {
        assert!(inspect(source, &policy()).is_err(), "Accepted {source}");
    }
}
#[test]
fn conditional_inner_and_macro_hidden_suppressions_are_rejected() {
    for source in [
        "#[cfg_attr(unix, allow(unsafe_code, reason = \"quality:native\"))] fn boundary() {}",
        "#![allow(unsafe_code, reason = \"quality:native\")] fn boundary() {}",
        "macro_rules! hidden { () => { #[allow(unsafe_code, reason = \"quality:native\")] fn boundary() {} }; }",
        "#[rustfmt::skip] fn boundary() {}",
    ] {
        assert!(inspect(source, &policy()).is_err(), "Accepted {source}");
    }
}
#[test]
fn strings_and_comments_cannot_create_fake_suppressions() {
    let source = "// #[allow(unsafe_code)]\nfn boundary() { let _s = \"#[allow(unsafe_code)]\"; }";
    assert!(inspect(source, &policy()).unwrap().is_empty());
}
#[test]
fn production_command_cannot_cap_compiler_lints() {
    let source =
        "fn boundary() { std::process::Command::new(\"cargo\").args([\"--cap-lints=allow\"]); }";
    assert!(inspect(source, &policy()).is_err());
}
#[test]
fn explicit_mutation_experiment_is_the_only_matching_cap_scope() {
    let mut policy = policy();
    policy.command_exceptions.push(super::CommandException {
        id: "mutation".into(),
        file: "fixture.rs".into(),
        scope: "crate/fn:boundary".into(),
        argument: "--cap-lints=true".into(),
        rationale: "Mutant-generated warnings are not product lint results.".into(),
    });
    let source = "fn boundary() { std::process::Command::new(\"cargo-mutants\").args([\"--cap-lints=true\"]); }";
    assert_eq!(inspect(source, &policy).unwrap().get("mutation"), Some(&1));
    assert!(inspect(&source.replace("boundary", "other"), &policy).is_err());
}
#[test]
fn shell_and_swift_disable_directives_are_rejected() {
    assert!(
        super::policy::check_directives("fixture.sh", "#!/bin/sh\n# shellcheck disable=SC2086\n")
            .is_err()
    );
    assert!(
        super::policy::check_directives("fixture.swift", "// swift-format-ignore\nfunc f() {}")
            .is_err()
    );
}
#[test]
fn ignored_or_external_include_source_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("fixture.rs");
    std::fs::write(&source, "").unwrap();
    let item: syn::ItemMod = syn::parse_str("#[path = \"fixture.rs\"] mod fixture;").unwrap();
    assert!(
        super::references::check(root.path(), "fixture.rs", &BTreeSet::new(), &item, &[]).is_err()
    );
}

#[test]
fn stale_and_duplicated_bindings_are_rejected() {
    let policy = policy();
    assert_eq!(
        super::unbound_exceptions(&policy, &BTreeMap::new()).len(),
        1
    );
    assert_eq!(
        super::unbound_exceptions(&policy, &BTreeMap::from([("native".into(), 2)])).len(),
        1
    );
    assert!(super::unbound_exceptions(&policy, &BTreeMap::from([("native".into(), 1)])).is_empty());
}
#[test]
fn package_lints_cannot_bypass_inheritance_or_weaken_standalone_rules() {
    let authority: toml::Value = toml::from_str("[rust]\nwarnings='deny'\n").unwrap();
    let good: toml::Value = toml::from_str("[lints]\nworkspace=true\n").unwrap();
    let bad: toml::Value = toml::from_str("[lints.rust]\nwarnings='allow'\n").unwrap();
    assert!(super::policy::lint_table("Cargo.toml", &good, &authority, true).is_ok());
    assert!(super::policy::lint_table("Cargo.toml", &bad, &authority, true).is_err());
    assert!(super::policy::lint_table("Cargo.toml", &bad, &authority, false).is_err());
}

#[test]
fn workflow_and_cargo_config_cannot_weaken_lint_commands() {
    for source in [
        "cargo clippy -- --cap-lints=allow",
        "cargo clippy -- -A warnings",
        "rustflags=['--allow=warnings']",
        "actionlint -ignore warning",
        "shellcheck --exclude SC2086 script.sh",
    ] {
        assert!(
            super::policy::check_tool_options("fixture", source).is_err(),
            "Accepted {source}"
        );
    }
    assert!(super::policy::check_tool_options("fixture", "cargo clippy -- -D warnings").is_ok());
}

#[test]
fn duplicate_registry_identifiers_are_rejected_before_source_binding() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("fixture.rs"), "").unwrap();
    let mut records = policy();
    records.exceptions.push(policy().exceptions.remove(0));
    let error = super::policy::validate(root.path(), &records).unwrap_err();
    assert!(error.to_string().contains("duplicate"));
}
#[test]
fn root_manifest_cannot_hide_a_blanket_lint_exception() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("fixture.rs"), "").unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace.lints.rust]\nwarnings='allow'\n",
    )
    .unwrap();
    let error = super::policy::validate(root.path(), &policy()).unwrap_err();
    assert!(error.to_string().contains("not strict"));
}
