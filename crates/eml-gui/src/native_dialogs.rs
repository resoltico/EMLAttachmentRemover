//! Native file panels and file-manager integration for the supported backends.
use crate::MainWindow;
#[cfg(feature = "desktop")]
use slint::ComponentHandle;
use std::path::{Path, PathBuf};

#[cfg(feature = "desktop")]
async fn dialog(ui: &MainWindow) -> Result<rfd::AsyncFileDialog, String> {
    use slint::winit_030::WinitWindowAccessor;
    let parent = ui
        .window()
        .winit_window()
        .await
        .map_err(|e| e.to_string())?;
    Ok(rfd::AsyncFileDialog::new().set_parent(parent.as_ref()))
}

pub(crate) async fn choose(ui: &MainWindow, files: bool) -> Result<Option<Vec<PathBuf>>, String> {
    #[cfg(feature = "desktop")]
    {
        let panel = dialog(ui).await?;
        Ok(if files {
            panel
                .add_filter(crate::wording::EML_FILTER, &["eml"])
                .pick_files()
                .await
                .map(|files| {
                    files
                        .into_iter()
                        .map(|file| file.path().to_owned())
                        .collect()
                })
        } else {
            panel
                .pick_folder()
                .await
                .map(|file| vec![file.path().to_owned()])
        })
    }
}

pub(crate) async fn save_report(
    ui: &MainWindow,
    directory: &Path,
    name: &str,
) -> Result<Option<PathBuf>, String> {
    #[cfg(feature = "desktop")]
    {
        Ok(dialog(ui)
            .await?
            .set_directory(directory)
            .set_file_name(name)
            .add_filter(crate::wording::REPORT_FILTER, &["jsonl"])
            .save_file()
            .await
            .map(|file| file.path().to_owned()))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn reveal(path: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .status()
        .map_err(|error| {
            eml_core::vocabulary::interface_io_error(
                eml_core::vocabulary::InterfaceIo::OpenPath,
                &error,
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(eml_core::vocabulary::open_status_message(status.code()))
    }
}
