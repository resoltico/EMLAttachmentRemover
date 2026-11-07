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

/// Connect native drops and parented file panels to the same admission boundary.
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
        if let Some(c) = weak.upgrade() {
            match event {
                WindowEvent::HoveredFile(_) => c.hover_drop(true),
                WindowEvent::DroppedFile(path) => c.deliver_drop(path.clone()),
                WindowEvent::HoveredFileCancelled => c.cancel_drop(),
                _ => {}
            }
        }
        EventResult::Propagate
    });
    for kind in 0..3 {
        let weak = Rc::downgrade(controller);
        let choose = move || {
            let Some(c) = weak.upgrade() else {
                return;
            };
            if !c.inputs_enabled() || (kind == 2 && !c.can_destination()) {
                return;
            }
            let epoch = c.panel_epoch();
            let ui = c.window();
            let _ = slint::spawn_local(async move {
                let parent = match ui.window().winit_window().await {
                    Ok(parent) => parent,
                    Err(e) => {
                        c.message(&e.to_string());
                        return;
                    }
                };
                let dialog = rfd::AsyncFileDialog::new().set_parent(parent.as_ref());
                let paths = if kind == 0 {
                    dialog
                        .add_filter("EML messages", &["eml"])
                        .pick_files()
                        .await
                        .map(|files| {
                            files
                                .into_iter()
                                .map(|f| f.path().to_owned())
                                .collect::<Vec<_>>()
                        })
                } else {
                    dialog
                        .pick_folder()
                        .await
                        .map(|folder| vec![folder.path().to_owned()])
                };
                if epoch != c.panel_epoch() || !c.inputs_enabled() {
                    return;
                }
                if let Some(paths) = paths {
                    if kind == 2 {
                        c.set_destination(paths.into_iter().next());
                    } else if let Err(e) = c.stage(paths) {
                        c.message(&crate::wording::request_error(&e));
                    }
                }
            });
        };
        match kind {
            0 => ui.on_choose_files(choose),
            1 => ui.on_choose_folder(choose),
            _ => ui.on_choose_destination(choose),
        }
    }
    let weak = Rc::downgrade(controller);
    ui.on_export_report(move || {
        let Some(c) = weak.upgrade() else {
            return;
        };
        let Some(source) = c.capture_export() else {
            return;
        };
        let ui = c.window();
        let on_error = c.clone();
        let failed = source.clone();
        if let Err(e) = slint::spawn_local(async move {
            let parent = match ui.window().winit_window().await {
                Ok(parent) => parent,
                Err(e) => {
                    c.cancel_export(&source);
                    c.message(&e.to_string());
                    return;
                }
            };
            let file = rfd::AsyncFileDialog::new()
                .set_parent(parent.as_ref())
                .set_directory(c.report_directory())
                .set_file_name(source.suggested_name())
                .add_filter("JSON Lines report", &["jsonl"])
                .save_file()
                .await;
            if let Some(file) = file {
                c.start_captured_export(source, file.path().to_owned());
            } else {
                c.cancel_export(&source);
            }
        }) {
            on_error.cancel_export(&failed);
            on_error.message(&e.to_string());
        }
    });
    let clipboard = Rc::new(std::cell::RefCell::new(None::<arboard::Clipboard>));
    let weak = Rc::downgrade(controller);
    ui.on_copy_details(move || {
        let Some(c) = weak.upgrade() else {
            return;
        };
        if !c.can_show_original() {
            return;
        }
        let Some(text) = c.clipboard_text() else {
            return;
        };
        let result = (|| {
            let mut context = clipboard.borrow_mut();
            if context.is_none() {
                *context = Some(arboard::Clipboard::new()?);
            }
            context.as_mut().unwrap().set_text(text)
        })();
        if let Err(e) = result {
            c.message(&e.to_string());
        }
    });
    for copy in [false, true] {
        let weak = Rc::downgrade(controller);
        let reveal = move || {
            let Some(c) = weak.upgrade() else {
                return;
            };
            if !(if copy {
                c.can_show_copy()
            } else {
                c.can_show_original()
            }) {
                return;
            }
            if let Some(path) = if copy {
                c.selected_copy()
            } else {
                c.selected_original()
            } {
                c.open_advisory(
                    crate::OpenTarget::Main,
                    "Couldn’t show the email. Check that it still exists.",
                    move || opener::reveal(path).map_err(|e| e.to_string()),
                );
            }
        };
        if copy {
            ui.on_show_copy(reveal);
        } else {
            ui.on_show_original(reveal);
        }
    }
    let weak = Rc::downgrade(controller);
    ui.on_reveal_output(move |index| {
        if let Some(c) = weak.upgrade()
            && c.can_show_output()
            && let Some(path) = c.output_roots().get(index as usize).cloned()
        {
            c.open_advisory(
                crate::OpenTarget::Main,
                "Couldn’t open the save folder.",
                move || opener::open(path).map_err(|e| e.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_reveal_report(move || {
        if let Some(c) = weak.upgrade()
            && c.can_show_report()
            && let Some(path) = c.saved_report_path()
        {
            c.open_advisory(
                crate::OpenTarget::Main,
                "Couldn’t show the saved report.",
                move || opener::reveal(path).map_err(|e| e.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_website(move || {
        if let Some(c) = weak.upgrade() {
            c.open_advisory(
                crate::OpenTarget::Main,
                "Couldn’t open the project website.",
                || opener::open(eml_core::product::WEBSITE).map_err(|e| e.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    controller.about_window().on_website(move || {
        if let Some(c) = weak.upgrade() {
            c.open_advisory(
                crate::OpenTarget::About,
                "Couldn’t open the project website.",
                || opener::open(eml_core::product::WEBSITE).map_err(|e| e.to_string()),
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
        if let Some(c) = weak.upgrade() {
            let path = notices.clone();
            c.open_advisory(
                crate::OpenTarget::Licenses,
                "Couldn’t open bundled third-party notices.",
                move || opener::open(path).map_err(|e| e.to_string()),
            );
        }
    });
    Ok(())
}
