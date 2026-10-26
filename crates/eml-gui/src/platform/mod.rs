//! Application-owned macOS menu routing and Quit admission.
#[cfg(all(feature = "desktop", target_os = "macos"))]
mod macos;
pub(crate) fn set_busy(busy: bool) {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    macos::set_busy(busy);
    let _ = busy;
}
/// Install application-menu About routing and Quit admission after native startup.
pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    macos::install()?;
    Ok(())
}
pub(crate) fn take_quit() -> bool {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    return macos::take_quit();
    #[cfg(not(all(feature = "desktop", target_os = "macos")))]
    false
}
pub(crate) fn take_about() -> bool {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    return macos::take_about();
    #[cfg(not(all(feature = "desktop", target_os = "macos")))]
    false
}

pub(crate) fn refresh_menu() {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    macos::refresh_menu();
}

/// Reject an unsupported macOS bare executable before creating a misidentified UI.
pub fn validate_launch(executable: &std::path::Path, cli: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let macos = executable.parent();
        let contents = macos.and_then(std::path::Path::parent);
        let bundle = contents.and_then(std::path::Path::parent);
        if macos.and_then(std::path::Path::file_name) != Some(std::ffi::OsStr::new("MacOS"))
            || contents.and_then(std::path::Path::file_name)
                != Some(std::ffi::OsStr::new("Contents"))
            || !bundle
                .and_then(std::path::Path::extension)
                .is_some_and(|ext| ext.as_encoded_bytes().eq_ignore_ascii_case(b"app"))
            || !contents.is_some_and(|p| p.join("Info.plist").is_file())
            || !cli.is_file()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Launch {}.app from the GUI archive, rather than the bare GUI executable. Its bundle metadata and Contents/Helpers CLI are required. To build it, run cargo xtask dist --app --out target/desktop.",
                    eml_core::product::DISPLAY_NAME
                ),
            ));
        }
    }
    let _ = (executable, cli);
    Ok(())
}
