//! Transient operation feedback never replaces immutable run observations.
use super::*;
impl DesktopController {
    pub(super) fn set_feedback(&self, feedback: state::Feedback) {
        self.operation_serial
            .set(self.operation_serial.get().wrapping_add(1));
        *self.feedback.borrow_mut() = feedback;
        self.present_feedback();
    }
    /// Display an actionable user-operation failure with optional technical detail.
    pub fn failure(&self, label: &str, message: &str, details: &str) {
        self.set_feedback(state::Feedback::Message {
            label: label.into(),
            text: message.into(),
            details: details.into(),
            tone: state::FeedbackTone::Attention,
        });
    }
    /// Display affirmative feedback for a completed, observed operation.
    pub fn success(&self, message: &str) {
        self.set_feedback(state::Feedback::Message {
            label: String::new(),
            text: message.into(),
            details: String::new(),
            tone: state::FeedbackTone::Success,
        });
    }
    /// Show a failure or other actionable status separate from run facts.
    pub fn message(&self, message: &str) {
        self.failure("", message, "");
    }
    pub(super) fn clear_operation(&self) {
        self.set_feedback(state::Feedback::None);
    }
    pub(super) fn expire_feedback(&self) {
        let expired = matches!(&*self.feedback.borrow(),state::Feedback::SavedReport{at,..} if now_ms().saturating_sub(*at)>=5000);
        if expired {
            self.clear_operation();
        }
    }
}
