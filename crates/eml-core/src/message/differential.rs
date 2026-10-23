use super::*;
use crate::RunControl;
use mail_parser::{MessageParser, MimeHeaders, PartType};
use proptest::prelude::*;
#[path = "../../tests/support/property_config.rs"]
mod property_config;
static DIFFERENTIAL_SEEDS: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    property_config::seed_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        "differential.txt",
    )
});
proptest! {
    #![proptest_config(property_config::bounded(ProptestConfig { cases:64, ..ProptestConfig::default() }, &DIFFERENTIAL_SEEDS))]
    #[test]
    fn independent_parser_agrees_on_crlf_identity_leaf_types_and_raw_spans(a in "[A-Za-z0-9 =]{0,128}", b in "[A-Za-z0-9 =]{0,128}") {
        // Matching syntax subset only: explicit ASCII types, CRLF, identity CTE,
        // unrelated safe boundaries. Forwarded coordinates and prefix-related
        // boundaries have independent exact-span controls below.
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

#[derive(Debug, PartialEq, Eq)]
struct SourceLeaf {
    path: Vec<usize>,
    media: String,
    entity: Span,
    body: Span,
}

fn literal_span(source: &[u8], literal: &[u8]) -> Span {
    let positions: Vec<_> = source
        .windows(literal.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == literal).then_some(offset))
        .collect();
    assert_eq!(
        positions.len(),
        1,
        "independent fixture literal must be unique"
    );
    positions[0]..positions[0] + literal.len()
}

fn comparator_leaves(
    message: &mail_parser::Message<'_>,
    id: usize,
    source: &[u8],
    path: &mut Vec<usize>,
    leaves: &mut Vec<SourceLeaf>,
    containers: &mut Vec<(Vec<usize>, String)>,
) -> usize {
    // mail-parser 0.11.9 keeps the complete original raw_message and absolute
    // source offsets in its identity-encoded forwarded-message branch. A nested
    // message has the SAME origin, not parent.offset_body. Assert that API premise
    // rather than accepting decoded/local coordinates or guessing a correction.
    assert_eq!(message.raw_message.as_ref(), source);
    let part = &message.parts[id];
    assert!(!part.is_encoding_problem);
    let media = part.content_type().unwrap();
    let media = format!("{}/{}", media.c_type, media.c_subtype.as_deref().unwrap());
    match &part.body {
        PartType::Message(nested) => {
            assert!(matches!(
                media.as_str(),
                "message/rfc822" | "message/global"
            ));
            assert!(nested.parts[0].offset_header >= part.offset_body);
            assert!(nested.parts[0].offset_end <= part.offset_end);
            containers.push((path.clone(), media));
            path.push(0); // The embedded physical root, not a multipart sibling.
            let forwarded = 1 + comparator_leaves(nested, 0, source, path, leaves, containers);
            path.pop();
            forwarded
        }
        PartType::Multipart(children) => {
            containers.push((path.clone(), media));
            let mut forwarded = 0;
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                forwarded +=
                    comparator_leaves(message, *child as usize, source, path, leaves, containers);
                path.pop();
            }
            forwarded
        }
        _ => {
            leaves.push(SourceLeaf {
                path: path.clone(),
                media,
                entity: part.offset_header as usize..part.offset_end as usize,
                body: part.offset_body as usize..part.offset_end as usize,
            });
            0
        }
    }
}

fn converter_leaves(
    message: &Message<'_>,
    id: usize,
    path: &mut Vec<usize>,
    leaves: &mut Vec<SourceLeaf>,
    containers: &mut Vec<(Vec<usize>, String)>,
) -> usize {
    let node = &message.nodes[id];
    match &node.kind {
        Kind::Message(child) => {
            containers.push((path.clone(), node.media.clone()));
            path.push(0);
            let forwarded = 1 + converter_leaves(message, *child, path, leaves, containers);
            path.pop();
            forwarded
        }
        Kind::Multipart { children, .. } => {
            containers.push((path.clone(), node.media.clone()));
            let mut forwarded = 0;
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                forwarded += converter_leaves(message, child.id, path, leaves, containers);
                path.pop();
            }
            forwarded
        }
        Kind::Leaf => {
            leaves.push(SourceLeaf {
                path: path.clone(),
                media: node.media.clone(),
                entity: node.span.clone(),
                body: node.body.clone(),
            });
            0
        }
    }
}

