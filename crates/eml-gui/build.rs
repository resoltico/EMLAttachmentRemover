//! Compile platform-neutral Slint layout with embedded resources.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .with_debug_info(std::env::var("PROFILE")? == "debug")
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )?;
    Ok(())
}
