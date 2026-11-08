//! Gate input selection and failure propagation; external analyzers are stubbed.
#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts")
        .join(name)
}
fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    );
}
fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(root.path(), &["config", "user.name", "Gate Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "gate@example.invalid"],
    );
    root
}
fn put(root: &Path, name: &str, text: &str) {
    let p = root.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}
fn budget(root: &Path, reference: Option<&str>) -> Output {
    let mut cmd = Command::new("bash");
    cmd.arg(script("check-budget.sh")).current_dir(root);
    if let Some(r) = reference {
        cmd.arg(r);
    }
    cmd.output().unwrap()
}
#[test]
fn budget_uses_dirty_untracked_and_deleted_inputs_but_can_measure_a_revision() {
    let root = repository();
    let root = root.path();
    put(root, "crates/product/src/lib.rs", &"product\n".repeat(20));
    put(root, "tests/check.rs", "test\n");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "baseline"]);
    assert!(budget(root, None).status.success());
    put(root, "tests/check.rs", &"test\n".repeat(21));
    assert!(!budget(root, None).status.success());
    assert!(budget(root, Some("HEAD")).status.success());
    fs::remove_file(root.join("tests/check.rs")).unwrap();
    assert!(budget(root, None).status.success());
    put(root, "tests/untracked check.rs", &"test\n".repeat(20));
    assert!(!budget(root, None).status.success());
    fs::remove_file(root.join("tests/untracked check.rs")).unwrap();
    put(root, "scripts/check.sh", &"shell\n".repeat(20));
    assert!(!budget(root, None).status.success());
    assert!(!budget(root, Some("missing-ref")).status.success());
}
#[test]
fn shell_lint_includes_new_files_and_fails_closed_on_listing_or_analyzer_failure() {
    let root = repository();
    let root = root.path();
    put(root, "scripts/old.sh", "#!/bin/sh\n");
    git(root, &["add", "."]);
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let stub = bin.join("shellcheck");
    fs::write(
        &stub,
        "#!/bin/sh\nfor arg do case $arg in *new*) exit 42;; esac; done\n",
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
    let run = |dir: &Path| {
        Command::new("bash")
            .arg(script("check-shell.sh"))
            .current_dir(dir)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .output()
            .unwrap()
    };
    assert!(run(root).status.success());
    put(root, "scripts/new file.sh", "#!/bin/sh\n");
    assert_eq!(run(root).status.code(), Some(42));
    let outside = tempfile::tempdir().unwrap();
    assert!(!run(outside.path()).status.success());
    fs::remove_file(root.join("scripts/old.sh")).unwrap();
    fs::remove_file(root.join("scripts/new file.sh")).unwrap();
    assert!(!run(root).status.success());
}
#[test]
fn tool_pins_come_from_the_invoking_workspace_and_reject_multiline_values() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers=[\"member\"]\nresolver=\"3\"\n",
    );
    put(
        root,
        "member/Cargo.toml",
        "[package]\nname=\"tool-pin-fixture\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
    );
    put(root, "member/src/lib.rs", "");
    put(
        root,
        "xtask/tools.toml",
        "[versions]\nactionlint=\"9.8.7\"\n",
    );
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("tool-versions")
            .current_dir(root.join("member"))
            .output()
            .unwrap()
    };
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"actionlint=9.8.7\n");
    put(
        root,
        "xtask/tools.toml",
        "[versions]\nactionlint=\"9.8.7\\nother=value\"\n",
    );
    assert!(!run().status.success());
}
