//! The pinned native compiler supplies its own matching SwiftSyntax host modules.
use crate::{Result, controller, macos::MacBuild};
use std::{collections::BTreeSet, fs, path::Path, process::Command, time::Duration};

pub(super) fn check(
    root: &Path,
    out: &Path,
    build: &MacBuild,
    paths: &BTreeSet<&str>,
) -> Result<serde_json::Value> {
    let host = host_modules(&build.swiftc)?;
    let tool = out.join("SourcePolicy");
    let mut compile = build.command(root, &build.swiftc);
    compile
        .args([
            "-parse-as-library",
            "-swift-version",
            "6",
            "-warnings-as-errors",
            "-I",
        ])
        .arg(&host)
        .arg("-L")
        .arg(&host)
        .args(["-Xlinker", "-rpath", "-Xlinker"])
        .arg(&host)
        .arg(root.join("xtask/swift/SourcePolicy.swift"))
        .arg("-o")
        .arg(&tool);
    let log = out.join("swift-structure-compile.log");
    controller()?
        .run(compile, &log, Duration::from_secs(120))?
        .require_success(&log)?;
    let clippy: toml::Value = toml::from_str(&fs::read_to_string(root.join("clippy.toml"))?)?;
    let limits = [
        "too-many-lines-threshold",
        "cognitive-complexity-threshold",
        "too-many-arguments-threshold",
    ]
    .map(|key| {
        clippy[key]
            .as_integer()
            .ok_or_else(|| "Missing structural limit".into())
    })
    .into_iter()
    .collect::<Result<Vec<_>>>()?;
    let mut command = Command::new(&tool);
    command
        .current_dir(root)
        .args(limits.iter().map(ToString::to_string))
        .args(paths);
    let log = out.join("swift-structure.log");
    let process = controller()?.run(command, &log, Duration::from_secs(120))?;
    process.require_success(&log)?;
    let negative_controls = controls(root, out, &tool)?;
    Ok(
        serde_json::json!({"passed":true,"sources":paths.len(),"host_modules":host,
        "limits":{"code_lines":limits[0],"branch_complexity":limits[1],"parameters":limits[2]},
        "negative_controls":negative_controls,"scope":"syntax-aware callable code lines and explicitly defined branch complexity, not Clippy cognitive complexity; closures/accessors/nested declarations checked independently"}),
    )
}

fn controls(root: &Path, out: &Path, tool: &Path) -> Result<usize> {
    let fixtures = [
        (
            "comments-and-strings",
            "func f() {\n// if if if if\nlet s = \"if && while\"\n/* if\nwhile */\nprint(s)\n}\n",
            true,
            [4, 1, 7],
        ),
        (
            "nested-callables",
            "func f() { func inner() { if true {} }; let c = { if true {} }; print(c); inner() }",
            true,
            [100, 2, 7],
        ),
        (
            "branch-failure",
            "func f(_ a: Bool, _ b: Bool) { if a && b {} }",
            false,
            [100, 2, 7],
        ),
        (
            "switch-failure",
            "func f(_ x: Int) { switch x { case 0: break; case 1: break; default: break } }",
            false,
            [100, 2, 7],
        ),
        (
            "line-failure",
            "func f() {\nprint(1)\nprint(2)\nprint(3)\n}",
            false,
            [2, 25, 7],
        ),
        (
            "parameter-failure",
            "func f(_ a: Int, _ b: Int) {}",
            false,
            [100, 25, 1],
        ),
        (
            "accessor-failure",
            "struct A { var x: Int { if true { return 1 }; return 0 } }",
            false,
            [100, 1, 7],
        ),
        ("malformed-failure", "func f( {", false, [100, 25, 7]),
        (
            "comma-condition-failure",
            "func f(_ a: Bool, _ b: Bool) { if a, b {} }",
            false,
            [100, 2, 7],
        ),
        (
            "formatter-suppression-failure",
            "// swift-format-ignore\nfunc f() {}",
            false,
            [100, 25, 7],
        ),
        (
            "formatter-word-in-string",
            "func f() { print(\"swift-format-ignore\") }",
            true,
            [100, 25, 7],
        ),
    ];
    for (name, source, passes, limits) in fixtures {
        let path = out.join(format!("{name}.swift"));
        fs::write(&path, source)?;
        let mut command = Command::new(tool);
        command
            .current_dir(root)
            .args(limits.map(|limit| limit.to_string()))
            .arg(path);
        let log = out.join(format!("swift-control-{name}.log"));
        let process = controller()?.run(command, &log, Duration::from_secs(30))?;
        if process.success() != passes {
            return Err(format!(
                "Swift structural control {name} failed its expected {} outcome; retained {}",
                if passes { "acceptance" } else { "rejection" },
                log.display()
            )
            .into());
        }
    }
    Ok(fixtures.len())
}

pub(super) fn host_modules(compiler: &Path) -> Result<std::path::PathBuf> {
    let host = compiler
        .parent()
        .and_then(Path::parent)
        .ok_or("Swift compiler has no toolchain parent")?
        .join("lib/swift/host");
    for module in ["SwiftParser.swiftmodule", "SwiftSyntax.swiftmodule"] {
        if !host.join(module).is_dir() {
            return Err(format!(
                "Selected Xcode lacks required {module} at {}; Swift structure is unverified",
                host.display()
            )
            .into());
        }
    }
    Ok(host)
}
