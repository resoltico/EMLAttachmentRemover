//! Exhaustive shared-label rendering at the CLI presentation boundary.
use super::*;
#[test]
fn u1_cli_renders_every_core_outcome_label() {
    let mut outcomes = vec![
        Outcome::Ready,
        Outcome::NothingToRemove,
        Outcome::CopySaved,
        Outcome::CopiedAsIs,
        Outcome::AlreadyExists,
        Outcome::WillVerify,
        Outcome::NeedsChecking,
        Outcome::Unknown,
        Outcome::NotProcessed,
    ];
    outcomes.extend(
        eml_core::ErrorCode::ALL
            .iter()
            .copied()
            .map(Outcome::Failed),
    );
    for outcome in outcomes {
        assert!(
            outcome.label().chars().count() <= LABEL_WIDTH,
            "{}",
            outcome.label()
        );
        let mut line = Vec::new();
        write_label(
            &mut line,
            outcome.label(),
            std::path::Path::new("message.eml"),
        )
        .unwrap();
        assert!(
            String::from_utf8(line)
                .unwrap()
                .starts_with(outcome.label())
        );
    }
}
