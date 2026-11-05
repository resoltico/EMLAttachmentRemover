//! Pure draft, queue and navigation state. No toolkit or I/O dependencies.
use eml_app::{RunId, RunQueue};
use eml_core::RunMode;
use std::path::PathBuf;

pub(super) struct Session {
    pub selection: Vec<PathBuf>,
    pub reviewing: bool,
    pub follow_run: Option<RunId>,
    pub queue: RunQueue,
    pub selected: usize,
    pub selected_item: usize,
    pub mode: RunMode,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            selection: Vec::new(),
            reviewing: false,
            follow_run: None,
            queue: RunQueue::default(),
            selected: 0,
            selected_item: 0,
            mode: RunMode::Create,
        }
    }
}
#[derive(Default)]
pub(super) struct RunPresentation {
    pub user_selected_item: bool,
    pub unseen: bool,
}
impl RunPresentation {
    pub(super) fn settle(&mut self, visible: bool) {
        self.unseen = !visible;
    }
    pub(super) fn visit(&mut self) {
        self.unseen = false;
    }
}
