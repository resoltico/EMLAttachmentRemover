//! Bounds/reproduction controls are owned here, not compiled twice in the runners.
#[path = "support/property_config.rs"]
mod property_config;
use proptest::test_runner::Config;
use std::{fs, process::Command};

#[test]
fn inherited_shrink_limits_remain_finite_and_lower_requests_are_preserved() {
    if let Ok(mode) = std::env::var("EML_PROPERTY_CONFIG_PROBE") {
        let config = Config::default();
        if mode == "high" {
            assert_eq!(config.max_shrink_iters, u32::MAX - 1);
            assert_eq!(config.max_shrink_time, u32::MAX);
        }
        let config = property_config::bounded(
            config,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/proptest/config-probe.txt"
            ),
        );
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
            "{mode}: {}",
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
