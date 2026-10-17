//! Compile the actual macOS UI profile with embedded resources.
#[cfg(feature = "macos-gui")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("CARGO_CFG_TARGET_OS")?.as_str() != "macos" {
        return Err("The desktop product currently supports macOS only".into());
    }
    // Track absence as well as presence; arbitrary compiler overrides must not
    // embed unrelated resources or silently change the supported UI profile.
    for name in [
        "SLINT_STYLE",
        "SLINT_FONT_SIZES",
        "SLINT_SCALE_FACTOR",
        "SLINT_ASSET_SECTION",
        "SLINT_EMBED_RESOURCES",
        "SLINT_EMBED_TEXTURES",
        "SLINT_EMIT_DEBUG_INFO",
        "SLINT_LIVE_PREVIEW",
        "SLINT_BUNDLE_TRANSLATIONS",
        "SLINT_INLINING",
        "SLINT_ENABLE_EXPERIMENTAL_FEATURES",
        "SLINT_CPP_NAMESPACE",
        "SLINT_COMPILER_DENY_WARNINGS",
        "SLINT_INCLUDE_GENERATED",
        "DEP_MCU_BOARD_SUPPORT_BOARD_CONFIG_PATH",
        "DEP_MCU_BOARD_SUPPORT_MCU_BOARD_CONFIG_PATH",
        "DEP_MCU_BOARD_SUPPORT_MCU_EMBED_TEXTURES",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for (name, value) in std::env::vars_os() {
        let spelling = name.to_string_lossy();
        if spelling.starts_with("SLINT_") || spelling.starts_with("DEP_MCU_BOARD_SUPPORT_") {
            println!("cargo:rerun-if-env-changed={spelling}");
            if name != "SLINT_COMPILER_DENY_WARNINGS" || value != "1" {
                return Err(format!("Unset unsupported desktop compiler override {name:?}").into());
            }
        }
    }
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )?;
    Ok(())
}
#[cfg(not(feature = "macos-gui"))]
fn main() {}
