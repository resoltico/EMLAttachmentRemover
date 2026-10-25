//! Bounded property shrinking and native paths for retained reproduction seeds.
use proptest::test_runner::{Config, FileFailurePersistence};
use std::path::Path;

pub(crate) fn seed_path(manifest: &Path, filename: &str) -> String {
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("Message crate lives in the workspace crates directory")
        .join("target")
        .join("proptest")
        .join(filename)
        .into_os_string()
        .into_string()
        .expect("Manifest directory and seed filename are UTF-8")
}

pub(crate) fn bounded(mut config: Config, path: &'static str) -> Config {
    config.verbose = 0;
    config.max_shrink_iters = config.max_shrink_iters().min(4096);
    config.max_shrink_time = match config.max_shrink_time {
        0 => 2000,
        lower => lower.min(2000),
    };
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(path)));
    config
}
