//! Independent semantic tree -> wire/expected-copy oracle. No production parser,
//! policy constants, classification helpers or edit vectors are used by the oracle.
use eml_core::{
    PreparationError, Refusal, RemovalReason, RemovedPart, RunControl, prepare_message,
};
use proptest::prelude::*;
use std::collections::BTreeSet;
#[derive(Clone, Debug)]
enum Shape {
    Leaf(u8, Vec<u8>),
    Multipart(u8, bool, Vec<Model>),
    Forward(bool, Box<Model>),
}
#[derive(Clone, Debug)]
struct Model {
    shape: Shape,
    attachment: bool,
    stale: bool,
    transport: u8,
}
fn models() -> BoxedStrategy<Model> {
    let payload = prop::collection::vec(
        prop::sample::select(vec![0, 255, b'a', b'=', b'<', b'>', 0xc3, 0x85]),
        0..24,
    );
    let leaf = (0u8..7, payload, any::<bool>(), any::<bool>(), 0u8..3).prop_map(
        |(kind, payload, attachment, stale, transport)| Model {
            shape: Shape::Leaf(kind, payload),
            attachment,
            stale,
            transport,
        },
    );
    leaf.prop_recursive(4, 64, 4, |inner| {
        prop_oneof![
            (
                0u8..6,
                any::<bool>(),
                prop::collection::vec(inner.clone(), 1..4),
                any::<bool>(),
                any::<bool>(),
                0u8..3
            )
                .prop_map(|(kind, last, children, attachment, stale, transport)| {
                    Model {
                        shape: Shape::Multipart(kind, last, children),
                        attachment,
                        stale,
                        transport,
                    }
                }),
            (any::<bool>(), inner, any::<bool>(), any::<bool>(), 0u8..3).prop_map(
                |(identity, child, attachment, stale, transport)| Model {
                    shape: Shape::Forward(identity, Box::new(child)),
                    attachment,
                    stale,
                    transport
                }
            ),
        ]
    })
    .boxed()
}
struct Rendered {
    wire: Vec<u8>,
    fields: Vec<(&'static str, Vec<u8>)>,
    separator: Vec<u8>,
    media: &'static str,
    attachment: bool,
    root: bool,
    section: String,
    kind: RenderedKind,
}
enum RenderedKind {
    Leaf,
    Multi {
        prefix: Vec<u8>,
        frames: Vec<(Vec<u8>, Rendered, Vec<u8>)>,
        suffix: Vec<u8>,
        selected: Option<usize>,
    },
    Forward {
        identity: bool,
        child: Box<Rendered>,
    },
}
fn indexed(prefix: &str, index: usize) -> String {
    if prefix.is_empty() {
        index.to_string()
    } else {
        format!("{prefix}.{index}")
    }
}
fn render(
    model: &Model,
    root: bool,
    section: String,
    cid: Option<String>,
    omit_type: bool,
    counter: &mut usize,
) -> Rendered {
    let ending = match model.transport {
        0 => b"\r\n".as_slice(),
        1 => b"\n".as_slice(),
        _ => b"\r".as_slice(),
    };
    let number = *counter;
    *counter += 1;
    let boundary = format!("model_{number}_end");
    let media = match &model.shape {
        Shape::Leaf(kind, _) => match kind {
            0 => "text/plain",
            1 => "text/html",
            2 => "image/png",
            3 => "text/calendar",
            4 => "application/pkcs7-mime",
            5 => "multipart/signed",
            _ => "application/pdf",
        },
        Shape::Multipart(kind, ..) => match kind {
            0 => "multipart/mixed",
            1 => "multipart/alternative",
            2 => "multipart/related",
            3 => "multipart/digest",
            4 => "multipart/report",
            _ => "multipart/x-other",
        },
        Shape::Forward(..) => "message/rfc822",
    };
    let mut fields = Vec::new();
    let mut add = |name: &'static str, value: String| {
        let mut bytes = format!("{name}: {value}").into_bytes();
        bytes.extend(ending);
        fields.push((name, bytes));
    };
    let mut content_type = media.to_owned();
    if let Shape::Multipart(kind, last, children) = &model.shape {
        content_type.push_str(&format!("; boundary={boundary}"));
        if *kind == 2 && *last {
            content_type.push_str(&format!("; start=\"<child_{}>\"", children.len()));
        }
    }
    if !omit_type {
        add("Content-Type", content_type);
    }
    if model.attachment {
        add(
            "Content-Disposition",
            "attachment; filename=part.bin".into(),
        );
    }
    if let Some(cid) = cid {
        add("Content-ID", format!("<{cid}>"));
    }
    if let Shape::Forward(identity, _) = &model.shape {
        add(
            "Content-Transfer-Encoding",
            if *identity { "8bit" } else { "base64" }.into(),
        );
    }
    if model.stale {
        for name in [
            "Content-Length",
            "Lines",
            "Content-MD5",
            "DKIM-Signature",
            "DomainKey-Signature",
            "ARC-Seal",
            "ARC-Message-Signature",
            "ARC-Authentication-Results",
            "X-MS-Has-Attach",
        ] {
            add(name, "stale-model-value".into());
        }
    }
    // Keep a header when a digest child omits Content-Type.
    add("Subject", "synthetic model entity".into());
    let mut body = Vec::new();
    let kind = match &model.shape {
        Shape::Leaf(_, payload) => {
            body.extend(payload);
            RenderedKind::Leaf
        }
        Shape::Multipart(kind, last, children) => {
            let mut prefix = b"model preamble".to_vec();
            prefix.extend(ending);
            body.extend(&prefix);
            let mut frames = Vec::new();
            for (index, child) in children.iter().enumerate() {
                let opening = format!("--{boundary} \t")
                    .into_bytes()
                    .into_iter()
                    .chain(ending.iter().copied())
                    .collect::<Vec<_>>();
                let child = render(
                    child,
                    false,
                    indexed(&section, index + 1),
                    (*kind == 2).then(|| format!("child_{}", index + 1)),
                    *kind == 3 && matches!(child.shape, Shape::Forward(..)),
                    counter,
                );
                body.extend(&opening);
                body.extend(&child.wire);
                // A CR-ending empty child followed by an LF-only parent frame
                // would merge into one CRLF and change entity ownership. Keep
                // this model's frames independently delimited; mixed transport
                // still occurs in the child and container's own physical lines.
                body.extend(b"\r\n");
                frames.push((opening, child, b"\r\n".to_vec()));
            }
            let mut suffix = format!("--{boundary}--\t").into_bytes();
            suffix.extend(ending);
            suffix.extend(b"model epilogue");
            body.extend(&suffix);
            RenderedKind::Multi {
                prefix,
                frames,
                suffix,
                selected: (*kind == 2).then_some(if *last { children.len() - 1 } else { 0 }),
            }
        }
        Shape::Forward(identity, child) => {
            let child_section = if matches!(child.shape, Shape::Multipart(..)) {
                section.clone()
            } else {
                indexed(&section, 1)
            };
            let child = Box::new(render(child, true, child_section, None, false, counter));
            body.extend(&child.wire);
            RenderedKind::Forward {
                identity: *identity,
                child,
            }
        }
    };
    let mut wire = Vec::new();
    for (_, field) in &fields {
        wire.extend(field);
    }
    wire.extend(ending);
    wire.extend(&body);
    Rendered {
        wire,
        fields,
        separator: ending.to_vec(),
        media,
        attachment: model.attachment,
        root,
        section,
        kind,
    }
}
struct Expected {
    copy: Option<Vec<u8>>,
    removals: Vec<RemovedPart>,
    stripped: BTreeSet<String>,
}
fn removal(node: &Rendered, reason: RemovalReason) -> Expected {
    Expected {
        copy: None,
        removals: vec![RemovedPart {
            section: node.section.clone(),
            content_type: node.media.into(),
            filename: node.attachment.then(|| "part.bin".into()),
            bytes: node.wire.len() as u64,
            reason,
        }],
        stripped: BTreeSet::new(),
    }
}
fn expected(node: &Rendered) -> Result<Expected, Refusal> {
    if node.attachment && !node.root {
        return Ok(removal(node, RemovalReason::Attachment));
    }
    if ["multipart/signed", "application/pkcs7-mime"].contains(&node.media) {
        return Err(Refusal::Protected);
    }
    if ["text/plain", "text/html"].contains(&node.media) {
        return Ok(Expected {
            copy: Some(node.wire.clone()),
            removals: vec![],
            stripped: BTreeSet::new(),
        });
    }
    let mut body = Vec::new();
    let mut removals = Vec::new();
    let mut stripped = BTreeSet::new();
    let mut body_count = 0;
    let mut changed = false;
    match &node.kind {
        RenderedKind::Leaf => return Ok(removal(node, RemovalReason::NonBodyPart)),
        RenderedKind::Forward {
            identity: false, ..
        } => return Ok(removal(node, RemovalReason::NonBodyPart)),
        RenderedKind::Forward {
            identity: true,
            child,
        } => {
            let result = expected(child)?;
            removals = result.removals;
            stripped = result.stripped;
            if let Some(copy) = result.copy {
                changed = copy != child.wire;
                body = copy;
                body_count = 1;
            }
        }
        RenderedKind::Multi {
            prefix,
            frames,
            suffix,
            selected,
        } => {
            body.extend(prefix);
            for (index, (opening, child, framing)) in frames.iter().enumerate() {
                let result = if selected.is_some_and(|selected| selected != index) {
                    removal(child, RemovalReason::RelatedResource)
                } else {
                    expected(child)?
                };
                removals.extend(result.removals);
                stripped.extend(result.stripped);
                if let Some(copy) = result.copy {
                    changed |= copy != child.wire;
                    body.extend(opening);
                    body.extend(copy);
                    body.extend(framing);
                    body_count += 1;
                } else {
                    changed = true;
                }
            }
            body.extend(suffix);
        }
    }
    if body_count == 0 {
        return Ok(Expected {
            copy: None,
            removals,
            stripped: BTreeSet::new(),
        });
    }
    let mut copy = Vec::new();
    for (name, wire) in &node.fields {
        let common = ["Content-Length", "Lines", "Content-MD5"].contains(name);
        let root_only = [
            "DKIM-Signature",
            "DomainKey-Signature",
            "ARC-Seal",
            "ARC-Message-Signature",
            "ARC-Authentication-Results",
            "X-MS-Has-Attach",
        ]
        .contains(name);
        if changed && (common || (node.root && root_only)) {
            stripped.insert(name.to_ascii_lowercase());
        } else {
            copy.extend(wire);
        }
    }
    copy.extend(&node.separator);
    copy.extend(body);
    Ok(Expected {
        copy: Some(copy),
        removals,
        stripped,
    })
}
proptest! {
    #![proptest_config(ProptestConfig {cases:256,failure_persistence:Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(concat!(env!("CARGO_MANIFEST_DIR"),"/../../target/proptest/model.txt")))),..ProptestConfig::default()})]
    #[test] fn semantic_model_preserves_all_authorized_bytes(model in models()) {
        let rendered=render(&model,true,String::new(),None,false,&mut 0);
        let oracle=expected(&rendered);let actual=prepare_message(&rendered.wire,&RunControl::default());
        match oracle {
            Err(kind)=>prop_assert!(matches!(actual,Err(PreparationError::Refused{kind:actual,..}) if actual==kind), "Expected the model refusal"),
            Ok(Expected{copy:None,..})=>prop_assert!(matches!(actual,Err(PreparationError::Refused{kind:Refusal::NoBody,..})), "Expected no supported body"),
            Ok(Expected{copy:Some(copy),removals,stripped})=>{
                let actual=actual.map_err(|error|TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(actual.as_bytes(),copy.as_slice());prop_assert_eq!(actual.removed(),removals.as_slice());
                let stripped: Vec<_> = stripped.into_iter().collect();
                prop_assert_eq!(actual.stripped_fields(),stripped.as_slice());
                let again=prepare_message(actual.as_bytes(),&RunControl::default()).map_err(|error|TestCaseError::fail(error.to_string()))?;
                prop_assert_eq!(again.as_bytes(),actual.as_bytes());prop_assert!(again.removed().is_empty());prop_assert!(again.stripped_fields().is_empty());
            }
        }
    }
}
