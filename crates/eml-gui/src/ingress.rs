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
            crate::wording::ABSOLUTE_LAUNCH_DIRECTORY,
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
#[cfg(feature = "native")]
pub fn install_native(
    controller: &std::rc::Rc<crate::DesktopController>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::rc::Rc;
    let ui = controller.window();
    install_drops(controller);
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
                let paths = match super::native_dialogs::choose(&ui, kind == 0).await {
                    Ok(paths) => paths,
                    Err(error) => {
                        c.message(&error);
                        return;
                    }
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
            let file = match super::native_dialogs::save_report(
                &ui,
                &c.report_directory(),
                &source.suggested_name(),
            )
            .await
            {
                Ok(file) => file,
                Err(error) => {
                    c.cancel_export(&source);
                    c.message(&error);
                    return;
                }
            };
            if let Some(file) = file {
                c.start_captured_export(source, file);
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
                    crate::wording::SHOW_EMAIL_FAILED,
                    move || super::native_dialogs::reveal(&path),
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
                crate::wording::SHOW_OUTPUT_FAILED,
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
                crate::wording::SHOW_REPORT_FAILED,
                move || super::native_dialogs::reveal(&path),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    ui.on_website(move || {
        if let Some(c) = weak.upgrade() {
            c.open_advisory(
                crate::OpenTarget::Main,
                crate::wording::WEBSITE_FAILED,
                || opener::open(eml_core::product::WEBSITE).map_err(|e| e.to_string()),
            );
        }
    });
    let weak = Rc::downgrade(controller);
    controller.about_window().on_website(move || {
        if let Some(c) = weak.upgrade() {
            c.open_advisory(
                crate::OpenTarget::About,
                crate::wording::WEBSITE_FAILED,
                || opener::open(eml_core::product::WEBSITE).map_err(|e| e.to_string()),
            );
        }
    });
    #[cfg(target_os = "macos")]
    let notices = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .ok_or(crate::wording::BUNDLE_CONTENTS_MISSING)?
        .join("Resources/THIRD-PARTY-NOTICES.txt");
    let weak = Rc::downgrade(controller);
    controller.license_window().on_third_party(move || {
        if let Some(c) = weak.upgrade() {
            let path = notices.clone();
            c.open_advisory(
                crate::OpenTarget::Licenses,
                crate::wording::LICENSES_FAILED,
                move || opener::open(path).map_err(|e| e.to_string()),
            );
        }
    });
    Ok(())
}

#[cfg(feature = "desktop")]
fn install_drops(controller: &std::rc::Rc<crate::DesktopController>) {
    use slint::{
        ComponentHandle,
        winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent},
    };
    let weak = std::rc::Rc::downgrade(controller);
    controller
        .window()
        .window()
        .on_winit_window_event(move |_, event| {
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
}
