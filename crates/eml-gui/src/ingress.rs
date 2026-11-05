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
                WindowEvent::HoveredFile(_) => controller.hover_drop(true),
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
        if !controller.inputs_enabled() {
            return;
        }
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
                && controller.inputs_enabled()
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
        if !controller.inputs_enabled() {
            return;
        }
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
                && controller.inputs_enabled()
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
        if !controller.inputs_enabled() {
            return;
        }
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
        controller.begin_export();
    });
    let weak = Rc::downgrade(controller);
    controller.export_window().on_choose_directory(move || {
        let Some(controller) = weak.upgrade() else {
            return;
        };
        let Some(request) = controller.capture_export_folder() else {
            return;
        };
        let export = controller.export_window();
        let _ = slint::spawn_local(async move {
            let parent = match export.window().winit_window().await {
                Ok(parent) => parent,
                Err(error) => {
                    controller.export_notice(&request, &error.to_string());
                    return;
                }
            };
            if let Some(folder) = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .pick_folder()
                .await
            {
                controller.set_export_directory(&request, folder.path().to_owned());
            }
        });
    });
    // Keep clipboard ownership alive for backends that serve requests asynchronously.
    let clipboard = Rc::new(std::cell::RefCell::new(None::<arboard::Clipboard>));
    for destination in [false, true] {
        let clipboard = clipboard.clone();
        let weak = Rc::downgrade(controller);
        let copy = move || {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let Some(path) = controller
                .selected_path(destination)
                .and_then(|p| p.into_os_string().into_string().ok())
            else {
                return;
            };
            let result = (|| {
                let mut context = clipboard.borrow_mut();
                if context.is_none() {
                    *context = Some(arboard::Clipboard::new()?);
                }
                context
                    .as_mut()
                    .expect("clipboard initialized")
                    .set_text(path)
            })();
            match result {
                Ok(()) => controller.message(if destination {
                    "Destination path copied."
                } else {
                    "Source path copied."
                }),
                Err(error) => controller.message(&format!("Cannot copy path: {error}")),
            }
        };
        if destination {
            ui.on_copy_destination_path(copy);
        } else {
            ui.on_copy_source_path(copy);
        }
    }
    let weak = Rc::downgrade(controller);
    ui.on_reveal(move || {
        if let Some(controller) = weak.upgrade()
            && let Some(path) = controller.selected_copy()
        {
            controller.open_advisory(
                crate::OpenTarget::Main,
                "Could not show the copy. Check that it still exists, then try again.",
                move || opener::reveal(path).map_err(|error| error.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_reveal_original(move || {
        if let Some(controller) = weak.upgrade()
            && let Some(path) = controller.selected_original()
        {
            controller.open_advisory(
                crate::OpenTarget::Main,
                "Could not show the original. Check that it still exists, then try again.",
                move || opener::reveal(path).map_err(|error| error.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_reveal_output(move |index| {
        if let Some(controller) = weak.upgrade()
            && let Ok(index) = usize::try_from(index)
            && let Some(path) = controller.output_roots().get(index).cloned()
        {
            controller.open_advisory(
                crate::OpenTarget::Main,
                "Could not open the output folder. Check that it still exists, then try again.",
                move || opener::open(path).map_err(|error| error.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_reveal_report(move || {
        if let Some(controller) = weak.upgrade()
            && let Some(path) = controller.saved_report_path()
        {
            controller.open_advisory(
                crate::OpenTarget::Main,
                "Could not show the saved report. Check that it still exists, then try again.",
                move || opener::reveal(path).map_err(|error| error.to_string()),
            );
        }
    });
    let about = controller.about_window();
    let weak = Rc::downgrade(controller);
    about.on_website(move || {
        if let Some(controller) = weak.upgrade() {
            controller.open_advisory(
                crate::OpenTarget::About,
                "Could not open the project website. Check your browser, then try again.",
                || opener::open(eml_core::product::WEBSITE).map_err(|error| error.to_string()),
            );
        }
    });
    let notices = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .ok_or("Desktop executable has no Contents directory")?
        .join("Resources/THIRD-PARTY-NOTICES.txt");
    let weak = Rc::downgrade(controller);
    controller.license_window().on_third_party(move || {
        if let Some(controller) = weak.upgrade() {
            let path = notices.clone();
            controller.open_advisory(crate::OpenTarget::Licenses,
                "Could not open bundled third-party notices. Check the app installation, then try again.",
                move || opener::open(path).map_err(|error| error.to_string()));
        }
    });
    Ok(())
}
