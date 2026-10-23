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
mod differential {
    use super::*;
    use crate::RunControl;
    use mail_parser::{MessageParser, MimeHeaders, PartType};
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { cases:64, failure_persistence:Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(concat!(env!("CARGO_MANIFEST_DIR"),"/../../target/proptest/differential.txt")))),..ProptestConfig::default() })]
        #[test]
        fn independent_parser_agrees_on_crlf_identity_leaf_types_and_raw_spans(a in "[A-Za-z0-9 =]{0,128}", b in "[A-Za-z0-9 =]{0,128}") {
            // Matching syntax subset only: explicit ASCII types, CRLF, identity CTE,
            // distinct safe boundaries, no embedded-message offset coordinate systems.
            let wire=format!("MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=outer\r\n\r\n--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\nContent-Type: text/plain\r\nContent-Transfer-Encoding: 7bit\r\n\r\n{a}Z\r\n--inner\r\nContent-Type: text/html\r\nContent-Transfer-Encoding: 8bit\r\n\r\n<p>{b}</p>\r\n--inner--\r\n--outer\r\nContent-Type: image/png\r\nContent-Transfer-Encoding: binary\r\n\r\nopaqueZ\r\n--outer--\r\n").into_bytes();
            let control=RunControl::default();let ours=parse(&wire,&control).unwrap();
            let independent=MessageParser::default().parse(&wire).unwrap();
            let mut actual:Vec<_>=ours.nodes.iter().filter(|node|matches!(node.kind,Kind::Leaf)).map(|node|(node.media.clone(),node.body.clone())).collect();
            let mut expected:Vec<_>=independent.parts.iter().filter(|part|!matches!(part.body,PartType::Multipart(_) | PartType::Message(_))).map(|part| {
                let media=part.content_type().unwrap();
                (format!("{}/{}",media.c_type,media.c_subtype.as_deref().unwrap()),part.offset_body as usize..part.offset_end as usize)
            }).collect();
            actual.sort_by_key(|(_,span)|span.start); expected.sort_by_key(|(_,span)|span.start);
            prop_assert_eq!(actual.len(),3);prop_assert_eq!(actual.len(),expected.len());
            for ((media,span),(other,other_span)) in actual.iter().zip(&expected) {
                prop_assert_eq!(media,other);prop_assert_eq!(span,other_span);
                prop_assert_eq!(&wire[span.clone()],&wire[other_span.clone()]);
            }
        }
    }
}
