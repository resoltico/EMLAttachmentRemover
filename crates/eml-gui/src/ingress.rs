//! Native path capture and deferred batching of one window drop.
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};
/// All launch operands form one selection; Finder's historical PSN operand is ignored.
pub fn launch_paths(
    arguments: impl IntoIterator<Item = OsString>,
    cwd: &Path,
) -> io::Result<Vec<PathBuf>> {
    if !cwd.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Launch operands need an absolute working directory",
        ));
    }
    Ok(arguments
        .into_iter()
        .filter(|arg| !arg.to_str().is_some_and(|s| s.starts_with("-psn_")))
        .map(|arg| {
            let path = PathBuf::from(arg);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect())
}
/// Paths delivered before the zero-delay Slint timer commits the native drop.
#[derive(Default)]
pub struct DropBatch {
    pending: Vec<PathBuf>,
}
impl DropBatch {
    /// Whether the first path needs to schedule the deferred commit.
    pub fn push(&mut self, path: PathBuf) -> bool {
        self.pending.push(path);
        self.pending.len() == 1
    }
    /// Cancel a hover that delivered no committed selection.
    pub fn cancel(&mut self) {
        self.pending.clear();
    }
    /// Consume the complete batch once.
    pub fn commit(&mut self) -> Option<Vec<PathBuf>> {
        (!self.pending.is_empty()).then(|| std::mem::take(&mut self.pending))
    }
}

/// Connect toolkit-native drops, parented rfd dialogs and advisory file-manager actions.
#[cfg(feature = "desktop")]
pub fn install_native(
    controller: &std::rc::Rc<crate::DesktopController>,
) -> Result<(), Box<dyn std::error::Error>> {
    use slint::{
        ComponentHandle,
        winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent},
    };
    use std::rc::Rc;
    let ui = controller.window();
    let weak = Rc::downgrade(controller);
    ui.window().on_winit_window_event(move |_, event| {
        if let Some(controller) = weak.upgrade() {
            match event {
                WindowEvent::DroppedFile(path) => controller.deliver_drop(path.clone()),
                WindowEvent::HoveredFileCancelled => controller.cancel_drop(),
                _ => {}
            }
        }
        EventResult::Propagate
    });
    let weak = Rc::downgrade(controller);
    ui.on_choose_files(move || {
        let Some(controller) = weak.upgrade() else {
            return;
        };
        let ui = controller.window();
        let _ = slint::spawn_local(async move {
            // rfd needs both native handle traits; keep the winit owner alive.
            let parent = match ui.window().winit_window().await {
                Ok(parent) => parent,
                Err(error) => {
                    controller.message(&error.to_string());
                    return;
                }
            };
            if let Some(files) = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .add_filter("EML messages", &["eml"])
                .pick_files()
                .await
                && let Err(error) = controller.stage(
                    files
                        .into_iter()
                        .map(|file| file.path().to_owned())
                        .collect(),
                )
            {
                controller.message(&error.to_string());
            }
        });
    });
    let weak = Rc::downgrade(controller);
    ui.on_choose_folder(move || {
        let Some(controller) = weak.upgrade() else {
            return;
        };
        let ui = controller.window();
        let _ = slint::spawn_local(async move {
            // rfd needs both native handle traits; keep the winit owner alive.
            let parent = match ui.window().winit_window().await {
                Ok(parent) => parent,
                Err(error) => {
                    controller.message(&error.to_string());
                    return;
                }
            };
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .pick_folder()
                .await
                && let Err(error) = controller.stage(vec![folder.path().to_owned()])
            {
                controller.message(&error.to_string());
            }
        });
    });
    let weak = Rc::downgrade(controller);
    ui.on_choose_destination(move || {
        let Some(controller) = weak.upgrade() else {
            return;
        };
        let ui = controller.window();
        let _ = slint::spawn_local(async move {
            // rfd needs both native handle traits; keep the winit owner alive.
            let parent = match ui.window().winit_window().await {
                Ok(parent) => parent,
                Err(error) => {
                    controller.message(&error.to_string());
                    return;
                }
            };
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .pick_folder()
                .await
            {
                controller.set_destination(Some(folder.path().to_owned()));
            }
        });
    });
    let weak = Rc::downgrade(controller);
    ui.on_export_report(move || {
        let Some(controller) = weak.upgrade() else {
            return;
        };
        let Some(source) = controller.capture_export() else {
            return;
        };
        let ui = controller.window();
        let _ = slint::spawn_local(async move {
            // rfd needs both native handle traits; keep the winit owner alive.
            let parent = match ui.window().winit_window().await {
                Ok(parent) => parent,
                Err(error) => {
                    controller.message(&error.to_string());
                    return;
                }
            };
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .set_file_name(format!("eml-run-{}.jsonl", source.run_id()))
                .add_filter("JSON Lines", &["jsonl"])
                .save_file()
                .await
            {
                controller.start_captured_export(source, file.path().to_owned());
            }
        });
    });
    let weak = Rc::downgrade(controller);
    ui.on_reveal(move || {
        if let Some(controller) = weak.upgrade()
            && let Some(path) = controller.selected_copy()
            && let Err(error) = opener::reveal(path)
        {
            controller.message(&error.to_string());
        }
    });
    let about = controller.about_window();
    let weak = Rc::downgrade(controller);
    about.on_website(move || {
        if let Err(error) = opener::open(eml_core::product::WEBSITE)
            && let Some(controller) = weak.upgrade()
        {
            controller.message(&error.to_string());
        }
    });
    let notices = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .ok_or("Desktop executable has no Contents directory")?
        .join("Resources/THIRD-PARTY-NOTICES.txt");
    let weak = Rc::downgrade(controller);
    controller.license_window().on_third_party(move || {
        if let Err(error) = opener::open(&notices)
            && let Some(controller) = weak.upgrade()
        {
            let feedback = format!(
                "Cannot open bundled third-party notices at {}: {error}",
                notices.display()
            );
            controller
                .license_window()
                .set_notice(eml_protocol::DisplayText::new(&feedback).to_string().into());
            controller.message(&feedback);
        }
    });
    Ok(())
}
