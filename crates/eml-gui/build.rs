//! Compile platform-neutral Slint layout with embedded resources.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let macos = std::env::var("CARGO_CFG_TARGET_OS")? == "macos";
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR missing")?);
    // Slint 1.18.1 Cupertino ScrollView has a 2px frame; Fluent has none.
    // Keep layout constraints independent of the size they are calculating.
    std::fs::write(
        out.join("widget-metrics.slint"),
        format!(
            "export global WidgetMetrics {{ out property <length> scroll-inset: {}px; out property <bool> macos: {macos}; }}",
            if macos { 2 } else { 0 }
        ),
    )?;
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_include_paths(vec![out])
            .with_style(if macos {
                "cupertino".into()
            } else {
                "fluent".into()
            })
            .with_debug_info(std::env::var("PROFILE")? == "debug")
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )?;
    Ok(())
}