fn compare_literal_leaves(
    source: &[u8],
    literals: &[(&str, &str, &str, &[usize])],
    containers: &[(&[usize], &str)],
    forwarded: usize,
) {
    // Literal source bytes, parent paths, container media and ranges are an
    // independent fixture oracle. Neither parser's attachment/body preference
    // supplies this application's removal-policy expectations.
    let expected: Vec<_> = literals
        .iter()
        .map(|(media, entity, body, path)| SourceLeaf {
            path: path.to_vec(),
            media: (*media).to_owned(),
            entity: literal_span(source, entity.as_bytes()),
            body: literal_span(source, body.as_bytes()),
        })
        .collect();
    let expected_containers: Vec<_> = containers
        .iter()
        .map(|(path, media)| (path.to_vec(), (*media).to_owned()))
        .collect();
    let ours = parse(source, &RunControl::default()).unwrap();
    let mut actual = Vec::new();
    let mut actual_containers = Vec::new();
    assert_eq!(
        converter_leaves(
            &ours,
            0,
            &mut Vec::new(),
            &mut actual,
            &mut actual_containers
        ),
        forwarded
    );
    let independent = MessageParser::default().parse(source).unwrap();
    let mut comparison = Vec::new();
    let mut comparison_containers = Vec::new();
    assert_eq!(
        comparator_leaves(
            &independent,
            0,
            source,
            &mut Vec::new(),
            &mut comparison,
            &mut comparison_containers
        ),
        forwarded
    );
    assert_eq!(
        actual, expected,
        "converter exact literal entity/body spans and parent paths"
    );
    assert_eq!(
        comparison, expected,
        "independent parser exact literal entity/body spans and parent paths"
    );
    assert_eq!(
        actual_containers, expected_containers,
        "converter container topology"
    );
    assert_eq!(
        comparison_containers, expected_containers,
        "independent parser container topology"
    );
}

#[test]
fn independent_parser_maps_identity_forwarded_messages_to_exact_source_spans() {
    let plain =
        "Content-Type: text/plain\r\nContent-Transfer-Encoding: 7bit\r\n\r\nforwarded-plainZ";
    let html =
        "Content-Type: text/html\r\nContent-Transfer-Encoding: 8bit\r\n\r\n<p>forwarded-htmlZ</p>";
    let outside =
        "Content-Type: image/png\r\nContent-Transfer-Encoding: binary\r\n\r\noutside-imageZ";
    for media in ["message/rfc822", "message/global"] {
        for encoding in ["7bit", "8bit", "binary"] {
            let embedded = format!(
                "MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=forward-inner\r\n\r\n--forward-inner\r\n{plain}\r\n--forward-inner\r\n{html}\r\n--forward-inner--"
            );
            let wire = format!(
                "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=forward-outer\r\n\r\n--forward-outer\r\nContent-Type: {media}\r\nContent-Transfer-Encoding: {encoding}\r\n\r\n{embedded}\r\n--forward-outer\r\n{outside}\r\n--forward-outer--\r\n"
            );
            let embedded_span = literal_span(wire.as_bytes(), embedded.as_bytes());
            assert!(
                embedded_span.start > 0,
                "forwarded origin must differ from source origin"
            );
            compare_literal_leaves(
                wire.as_bytes(),
                &[
                    ("text/plain", plain, "forwarded-plainZ", &[0, 0, 0]),
                    ("text/html", html, "<p>forwarded-htmlZ</p>", &[0, 0, 1]),
                    ("image/png", outside, "outside-imageZ", &[1]),
                ],
                &[
                    (&[], "multipart/mixed"),
                    (&[0], media),
                    (&[0, 0], "multipart/alternative"),
                ],
                1,
            );
        }
    }
}

#[test]
fn independent_parser_agrees_on_distinct_prefix_boundaries_and_exact_source_spans() {
    let plain = "Content-Type: text/plain\r\n\r\nprefix-plainZ";
    let html = "Content-Type: text/html\r\n\r\n<p>prefix-htmlZ</p>";
    let outside = "Content-Type: image/png\r\n\r\nprefix-imageZ";
    // RFC 2046 section 5.1 forbids an inner boundary beginning with its
    // enclosing boundary. The reverse prefix orientation is conforming.
    // B/BB tolerance and B/B-- ambiguity are not standards-subset oracles.
    let (outer, inner) = ("BB", "B");
    let wire = format!(
        "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary={outer}\r\n\r\n--{outer}\r\nContent-Type: multipart/alternative; boundary={inner}\r\n\r\n--{inner}\r\n{plain}\r\n--{inner}\r\n{html}\r\n--{inner}--\r\n--{outer}\r\n{outside}\r\n--{outer}--\r\n"
    );
    compare_literal_leaves(
        wire.as_bytes(),
        &[
            ("text/plain", plain, "prefix-plainZ", &[0, 0]),
            ("text/html", html, "<p>prefix-htmlZ</p>", &[0, 1]),
            ("image/png", outside, "prefix-imageZ", &[1]),
        ],
        &[(&[], "multipart/mixed"), (&[0], "multipart/alternative")],
        0,
    );
}
