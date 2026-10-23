//! Shared display identity for CLI, desktop and distribution metadata.
use std::sync::LazyLock;

/// Release version inherited from the authoritative workspace package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Distributed product revision. Increase before distributing changed binaries;
/// never increment during compilation or reset on a release-version change.
pub const BUILD_NUMBER: u16 = 36;
/// Human-facing identity; protocol version fields retain the release version.
pub static VERSION_WITH_BUILD: LazyLock<String> =
    LazyLock::new(|| format!("{VERSION} (Build {BUILD_NUMBER})"));

/// Shared human product name.
pub const DISPLAY_NAME: &str = "EML Attachment Remover";
/// Project source and documentation.
pub const WEBSITE: &str = "https://github.com/resoltico/EMLAttachmentRemover";
