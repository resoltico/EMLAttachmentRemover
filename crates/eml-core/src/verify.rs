//! Independent source policy and expected-byte reconstruction. This module does not
//! call the planner, consume its eligibility flags, edits or ancestor bookkeeping.
use crate::{
    PreparationError, Refusal, RemovalReason, RunControl, error, grammar,
    message::{Kind, Message, PROTECTED},
    plan::{CONTAINER_STALE, Claim, ROOT_STALE},
};
use std::collections::BTreeSet;
struct Proof {
    id: usize,
    changed: bool,
    children: Vec<Proof>,
}
struct Evaluated {
    bytes: Option<Vec<u8>>,
    proof: Option<Proof>,
    claims: Vec<Claim>,
    stripped: BTreeSet<String>,
}
// No identity-free Default exists: bytes and the retained-structure proof must
// be independently derived from this message. The generated FnValue mutation
// Ok(Default::default()) is therefore compiler-unviable, not caught by tests.
pub(crate) struct Authorization {
    bytes: Vec<u8>,
    proof: Proof,
    pub(crate) stripped: BTreeSet<String>,
}
fn internal() -> PreparationError {
    error(
        Refusal::Internal,
        "Independent MIME authorization or byte/structure verification failed",
    )
}
fn removed(id: usize, reason: RemovalReason) -> Evaluated {
    Evaluated {
        bytes: None,
        proof: None,
        claims: vec![Claim { id, reason }],
        stripped: BTreeSet::new(),
    }
}
fn independent(
    message: &Message<'_>,
    id: usize,
    control: &RunControl,
) -> Result<Evaluated, PreparationError> {
    control.checkpoint()?;
    let node = &message.nodes[id];
    // Deliberately rederive eligibility from immutable facts, not planner actions.
    if node.disposition.as_deref() == Some("attachment") && !node.root {
        return Ok(removed(id, RemovalReason::Attachment));
    }
    if PROTECTED.iter().any(|media| *media == node.media) {
        return Err(internal());
    }
    if node.media == "text/plain" || node.media == "text/html" {
        return Ok(Evaluated {
            bytes: Some(message.input[node.span.clone()].to_vec()),
            proof: Some(Proof {
                id,
                changed: false,
                children: Vec::new(),
            }),
            claims: Vec::new(),
            stripped: BTreeSet::new(),
        });
    }
    let mut claims = Vec::new();
    let mut stripped = BTreeSet::new();
    let mut proofs = Vec::new();
    let mut body = Vec::new();
    let mut changed = false;
    if node.media.starts_with("multipart/") {
        let Kind::Multipart {
            prefix,
            children,
            suffix,
        } = &node.kind
        else {
            return Err(internal());
        };
        let root = if node.media == "multipart/related" {
            related_root(message, node, children)?
        } else {
            usize::MAX
        };
        body.extend_from_slice(&message.input[prefix.clone()]);
        for child in children {
            let result = if node.media == "multipart/related" && root != child.id {
                removed(child.id, RemovalReason::RelatedResource)
            } else {
                independent(message, child.id, control)?
            };
            claims.extend(result.claims);
            stripped.extend(result.stripped);
            if let Some(bytes) = result.bytes {
                let proof = result.proof.ok_or_else(internal)?;
                changed |= proof.changed;
                body.extend_from_slice(&message.input[child.opening.clone()]);
                body.extend(bytes);
                body.extend_from_slice(&message.input[child.framing.clone()]);
                proofs.push(proof);
            } else {
                changed = true;
            }
        }
        body.extend_from_slice(&message.input[suffix.clone()]);
    } else if (node.media == "message/rfc822" || node.media == "message/global")
        && node
            .encoding
            .as_deref()
            .is_some_and(|encoding| ["7bit", "8bit", "binary"].contains(&encoding))
    {
        let Kind::Message(child) = node.kind else {
            return Err(internal());
        };
        let result = independent(message, child, control)?;
        claims.extend(result.claims);
        stripped.extend(result.stripped);
        if let Some(bytes) = result.bytes {
            let proof = result.proof.ok_or_else(internal)?;
            changed |= proof.changed;
            body = bytes;
            proofs.push(proof);
        }
    } else {
        return Ok(removed(id, RemovalReason::NonBodyPart));
    }
    if proofs.is_empty() {
        return Ok(Evaluated {
            bytes: None,
            proof: None,
            claims,
            stripped: BTreeSet::new(),
        });
    }
    rebuild_retained_node(message, id, changed, body, proofs, claims, stripped)
}

