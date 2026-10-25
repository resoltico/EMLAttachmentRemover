//! Immutable physical MIME facts. Opaque removal interiors are never parsed.
use crate::{PreparationError, Refusal, error, grammar};
use std::ops::Range;
pub(crate) type Span = Range<usize>;
pub(crate) const PROTECTED: &[&str] = &[
    "multipart/signed",
    "multipart/encrypted",
    "application/pkcs7-mime",
    "application/x-pkcs7-mime",
    "application/pkcs7-signature",
    "application/x-pkcs7-signature",
    "application/pgp-encrypted",
    "application/pgp-signature",
];
pub(crate) const UNSUPPORTED_BODY_MEDIA: &[&str] = &["multipart/report"];
#[derive(Debug)]
pub(crate) struct Field {
    pub name: String,
    pub value: Vec<u8>,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub(crate) struct Child {
    pub id: usize,
    pub opening: Span,
    pub framing: Span,
    pub deletion: Span,
}
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Leaf,
    Multipart {
        prefix: Span,
        children: Vec<Child>,
        suffix: Span,
    },
    Message(usize),
}
#[derive(Debug)]
pub(crate) struct Node {
    pub span: Span,
    pub body: Span,
    pub separator: Span,
    pub envelope: Span,
    pub fields: Vec<Field>,
    pub media: String,
    pub disposition: Option<String>,
    pub filename: Option<String>,
    pub encoding: Option<String>,
    pub kind: Kind,
    pub root: bool,
    pub section: Option<String>,
    pub content_type: grammar::Structured,
}
pub(crate) struct Message<'a> {
    pub input: &'a [u8],
    pub nodes: Vec<Node>,
}
fn located(mut failure: PreparationError, source_section: Option<String>) -> PreparationError {
    if let PreparationError::Refused { section, .. } = &mut failure
        && section.is_none()
    {
        *section = source_section;
    }
    failure
}
fn malformed(detail: &'static str) -> PreparationError {
    error(Refusal::Malformed, detail)
}
fn limit() -> PreparationError {
    error(
        Refusal::LimitExceeded,
        "MIME tree or header admission limit exceeded",
    )
}
fn previous_terminator(input: &[u8], start: usize, minimum: usize) -> usize {
    if start >= minimum + 2 && &input[start - 2..start] == b"\r\n" {
        start - 2
    } else if start > minimum && matches!(input[start - 1], b'\r' | b'\n') {
        start - 1
    } else {
        start
    }
}
fn section(prefix: Option<&str>, number: usize) -> String {
    prefix.map_or_else(|| number.to_string(), |prefix| format!("{prefix}.{number}"))
}
mod parse;
mod scan;
pub(crate) use parse::parse;

#[cfg(test)]
mod differential;
