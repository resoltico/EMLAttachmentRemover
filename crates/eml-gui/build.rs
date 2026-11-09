//! Compile platform-neutral Slint layout with embedded resources.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::var("CARGO_CFG_TARGET_OS")?;
    let macos = target == "macos";
    let winit = std::env::var_os("CARGO_FEATURE_DESKTOP").is_some();
    let native = std::env::var_os("CARGO_FEATURE_NATIVE").is_some();
    if (winit && !macos) || (native && !winit) {
        return Err("The desktop feature requires macOS; native requires desktop".into());
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR missing")?);
    // Slint 1.18.1: Cupertino has a 2px frame and a 20px hover scrollbar;
    // Fluent has no frame and a 14px overlay scrollbar.
    // Keep layout constraints independent of the size they are calculating.
    std::fs::write(
        out.join("widget-metrics.slint"),
        format!(
            "export global WidgetMetrics {{ out property <length> scroll-inset: {}px; out property <length> overlay-scrollbar-width: {}px; out property <bool> macos: {macos}; }}",
            if macos { 2 } else { 0 },
            if macos { 20 } else { 14 }
        ),
    )?;
    let configuration = slint_build::CompilerConfiguration::new()
        .with_include_paths(vec![out])
        .with_style(if macos {
            "cupertino".into()
        } else {
            "fluent".into()
        })
        .with_debug_info(std::env::var("PROFILE")? == "debug")
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    slint_build::compile_with_config("ui/main.slint", configuration)?;
    Ok(())
}
