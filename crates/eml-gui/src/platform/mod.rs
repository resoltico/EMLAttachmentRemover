//! Application-owned macOS menu routing and Quit admission.
#[cfg(all(feature = "native", target_os = "macos"))]
mod macos;
pub(crate) fn set_busy(busy: bool) {
    #[cfg(all(feature = "native", target_os = "macos"))]
    macos::set_busy(busy);
    let _ = busy;
}
/// Install application-menu About routing and Quit admission after native startup.
pub fn install() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(all(feature = "native", target_os = "macos"))]
    macos::install()?;
    Ok(())
}
pub(crate) fn take_quit() -> bool {
    #[cfg(all(feature = "native", target_os = "macos"))]
    return macos::take_quit();
    #[cfg(not(all(feature = "native", target_os = "macos")))]
    false
}
pub(crate) fn take_about() -> bool {
    #[cfg(all(feature = "native", target_os = "macos"))]
    return macos::take_about();
    #[cfg(not(all(feature = "native", target_os = "macos")))]
    false
}

pub(crate) fn refresh_menu() {
    #[cfg(all(feature = "native", target_os = "macos"))]
    macos::refresh_menu();
}

/// Validate the platform's native launch layout before creating the UI.
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
                crate::wording::bundle_required(),
            ));
        }
    }
    let _ = (executable, cli);
    Ok(())
}

/// Platform-appropriate output-folder action label, without native service policy.
pub fn file_manager_label() -> &'static str {
    if cfg!(target_os = "macos") {
        crate::wording::SHOW_FINDER
    } else {
        crate::wording::SHOW_FILE_MANAGER
    }
}

/// Drain one batch of native file URLs delivered by macOS.
pub fn take_opened() -> Vec<std::path::PathBuf> {
    #[cfg(all(feature = "native", target_os = "macos"))]
    return macos::take_opened();
    #[cfg(not(all(feature = "native", target_os = "macos")))]
    Vec::new()
}
