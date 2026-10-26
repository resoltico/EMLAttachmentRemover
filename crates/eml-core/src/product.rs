//! Shared display identity for CLI, desktop and distribution metadata.
use std::sync::LazyLock;

/// Release version inherited from the authoritative workspace package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Distributed product revision. Increase before distributing changed binaries;
/// never increment during compilation or reset on a release-version change.
pub const BUILD_NUMBER: u16 = checked_build_number(41);
const fn checked_build_number(value: u16) -> u16 {
    assert!(
        value > 0 && value <= 9999,
        "Product build number must be in 1..=9999"
    );
    value
}

/// Human-facing identity; protocol version fields retain the release version.
pub static VERSION_WITH_BUILD: LazyLock<String> =
    LazyLock::new(|| format!("{VERSION} (Build {BUILD_NUMBER})"));

/// Canonical public CLI executable/command and protocol-1 program identity.
/// The desktop executable and bundle use the display name.
pub const CLI_NAME: &str = "emlattachmentremover";

/// Shared human product name.
pub const DISPLAY_NAME: &str = "EML Attachment Remover";
/// Project source and documentation.
pub const WEBSITE: &str = "https://github.com/resoltico/EMLAttachmentRemover";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_revision_range_has_checked_constant_boundaries() {
        const FIRST: u16 = checked_build_number(1);
        const LAST: u16 = checked_build_number(9999);
        assert_eq!(FIRST, 1);
        assert_eq!(LAST, 9999);
        for invalid in [0, 10000, u16::MAX] {
            assert!(std::panic::catch_unwind(|| checked_build_number(invalid)).is_err());
        }
    }
}
