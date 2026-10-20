//! Shared command handlers retain their output and clean-source contracts.
use crate::{Result, cli_build, git, native_host, notices, qualify, smoke};
use std::path::Path;

pub(super) fn print_json(result: Result<serde_json::Value>) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&result?)?);
    Ok(())
}
pub(super) fn native_tests(root: &Path, target: &str, out: &Path, release: bool) -> Result<()> {
    let host = native_host::inspect_target(root, target)?;
    std::fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let mut result = qualify::checked_tests(root, Some(target), release, &out)?;
    result["host"] = host;
    std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
pub(super) fn build_cli_archive(root: &Path, target: &str, out: &Path) -> Result<()> {
    if !git(root, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty() {
        return Err("CLI archives require a clean committed source checkout".into());
    }
    std::fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let cli = cli_build::build(root, target, &out.join("cli-build"))?;
    let materials = notices::checked_target(root, cli.platform())?;
    let archive = qualify::staged_cli(root, &cli, &materials, &out)?;
    std::fs::write(
        out.join("cli-result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"source":git(root, &["rev-parse", "HEAD"])?,
            "target":cli.target(),"platform":cli.platform(),"version":cli.version(),
            "host":cli.host(),"frozen_image":cli.identity(),"archive":archive,
            "scope":"native CLI build and archive readback only; full qualification remains separate"}),
        )?,
    )?;
    Ok(())
}
pub(super) fn cli_smoke(root: &Path, executable: &Path, out: &Path) -> Result<()> {
    std::fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let owned = xtask::archive::create_artifact_directory(&out)?;
    let result = smoke::run(
        root,
        &executable.canonicalize()?,
        "remove-eml-attachments",
        env!("CARGO_PKG_VERSION"),
        owned.path(),
        &out.join("behavior"),
    )?;
    std::fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
pub(super) fn notices(root: &Path, out: Option<&Path>, check: bool) -> Result<()> {
    if check {
        print_json(notices::check(root, out))
    } else {
        notices::run(
            root,
            &out.map(Path::to_owned)
                .unwrap_or_else(|| root.join("target/generated-legal-materials")),
        )
    }
}
