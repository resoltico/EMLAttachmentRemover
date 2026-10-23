//! Bounds/reproduction controls are owned here, not compiled twice in the runners.
#[path = "support/property_config.rs"]
mod property_config;
use proptest::test_runner::Config;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::LazyLock,
};
static CONFIG_SEEDS: LazyLock<String> = LazyLock::new(|| {
    property_config::seed_path(Path::new(env!("CARGO_MANIFEST_DIR")), "config-probe.txt")
});

#[test]
fn inherited_shrink_limits_remain_finite_and_lower_requests_are_preserved() {
    if let Ok(mode) = std::env::var("EML_PROPERTY_CONFIG_PROBE") {
        let config = Config::default();
        if mode == "high" {
            assert_eq!(config.max_shrink_iters, u32::MAX - 1);
            assert_eq!(config.max_shrink_time, u32::MAX);
        }
        let config = property_config::bounded(config, &CONFIG_SEEDS);
        let (iterations, time) = if mode == "high" { (4096, 2000) } else { (7, 9) };
        assert_eq!(config.max_shrink_iters, iterations);
        assert_eq!(config.max_shrink_time, time);
        return;
    }
    for (mode, iterations, time) in [("high", 4294967294u32, u32::MAX), ("low", 7, 9)] {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "inherited_shrink_limits_remain_finite_and_lower_requests_are_preserved",
            ])
            .env("EML_PROPERTY_CONFIG_PROBE", mode)
            .env("PROPTEST_MAX_SHRINK_ITERS", iterations.to_string())
            .env("PROPTEST_MAX_SHRINK_TIME", time.to_string())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{mode}: status={}; stdout:\n{}\nstderr:\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed; 0 failed"));
    }
}

#[test]
fn replay_admission_preserves_valid_seeds_and_refuses_growth_or_malformed_records() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("seeds.txt");
    let seed = format!("cc {}", "0".repeat(64));
    let valid = format!(
        "# independent synthetic reproduction seed, not a known product failure\n{seed} # shrinks to a bounded model\n"
    );
    fs::write(&path, &valid).unwrap();
    property_config::check_seed_file(&path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), valid);
    for invalid in [
        "invalid seed".to_owned(),
        format!("cc {}", "A".repeat(64)),
        format!("{}\n", seed).repeat(4097),
        "#".repeat(1024 * 1024 + 1),
    ] {
        fs::write(&path, &invalid).unwrap();
        assert!(property_config::check_seed_file(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    }
    assert!(property_config::check_seed_file(root.path()).is_err());
    assert!(property_config::check_seed_file(&root.path().join("not-created.txt")).is_ok());
}

#[test]
fn canonical_native_seed_path_supports_standard_missing_write_and_replay() {
    use proptest::test_runner::PersistedSeed;
    // The persistence API borrows a static str. One owned fixture and one
    // immutable path provide that lifetime without leaking per configuration.
    static ROOT: LazyLock<PathBuf> =
        LazyLock::new(|| tempfile::tempdir().unwrap().keep().canonicalize().unwrap());
    static SEEDS: LazyLock<String> = LazyLock::new(|| {
        property_config::seed_path(&ROOT.join("crates").join("eml-core"), "native-control.txt")
    });
    let path = Path::new(&*SEEDS);
    let expected = ROOT
        .join("target")
        .join("proptest")
        .join("native-control.txt");
    assert_eq!(path, expected);
    assert!(path.is_absolute());
    assert!(
        path.components()
            .all(|c| !matches!(c, std::path::Component::ParentDir))
    );
    #[cfg(windows)]
    assert!(
        matches!(path.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(),std::path::Prefix::VerbatimDisk(_)|std::path::Prefix::VerbatimUNC(_, _)))
    );
    assert!(!path.exists());
    property_config::check_seed_file(path).unwrap();
    let mut config = property_config::bounded(Config::default(), &SEEDS);
    let seed: PersistedSeed = format!("cc {}", "0".repeat(64)).parse().unwrap();
    let persistence = config.failure_persistence.as_mut().unwrap();
    assert!(persistence.load_persisted_failures2(None).is_empty());
    persistence.save_persisted_failure2(
        None,
        seed.clone(),
        &"independent native persistence control",
    );
    let written = fs::read(path).expect("Standard persistence must create the exact native path");
    assert!(String::from_utf8_lossy(&written).contains(&seed.to_string()));
    assert_eq!(persistence.load_persisted_failures2(None), vec![seed]);
    property_config::check_seed_file(path).unwrap();
    assert_eq!(fs::read(path).unwrap(), written);
    fs::write(path, "invalid seed retained on refusal").unwrap();
    assert!(property_config::check_seed_file(path).is_err());
    assert_eq!(fs::read(path).unwrap(), b"invalid seed retained on refusal");
    fs::remove_dir_all(&*ROOT).unwrap();
}
