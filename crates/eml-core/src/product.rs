//! Shared display identity for CLI, desktop and distribution metadata.
use std::sync::LazyLock;

/// Release version inherited from the authoritative workspace package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Full source revision when supplied by CI or distribution; ordinary builds may omit it.
pub const SOURCE_REVISION: Option<&str> = if env!("EML_SOURCE_REVISION").is_empty() {
    None
} else {
    Some(env!("EML_SOURCE_REVISION"))
};

/// Full human-facing source identity; protocol fields retain the release version.
pub static VERSION_WITH_REVISION: LazyLock<String> =
    LazyLock::new(|| version_text(VERSION, SOURCE_REVISION));

/// Compact About identity using the first twelve characters of a supplied revision.
pub static ABOUT_VERSION: LazyLock<String> =
    LazyLock::new(|| version_text(VERSION, SOURCE_REVISION.map(|value| &value[..12])));

fn version_text(version: &str, revision: Option<&str>) -> String {
    revision.map_or_else(
        || version.to_owned(),
        |value| format!("{version} ({value})"),
    )
}

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
    fn full_source_identity_and_compact_about_identity_share_the_release_version() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(version_text("7.0.0", None), "7.0.0");
        assert_eq!(
            version_text("7.0.0", Some(revision)),
            "7.0.0 (0123456789abcdef0123456789abcdef01234567)"
        );
        assert_eq!(
            version_text("7.0.0", Some(&revision[..12])),
            "7.0.0 (0123456789ab)"
        );
    }
}
