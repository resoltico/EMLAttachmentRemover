//! Parented XDG desktop file choosers with native path preservation.
use crate::MainWindow;
use ashpd::{
    WindowIdentifier,
    desktop::{
        ResponseError,
        file_chooser::{FileFilter, SelectedFiles},
    },
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

struct Parent {
    window: slint::WindowHandle,
    identifier: WindowIdentifier,
}
impl Parent {
    async fn new(ui: &MainWindow) -> Result<Self, String> {
        let window = ui.window().window_handle();
        let raw_window = window
            .window_handle()
            .map_err(|error| format!("{}: {error}", crate::wording::PORTAL_PARENT_UNAVAILABLE))?
            .as_raw();
        // X11 portals need only the XID. Wayland export requires its real display.
        let display = match raw_window {
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => None,
            RawWindowHandle::Wayland(_) => Some(
                window
                    .display_handle()
                    .map_err(|error| {
                        format!("{}: {error}", crate::wording::PORTAL_PARENT_UNAVAILABLE)
                    })?
                    .as_raw(),
            ),
            _ => return Err(crate::wording::PORTAL_PARENT_UNAVAILABLE.to_owned()),
        };
        let identifier = valid_identifier(
            WindowIdentifier::from_raw_handle(&raw_window, display.as_ref()).await,
        )?;
        Ok(Self { window, identifier })
    }
}
fn valid_identifier(identifier: Option<WindowIdentifier>) -> Result<WindowIdentifier, String> {
    identifier
        .filter(|identifier| {
            let value = identifier.to_string();
            value
                .strip_prefix("x11:")
                .is_some_and(|id| !id.is_empty() && id != "0")
                || value
                    .strip_prefix("wayland:")
                    .is_some_and(|id| !id.is_empty())
        })
        .ok_or_else(|| crate::wording::PORTAL_PARENT_UNAVAILABLE.to_owned())
}
fn dialog_error(error: impl std::fmt::Display) -> String {
    format!("{}: {error}", crate::wording::PORTAL_DIALOG_FAILED)
}
fn selection(
    response: Result<SelectedFiles, ashpd::Error>,
    multiple: bool,
) -> Result<Option<Vec<PathBuf>>, String> {
    match response {
        Ok(files) => Ok(Some(selected_paths(files.uris(), multiple)?)),
        Err(
            ashpd::Error::Response(ResponseError::Cancelled)
            | ashpd::Error::Portal(ashpd::PortalError::Cancelled(_)),
        ) => Ok(None),
        Err(error) => Err(dialog_error(error)),
    }
}
fn selected_paths(uris: &[ashpd::Uri], multiple: bool) -> Result<Vec<PathBuf>, String> {
    let paths: Vec<_> = uris
        .iter()
        .map(|uri| {
            let uri = url::Url::parse(uri.as_str()).map_err(|_| ())?;
            if uri.query().is_some() || uri.fragment().is_some() {
                return Err(());
            }
            let path = uri.to_file_path().map_err(|_| ())?;
            if path.as_os_str().as_bytes().contains(&0) {
                return Err(());
            }
            Ok(path)
        })
        .collect::<Result<_, ()>>()
        .map_err(|()| crate::wording::PORTAL_SELECTION_INVALID.to_owned())?;
    if paths.is_empty() || (!multiple && paths.len() != 1) {
        return Err(crate::wording::PORTAL_SELECTION_INVALID.to_owned());
    }
    Ok(paths)
}
pub(super) async fn choose(ui: &MainWindow, files: bool) -> Result<Option<Vec<PathBuf>>, String> {
    let parent = Parent::new(ui).await?;
    let words = ui.get_words();
    let mut request = SelectedFiles::open_file()
        .identifier(parent.identifier)
        .title(if files {
            words.add_files.as_str()
        } else {
            words.add_folder.as_str()
        })
        .modal(true)
        .multiple(files)
        .directory(!files)
        .current_folder(std::env::current_dir().map_err(dialog_error)?)
        .map_err(dialog_error)?;
    if files {
        request = request.filter(FileFilter::new(crate::wording::EML_FILTER).glob("*.eml"));
    }
    let response = request.send().await.and_then(|request| request.response());
    // Retain the Qt window and exported identifier for the entire portal request.
    drop(parent.window);
    selection(response, files)
}
pub(super) async fn save_report(
    ui: &MainWindow,
    directory: &Path,
    name: &str,
) -> Result<Option<PathBuf>, String> {
    let parent = Parent::new(ui).await?;
    let response = SelectedFiles::save_file()
        .identifier(parent.identifier)
        .title(ui.get_words().save_report.as_str())
        .modal(true)
        .current_folder(directory)
        .map_err(dialog_error)?
        .current_name(name)
        .filter(FileFilter::new(crate::wording::REPORT_FILTER).glob("*.jsonl"))
        .send()
        .await
        .and_then(|request| request.response());
    drop(parent.window);
    Ok(selection(response, false)?.map(|mut paths| paths.remove(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files(uris: &[&str]) -> Vec<ashpd::Uri> {
        uris.iter()
            .map(|uri| ashpd::Uri::parse(uri).unwrap())
            .collect()
    }
    #[test]
    fn portal_selection_preserves_bytes_and_rejects_partial_or_invalid_responses() {
        let paths = selected_paths(
            &files(&["file:///tmp/mail%FF.eml", "file:///tmp/a%20b.eml"]),
            true,
        )
        .unwrap();
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/mail\xff.eml");
        assert_eq!(paths[1], Path::new("/tmp/a b.eml"));
        for uris in [
            vec![],
            vec!["https://example.test/a"],
            vec!["file:///tmp/a", "https://example.test/b"],
            vec!["file://remote/tmp/a"],
            vec!["file:///tmp/%00"],
            vec!["file:///tmp/a?query"],
            vec!["file:///tmp/a#fragment"],
        ] {
            assert!(selected_paths(&files(&uris), true).is_err(), "{uris:?}");
        }
        assert!(selected_paths(&files(&["file:///a", "file:///b"]), false).is_err());
    }
    #[test]
    fn only_explicit_portal_cancellation_is_an_empty_selection() {
        assert!(
            selection(Err(ashpd::Error::Response(ResponseError::Cancelled)), true)
                .unwrap()
                .is_none()
        );
        assert!(
            selection(
                Err(ashpd::Error::Portal(ashpd::PortalError::Cancelled(
                    "user".into()
                ))),
                false
            )
            .unwrap()
            .is_none()
        );
        assert!(selection(Err(ashpd::Error::NoResponse), true).is_err());
        assert!(selection(Err(ashpd::Error::Response(ResponseError::Other)), true).is_err());
    }
    #[test]
    fn missing_or_empty_parent_export_is_an_error() {
        assert!(valid_identifier(None).is_err());
        assert!(valid_identifier(Some(WindowIdentifier::from_xid(0))).is_err());
        assert!(
            valid_identifier(Some(WindowIdentifier::from_xdg_foreign_exported(
                String::new()
            )))
            .is_err()
        );
        assert!(valid_identifier(Some(WindowIdentifier::from_xid(42))).is_ok());
    }
    #[test]
    fn portal_initial_directory_accepts_native_bytes_and_rejects_nul() {
        use std::os::unix::ffi::OsStrExt;
        let directory = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/mail\xff"));
        assert!(SelectedFiles::save_file().current_folder(directory).is_ok());
        let nul = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\0mail"));
        assert!(SelectedFiles::save_file().current_folder(nul).is_err());
    }
}
