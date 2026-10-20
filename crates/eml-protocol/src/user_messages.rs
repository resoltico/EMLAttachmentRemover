//! Human-facing explanations shared by the CLI and desktop, outside wire facts.

/// Explain an observed failure and the next useful action. Unknown protocol codes
/// keep their supplied detail; they must not acquire an invented interpretation.
pub fn recovery_hint(code: &str) -> Option<&'static str> {
    eml_core::ErrorCode::from_wire_name(code).and_then(eml_core::ErrorCode::recovery_hint)
}

/// Material consequence of the body-only policy; HTML references are not rewritten.
pub const RELATED_RESOURCE_NOTICE: &str = "Inline images or other related resources were removed. They may appear missing in the copied message.";

/// Consequence for a prepared candidate that has not been confirmed as published.
pub const PREPARED_RELATED_RESOURCE_NOTICE: &str = "The prepared copy omits inline images or other related resources. They may appear missing if that copy is saved.";
