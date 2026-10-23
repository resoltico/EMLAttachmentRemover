//! Dependency assurance over the declared authored graphs and desktop products.
use crate::{
    Result,
    distribution::{BuildPlan, ProductRole},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Serialize)]
struct Graph {
    name: String,
    manifest: PathBuf,
    target: Option<String>,
    features: Vec<&'static str>,
}
fn graphs(root: &Path, plan: &BuildPlan) -> Result<Vec<Graph>> {
    let mut graphs = vec![Graph {
        name: "workspace".into(),
        manifest: root.join("Cargo.toml"),
        target: None,
        features: Vec::new(),
    }];
    for manifest in crate::repository_config::standalone_manifests(root)? {
        graphs.push(Graph {
            name: format!("standalone-{}", manifest.replace('/', "-")),
            manifest: root.join(manifest),
            target: None,
            features: Vec::new(),
        });
    }
    for target in plan
        .targets()
        .filter(|target| target.products().contains(&ProductRole::DesktopApp))
    {
        graphs.push(Graph {
            name: format!("desktop-{}", target.platform()),
            manifest: root.join("Cargo.toml"),
            target: Some(target.rust_target().to_owned()),
            features: vec!["eml-gui/macos-gui"],
        });
    }
    Ok(graphs)
}
pub(crate) fn run(root: &Path, plan: &BuildPlan, out: &Path) -> Result<Value> {
    crate::campaign::require_stock_topology(root)?;
    let graphs = graphs(root, plan)?;
    let expected = plan.tools().version("cargo_deny")?;
    let executable = crate::tools::path_pinned(root, "deny", expected)?;
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    let mut results = Vec::new();
    let mut passed = true;
    for graph in graphs {
        let log = out.join(format!("{}.log", graph.name));
        let mut command = std::process::Command::new(&executable);
        command
            .current_dir(root)
            .arg("deny")
            .args(["--locked", "--manifest-path"])
            .arg(&graph.manifest)
            .env("CARGO_HOME", crate::campaign::cargo_home()?)
            .env("RUSTUP_TOOLCHAIN", crate::tools::compiler(root)?);
        if let Some(target) = &graph.target {
            command.args(["--target", target]);
        }
        if !graph.features.is_empty() {
            command.arg("--features").arg(graph.features.join(","));
        }
        command
            .arg("--config")
            .arg(root.join("xtask/dependencies.toml"))
            .args(["check", "advisories", "sources", "licenses"]);
        let result = match crate::controller()?.run(command, &log, Duration::from_secs(600)) {
            Ok(process) => {
                passed &= process.success();
                json!({"graph":graph,"process":process.evidence(),"log":log,"passed":process.success()})
            }
            Err(error) => {
                passed = false;
                json!({"graph":graph,"log":log,"passed":false,"error":error.to_string()})
            }
        };
        results.push(result);
    }
    let result = json!({"schema":"eml-dependency-check","schema_version":1,"version":plan.version(),"cargo_deny":expected,"graphs":results,"passed":passed,"scope":"actual advisory/source/license results for every declared authored and desktop graph; no native product qualification"});
    fs::write(out.join("result.json"), serde_json::to_vec_pretty(&result)?)?;
    if !passed {
        return Err(format!(
            "Dependency graph checking failed; inspect {}",
            out.display()
        )
        .into());
    }
    Ok(result)
}