fn rebuild_retained_node(
    message: &Message<'_>,
    id: usize,
    changed: bool,
    body: Vec<u8>,
    proofs: Vec<Proof>,
    claims: Vec<Claim>,
    mut stripped: BTreeSet<String>,
) -> Result<Evaluated, PreparationError> {
    let node = &message.nodes[id];
    let mut output = Vec::new();
    output.extend_from_slice(&message.input[node.envelope.clone()]);
    for field in &node.fields {
        // Independently derive this node's affected ancestor state from surviving results.
        let stale = changed
            && (CONTAINER_STALE.iter().any(|name| *name == field.name)
                || (node.root && ROOT_STALE.iter().any(|name| *name == field.name)));
        if stale {
            stripped.insert(field.name.clone());
        } else {
            output.extend_from_slice(&message.input[field.span.clone()]);
        }
    }
    output.extend_from_slice(&message.input[node.separator.clone()]);
    output.extend(body);
    Ok(Evaluated {
        bytes: Some(output),
        proof: Some(Proof {
            id,
            changed,
            children: proofs,
        }),
        claims,
        stripped,
    })
}
fn related_root(
    message: &Message<'_>,
    node: &crate::message::Node,
    children: &[crate::message::Child],
) -> Result<usize, PreparationError> {
    Ok(
        match grammar::parameter_value(&node.content_type.parameters, b"start", true)? {
            None => children
                .first()
                .map(|child| child.id)
                .ok_or_else(internal)?,
            Some(grammar::DecodedParameter { bytes: start, .. }) => {
                let requested = grammar::identifier(&start)?;
                let mut matches = Vec::new();
                for child in children {
                    let fields: Vec<_> = message.nodes[child.id]
                        .fields
                        .iter()
                        .filter(|field| field.name == "content-id")
                        .collect();
                    if fields.len() > 1 {
                        return Err(internal());
                    }
                    if let Some(field) = fields.first()
                        && grammar::identifier(&field.value)? == requested
                    {
                        matches.push(child.id);
                    }
                }
                if matches.len() != 1 {
                    return Err(internal());
                }
                matches[0]
            }
        },
    )
}
/// Actual authorization guard, targeted by the bounded mutation selection.
pub(crate) fn authorize_deletions(
    message: &Message<'_>,
    claims: &[Claim],
    control: &RunControl,
) -> Result<Authorization, PreparationError> {
    let permitted = independent(message, 0, control)?;
    if claims.len() != permitted.claims.len() {
        return Err(internal());
    }
    for (claim, expected) in claims.iter().zip(&permitted.claims) {
        if claim.id != expected.id || claim.reason != expected.reason {
            return Err(internal());
        }
    }
    let bytes = permitted.bytes.ok_or_else(internal)?;
    let proof = permitted.proof.ok_or_else(internal)?;
    Ok(Authorization {
        bytes,
        proof,
        stripped: permitted.stripped,
    })
}
fn compare(
    source: &Message<'_>,
    proof: &Proof,
    output: &Message<'_>,
    output_id: usize,
    control: &RunControl,
) -> Result<(), PreparationError> {
    control.checkpoint()?;
    let original = &source.nodes[proof.id];
    let candidate = &output.nodes[output_id];
    if original.media != candidate.media
        || original.root != candidate.root
        || source.input[original.envelope.clone()] != output.input[candidate.envelope.clone()]
        || source.input[original.separator.clone()] != output.input[candidate.separator.clone()]
    {
        return Err(internal());
    }
    let fields: Vec<_> = original
        .fields
        .iter()
        .filter(|field| {
            !(proof.changed
                && (CONTAINER_STALE.contains(&field.name.as_str())
                    || (original.root && ROOT_STALE.contains(&field.name.as_str()))))
        })
        .collect();
    if fields.len() != candidate.fields.len() {
        return Err(internal());
    }
    for (expected, actual) in fields.iter().zip(&candidate.fields) {
        if source.input[expected.span.clone()] != output.input[actual.span.clone()] {
            return Err(internal());
        }
    }
    match (&original.kind, &candidate.kind) {
        (Kind::Leaf, Kind::Leaf) => {
            if source.input[original.body.clone()] != output.input[candidate.body.clone()] {
                return Err(internal());
            }
        }
        (Kind::Message(_), Kind::Message(child)) => {
            if proof.children.len() != 1 {
                return Err(internal());
            }
            compare(source, &proof.children[0], output, *child, control)?;
        }
        (
            Kind::Multipart {
                prefix,
                children,
                suffix,
            },
            Kind::Multipart {
                prefix: out_prefix,
                children: out_children,
                suffix: out_suffix,
            },
        ) => {
            if source.input[prefix.clone()] != output.input[out_prefix.clone()]
                || source.input[suffix.clone()] != output.input[out_suffix.clone()]
                || proof.children.len() != out_children.len()
            {
                return Err(internal());
            }
            let mut source_frames = children.iter();
            for (kept, actual) in proof.children.iter().zip(out_children) {
                let expected = source_frames
                    .by_ref()
                    .find(|frame| frame.id == kept.id)
                    .ok_or_else(internal)?;
                if source.input[expected.opening.clone()] != output.input[actual.opening.clone()]
                    || source.input[expected.framing.clone()]
                        != output.input[actual.framing.clone()]
                {
                    return Err(internal());
                }
                compare(source, kept, output, actual.id, control)?;
            }
        }
        _ => return Err(internal()),
    }
    Ok(())
}
pub(crate) fn verify_candidate(
    source: &Message<'_>,
    authorization: &Authorization,
    bytes: &[u8],
    control: &RunControl,
) -> Result<(), PreparationError> {
    if authorization.bytes != bytes {
        return Err(internal());
    }
    let output = crate::message::parse(bytes, control).map_err(|failure| {
        if failure == PreparationError::Interrupted {
            failure
        } else {
            internal()
        }
    })?;
    compare(source, &authorization.proof, &output, 0, control)?;
    let resulting_policy = independent(&output, 0, control)?;
    if !resulting_policy.claims.is_empty()
        || !resulting_policy.stripped.is_empty()
        || resulting_policy.bytes.as_deref() != Some(bytes)
    {
        return Err(internal());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source(control: &RunControl) -> Message<'static> {
        crate::message::parse(b"Content-Type: multipart/alternative; boundary=B\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nplain\r\n--B\r\nContent-Type: text/html\r\n\r\n<p>html</p>\r\n--B--\r\n", control).unwrap()
    }
    #[test]
    fn rejects_forged_kept_html_even_when_plain_survives() {
        let control = RunControl::default();
        let message = source(&control);
        for reason in [
            RemovalReason::Attachment,
            RemovalReason::RelatedResource,
            RemovalReason::NonBodyPart,
        ] {
            assert!(authorize_deletions(&message, &[Claim { id: 2, reason }], &control).is_err());
        }
        assert!(authorize_deletions(&message, &[], &control).is_ok());
    }
    #[test]
    fn rejects_unowned_field_and_delimiter_byte_deletions() {
        let control = RunControl::default();
        let message = source(&control);
        let allowed = authorize_deletions(&message, &[], &control).unwrap();
        for index in [0, 47, 59, message.input.len() - 1] {
            let mut corrupted = message.input.to_vec();
            corrupted.remove(index);
            assert!(verify_candidate(&message, &allowed, &corrupted, &control).is_err());
        }
    }
}

