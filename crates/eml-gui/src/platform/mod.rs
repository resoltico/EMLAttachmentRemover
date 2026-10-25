//! The sole native desktop adapter is macOS Quit admission.
#[cfg(all(feature = "desktop", target_os = "macos"))]
mod macos;
pub(crate) fn set_busy(busy: bool) {
    #[cfg(all(feature = "desktop", target_os = "macos"))]
    macos::set_busy(busy);
    let _ = busy;
}
/// Install the Quit adapter after the window has created its backend delegate.
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
