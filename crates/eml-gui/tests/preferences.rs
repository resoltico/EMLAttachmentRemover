//! Fresh-process Foundation String precedence, persistence and owned-suite cleanup.
#![cfg(all(
    target_os = "macos",
    feature = "macos-gui",
    feature = "native-fixtures"
))]
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{ffi::OsStr, path::Path, time::Duration};
#[path = "../../../tests/support/native_child.rs"]
mod native_child;
const BIN: &str = env!("CARGO_BIN_EXE_macos-preference-observation");
const ABSENT: &str = "has_saved_keys:false\nowner:false\ncount:0\nkind:none\nvalue:\n";
fn expected_snapshot(kind: &str, value: &str) -> String {
    format!(
        "has_saved_keys:true\nowner:true\ncount:2\nkind:{kind}\nvalue:{}\n",
        STANDARD.encode(value)
    )
}
struct Suite {
    name: String,
    marker: String,
    cleaned: bool,
    _directory: tempfile::TempDir,
}
impl Suite {
    fn new() -> Self {
        // Exclusive creation, retained until preference cleanup completes.
        // Wall-clock resolution is not a namespace allocation primitive.
        let directory = tempfile::Builder::new()
            .prefix("eml-preference-observation-")
            .tempdir()
            .unwrap();
        let unique = directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        Self {
            name: format!("io.github.resoltico.eml-preference-observation.{unique}"),
            marker: unique,
            cleaned: false,
            _directory: directory,
        }
    }
    fn run(&self, action: &str, extra: &[&str]) -> String {
        let mut arguments = vec![
            OsStr::new("--owned-suite"),
            OsStr::new(&self.name),
            OsStr::new("--marker"),
            OsStr::new(&self.marker),
            OsStr::new("--action"),
            OsStr::new(action),
        ];
        arguments.extend(extra.iter().map(OsStr::new));
        let observed = eml_macos::spawn(Path::new(BIN), &arguments).unwrap();
        let child = observed.child.map(native_child::Settle).unwrap();
        assert!(observed.issues.is_empty(), "{:?}", observed.issues);
        let output = native_child::drain(observed.pipes.output);
        let errors = native_child::drain(observed.pipes.errors);
        eml_macos::close_writer(observed.pipes.input).unwrap();
        let status = native_child::terminal(&child.0);
        let output = output.recv_timeout(Duration::from_secs(2)).unwrap();
        let diagnostics = errors.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            status,
            eml_macos::ChildExit {
                code: Some(0),
                signal: None
            },
            "{}",
            String::from_utf8_lossy(&diagnostics)
        );
        assert!(diagnostics.is_empty());
        let output = String::from_utf8(output).unwrap();
        let prefix = format!("scope:{}:{}\n", self.name, self.marker);
        assert!(
            output.starts_with(&prefix),
            "Probe scope receipt differs: expected {prefix:?}, received {output:?}"
        );
        output[prefix.len()..].to_owned()
    }
    fn cleanup(&mut self) {
        assert_eq!(self.run("cleanup", &[]), "cleanup:requested\n");
        assert_eq!(
            self.run("observe", &[]),
            ABSENT,
            "Fresh observer must confirm no saved keys remain"
        );
        self.cleaned = true;
    }
}
impl Drop for Suite {
    fn drop(&mut self) {
        if !self.cleaned
            && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.cleanup())).is_err()
        {
            eprintln!(
                "Owned preference suite cleanup failed: {}; any earlier failure remains; abort later tests",
                self.name
            );
            std::process::exit(1);
        }
    }
}
#[test]
fn argument_strings_override_seeded_values_without_persisting_later_choices() {
    let saved = STANDARD.encode(b"/owned-synthetic/saved");
    for value in [
        String::new(),
        STANDARD.encode(b"/owned-synthetic/\xff copies"),
        "not base64".into(),
    ] {
        let mut suite = Suite::new();
        let before = expected_snapshot("string", &saved);
        assert_eq!(suite.run("seed-string", &["--value", &saved]), before);
        assert_eq!(
            suite.run("observe", &[]),
            before,
            "Fresh process reads the seeded String"
        );
        let destination = if value.is_empty() {
            "beside"
        } else if value == "not base64" {
            "invalid"
        } else {
            &value
        };
        assert_eq!(
            suite.run("choose", &["-copyDestinationBytes", &value]),
            format!(
                "before-load\n{before}after-load\n{before}session-only:true\ndestination:{destination}\nafter-choice\n{before}"
            )
        );
        assert_eq!(
            suite.run("observe", &[]),
            before,
            "Fresh process must still see the original saved String"
        );
        suite.cleanup();
    }
}
#[test]
fn saved_string_and_non_string_reads_share_the_decoder_and_explicit_choice_persists() {
    let saved = STANDARD.encode(b"/owned-synthetic/saved-\xff");
    let changed = STANDARD.encode(b"/owned-synthetic/changed-\xff");
    for kind in ["string", "number"] {
        let mut suite = Suite::new();
        let before = expected_snapshot(kind, if kind == "number" { "7" } else { &saved });
        assert_eq!(
            suite.run(
                if kind == "number" {
                    "seed-number"
                } else {
                    "seed-string"
                },
                &["--value", &saved]
            ),
            before
        );
        assert_eq!(suite.run("observe", &[]), before);
        let after = expected_snapshot("string", &changed);
        let destination = if kind == "number" { "invalid" } else { &saved };
        assert_eq!(
            suite.run("choose", &[]),
            format!(
                "before-load\n{before}after-load\n{before}session-only:false\ndestination:{destination}\nafter-choice\n{after}"
            )
        );
        assert_eq!(
            suite.run("observe", &[]),
            after,
            "Fresh process observes actual saved native bytes; no reboot durability claim"
        );
        suite.cleanup();
    }
}