#[cfg(test)]
mod claim_tests {
    use super::*;
    #[test]
    fn exact_claim_sequence_rejects_missing_extra_duplicate_reordered_wrong_reason_and_ids() {
        let control = RunControl::default();
        let message=crate::message::parse(b"Content-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\n\r\nbody\r\n--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment\r\n\r\npdf\r\n--B\r\nContent-Type: image/png\r\n\r\npng\r\n--B--\r\n",&control).unwrap();
        let attachment = Claim {
            id: 2,
            reason: RemovalReason::Attachment,
        };
        let non_body = Claim {
            id: 3,
            reason: RemovalReason::NonBodyPart,
        };
        assert!(
            authorize_deletions(&message, &[attachment.clone(), non_body.clone()], &control)
                .is_ok()
        );
        for claims in [
            vec![],
            vec![attachment.clone()],
            vec![attachment.clone(), non_body.clone(), non_body.clone()],
            vec![attachment.clone(), attachment.clone()],
            vec![non_body.clone(), attachment.clone()],
            vec![
                Claim {
                    id: 2,
                    reason: RemovalReason::RelatedResource,
                },
                non_body.clone(),
            ],
            vec![
                Claim {
                    id: 1,
                    reason: RemovalReason::NonBodyPart,
                },
                non_body.clone(),
            ],
            vec![
                Claim {
                    id: usize::MAX,
                    reason: RemovalReason::Attachment,
                },
                non_body,
            ],
        ] {
            assert!(
                authorize_deletions(&message, &claims, &control).is_err(),
                "{claims:?}"
            );
        }
    }
}
