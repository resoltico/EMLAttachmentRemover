//! Finite failure reproduction for the two existing MIME property runners.
use proptest::test_runner::{Config, FileFailurePersistence, PersistedSeed};
use std::{io::Read, path::Path};
const MAX_REPLAY_BYTES: u64 = 1024 * 1024;
const MAX_REPLAY_SEEDS: usize = 4096;

pub(crate) fn check_seed_file(path: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Cannot inspect retained property seeds: {error}")),
    };
    if !metadata.is_file() || metadata.len() > MAX_REPLAY_BYTES {
        return Err("Retained property seeds must be a regular file <=1MiB; archive/curate it explicitly without discarding the failing evidence".to_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_REPLAY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_REPLAY_BYTES {
        return Err(
            "Retained property seeds grew past1MiB; preserve and curate before replay".to_owned(),
        );
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Retained property seeds are not UTF8")?;
    let mut count = 0;
    for line in text.lines() {
        let seed = line.split('#').next().unwrap().trim();
        if seed.is_empty() {
            continue;
        }
        let parsed: PersistedSeed = seed.parse().map_err(
            |_| "Invalid retained property seed; preserve evidence and correct it explicitly",
        )?;
        if parsed.to_string() != seed {
            return Err("Retained property seed is not canonical".to_owned());
        }
        count += 1;
        if count > MAX_REPLAY_SEEDS {
            return Err("Retained property replay exceeds4096seeds; preserve/archive and curate before running".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn bounded(mut config: Config, path: &'static str) -> Config {
    check_seed_file(Path::new(path)).unwrap_or_else(|error| panic!("{}: {error}", path));
    config.max_shrink_iters = config.max_shrink_iters().min(4096);
    config.max_shrink_time = match config.max_shrink_time {
        0 => 2000,
        lower => lower.min(2000),
    };
    // Standard Proptest writes one failing seed after shrinking. This is a
    // pre-replay bound, not a strict append quota; no seed is silently dropped.
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(path)));
    config
}
