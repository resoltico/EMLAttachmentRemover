//! Embed an explicitly supplied source identity in both product frontends.
mod build_input;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build_input.rs");
    println!("cargo:rerun-if-env-changed=EML_SOURCE_REVISION");
    let revision = match std::env::var("EML_SOURCE_REVISION") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => String::new(),
        Err(error) => return Err(error.into()),
    };
    if !revision.is_empty() {
        build_input::revision(&revision)?;
    }
    println!("cargo:rustc-env=EML_SOURCE_REVISION={revision}");
    Ok(())
}
