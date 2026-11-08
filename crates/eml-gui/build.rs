//! Compile platform-neutral Slint layout with embedded resources.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let target = std::env::var("CARGO_CFG_TARGET_OS")?;
    let macos = target == "macos";
    let winit = std::env::var_os("CARGO_FEATURE_DESKTOP_WINIT").is_some();
    let qt = std::env::var_os("CARGO_FEATURE_DESKTOP_QT").is_some();
    let native = std::env::var_os("CARGO_FEATURE_NATIVE").is_some();
    if (winit && (!macos || qt)) || (qt && target != "linux") || (native && !winit && !qt) {
        return Err("Select desktop-winit on macOS or desktop-qt on Linux; native requires one supported backend".into());
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
    let linux = target == "linux";
    let mut include_paths = vec![out];
    if linux {
        let widgets = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR missing")?,
        )
        .join("../slint-widgets");
        include_paths.extend([widgets.clone(), widgets.join("eml-fluent")]);
    }
    let mut configuration = slint_build::CompilerConfiguration::new()
        .with_include_paths(include_paths)
        .with_style(if macos {
            "cupertino".into()
        } else if linux {
            "eml-fluent".into()
        } else {
            "fluent".into()
        })
        .with_debug_info(std::env::var("PROFILE")? == "debug")
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    if linux {
        // The published Fluent sources use internal interfaces/properties. This
        // is the broad compiler flag, scoped to this Linux compilation only.
        configuration = configuration.with_experimental_features(true);
    }
    slint_build::compile_with_config("ui/main.slint", configuration)?;
    Ok(())
}
