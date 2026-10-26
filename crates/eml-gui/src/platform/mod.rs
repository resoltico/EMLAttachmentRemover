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
