//! Single action predicates shared by menus, buttons and controller boundaries.
use super::*;
impl DesktopController {
    pub(super) fn locked(&self) -> bool {
        let r = self.runtime.borrow();
        r.quit || r.quitting
    }
    /// All input sources use this predicate again when committing paths.
    pub fn inputs_enabled(&self) -> bool {
        !self.locked() && self.runtime.borrow().batch.state() != State::Removing
    }
    pub(super) fn can_destination(&self) -> bool {
        !self.locked()
            && matches!(
                self.runtime.borrow().batch.state(),
                State::Checking | State::Ready
            )
    }
    pub(super) fn can_reset(&self) -> bool {
        !self.locked()
            && !matches!(
                self.runtime.borrow().batch.state(),
                State::Empty | State::Removing
            )
    }
    pub(super) fn can_start(&self) -> bool {
        if self.locked() {
            return false;
        }
        let r = self.runtime.borrow();
        if r.batch.inputs.is_empty() {
            return false;
        }
        match r.batch.state() {
            // A stopped request is repeatable even before any preview/item exists.
            State::Checking | State::Stopped => true,
            State::Ready => r
                .batch
                .check
                .as_ref()
                .and_then(|slot| slot.report.as_ref())
                .map(|handle| handle.with_report(worker::RunReport::can_offer_copy))
                .unwrap_or(false),
            State::Empty | State::Removing | State::Done => false,
        }
    }
    pub(super) fn can_stop(&self) -> bool {
        !self.locked()
            && self
                .runtime
                .borrow()
                .active
                .as_ref()
                .is_some_and(|a| a.kind == Kind::Create && a.stop_at.is_none())
    }
    pub(super) fn can_quit_now(&self) -> bool {
        let r = self.runtime.borrow();
        r.quit
            && r.quitting
            && r.active.as_ref().is_some_and(|a| a.kind == Kind::Create)
            && r.quit_at
                .is_some_and(|at| now_ms().saturating_sub(at) >= 5000)
    }
    /// Reveal actions require an earlier verified create receipt.
    pub fn can_show_copy(&self) -> bool {
        !self.locked() && self.selected_copy().is_some()
    }
    /// Originals use the selected native operand without publication qualification.
    pub fn can_show_original(&self) -> bool {
        !self.locked() && self.selected_original().is_some()
    }
    /// Output roots are available only for settled creates.
    pub fn can_show_output(&self) -> bool {
        !self.locked()
            && matches!(
                self.runtime.borrow().batch.state(),
                State::Done | State::Stopped
            )
            && !self.output_roots().is_empty()
    }
    /// Only settled streams can be captured, with one pending save lifetime.
    pub fn can_save_report(&self) -> bool {
        !self.locked()
            && self.pending_save.borrow().is_none()
            && self.runtime.borrow().export.is_none()
            && self.settled_bytes().is_some()
    }
}

impl DesktopController {
    pub(super) fn can_show_report(&self) -> bool {
        !self.locked()
            && self.saved_report.borrow().is_some()
            && self.notice.borrow().as_ref().is_some_and(|(text, at)| {
                text == wording::REPORT_SAVED && now_ms().saturating_sub(*at) < 6000
            })
    }
}

impl DesktopController {
    pub(super) fn can_filter(&self) -> bool {
        !self.locked() && self.rows.borrow().iter().any(present::Row::problem)
    }
    pub(super) fn can_copy_details(&self) -> bool {
        !self.locked() && self.selected_original().is_some()
    }
}
