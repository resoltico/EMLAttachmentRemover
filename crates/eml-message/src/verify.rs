//! Independent retained-tree authorization and forward source-span proof.
//! Planner edits and retention decisions are never inputs to this derivation.
use crate::{
    PreparationError, Refusal, RemovalReason, RunControl, error, grammar,
    message::{Kind, Message, PROTECTED, Span, UNSUPPORTED_BODY_MEDIA},
    plan::{CONTAINER_STALE, Claim, ROOT_STALE},
};
use std::collections::BTreeSet;
struct RetainedNode {
    changed: bool,
    children: Vec<usize>,
}
// No identity-free Default exists. This proof is derived only from source facts.
pub(crate) struct Authorization {
    nodes: Vec<Option<RetainedNode>>,
    claims: Vec<Claim>,
    expected_length: usize,
    pub(crate) stripped: BTreeSet<String>,
}
fn internal() -> PreparationError {
    error(
        Refusal::Internal,
        "Independent MIME authorization or byte/structure verification failed",
    )
}
fn stale(message: &Message<'_>, id: usize, changed: bool, name: &str) -> bool {
    changed
        && (CONTAINER_STALE.contains(&name)
            || (message.nodes[id].root && ROOT_STALE.contains(&name)))
}
enum Evaluation {
    Visit(usize, Option<RemovalReason>),
    Finish(usize),
}
fn independent(
    message: &Message<'_>,
    control: &RunControl,
) -> Result<Authorization, PreparationError> {
    let mut proof = Authorization {
        nodes: (0..message.nodes.len()).map(|_| None).collect(),
        claims: Vec::new(),
        stripped: BTreeSet::new(),
        expected_length: 0,
    };
    let mut stack = vec![Evaluation::Visit(0, None)];
    while let Some(task) = stack.pop() {
        control.checkpoint()?;
        #[cfg(test)]
        crate::work_bounds::record(crate::work_bounds::Work::Node, 1, control)?;
        match task {
            Evaluation::Visit(id, forced) => {
                let node = &message.nodes[id];
                let reason = forced.or_else(|| {
                    (!node.root
                        && node
                            .disposition
                            .as_deref()
                            .is_some_and(|token| token != "inline"))
                    .then_some(RemovalReason::Attachment)
                });
                if let Some(reason) = reason {
                    proof.claims.push(Claim { id, reason });
                    continue;
                }
                if PROTECTED.contains(&node.media.as_str())
                    || UNSUPPORTED_BODY_MEDIA.contains(&node.media.as_str())
                {
                    return Err(internal());
                }
                if matches!(node.media.as_str(), "text/plain" | "text/html") {
                    proof.nodes[id] = Some(RetainedNode {
                        changed: false,
                        children: Vec::new(),
                    });
                    continue;
                }
                match &node.kind {
                    Kind::Multipart { children, .. } if node.media.starts_with("multipart/") => {
                        let selected = if node.media == "multipart/related" {
                            Some(related_root(message, node, children)?)
                        } else {
                            None
                        };
                        stack.push(Evaluation::Finish(id));
                        for child in children.iter().rev() {
                            stack.push(Evaluation::Visit(
                                child.id,
                                selected
                                    .filter(|selected| *selected != child.id)
                                    .map(|_| RemovalReason::RelatedResource),
                            ));
                        }
                    }
                    Kind::Message(child)
                        if matches!(node.media.as_str(), "message/rfc822" | "message/global")
                            && node.encoding.as_deref().is_some_and(|encoding| {
                                ["7bit", "8bit", "binary"].contains(&encoding)
                            }) =>
                    {
                        stack.push(Evaluation::Finish(id));
                        stack.push(Evaluation::Visit(*child, None));
                    }
                    _ => proof.claims.push(Claim {
                        id,
                        reason: RemovalReason::NonBodyPart,
                    }),
                }
            }
            Evaluation::Finish(id) => {
                let children: Vec<_> = match &message.nodes[id].kind {
                    Kind::Multipart { children, .. } => {
                        children.iter().map(|child| child.id).collect()
                    }
                    Kind::Message(child) => vec![*child],
                    _ => return Err(internal()),
                };
                let changed = children
                    .iter()
                    .any(|child| proof.nodes[*child].as_ref().is_none_or(|p| p.changed));
                let retained: Vec<_> = children
                    .into_iter()
                    .filter(|child| proof.nodes[*child].is_some())
                    .collect();
                if !retained.is_empty() {
                    proof.nodes[id] = Some(RetainedNode {
                        changed,
                        children: retained,
                    });
                }
            }
        }
    }
    if proof.nodes.first().is_none_or(Option::is_none) {
        return Err(internal());
    }
    // Only retained ancestors contribute stripped fields. Collapsed containers
    // retain descendant removal claims without retaining their stale-field claims.
    let mut pending = vec![0];
    while let Some(id) = pending.pop() {
        control.checkpoint()?;
        let retained = proof.nodes[id].as_ref().ok_or_else(internal)?;
        for field in &message.nodes[id].fields {
            if stale(message, id, retained.changed, &field.name) {
                proof.stripped.insert(field.name.clone());
            }
        }
        pending.extend(retained.children.iter().rev().copied());
    }
    let mut end = 0;
    let mut length = 0usize;
    for span in RetainedSpans::new(message, &proof) {
        control.checkpoint()?;
        let span = span?;
        if span.start < end || span.end < span.start || span.end > message.input.len() {
            return Err(internal());
        }
        length = length.checked_add(span.len()).ok_or_else(internal)?;
        end = span.end;
    }
    if length > message.input.len() {
        return Err(internal());
    }
    proof.expected_length = length;
    Ok(proof)
}
struct Frame {
    id: usize,
    stage: u8,
    field: usize,
    kept: usize,
    source_child: usize,
}
impl Frame {
    fn new(id: usize) -> Self {
        Self {
            id,
            stage: 0,
            field: 0,
            kept: 0,
            source_child: 0,
        }
    }
}
// One frame per retained ancestor. Siblings are visited on demand, never copied
// into a work stack or reconstructed into message-sized byte buffers.
struct RetainedSpans<'a, 'input> {
    source: &'a Message<'input>,
    proof: &'a Authorization,
    stack: Vec<Frame>,
}
impl<'a, 'input> RetainedSpans<'a, 'input> {
    fn new(source: &'a Message<'input>, proof: &'a Authorization) -> Self {
        Self {
            source,
            proof,
            stack: vec![Frame::new(0)],
        }
    }
}
impl Iterator for RetainedSpans<'_, '_> {
    type Item = Result<Span, PreparationError>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let frame = self.stack.last_mut()?;
            let node = &self.source.nodes[frame.id];
            let Some(proof) = &self.proof.nodes[frame.id] else {
                self.stack.clear();
                return Some(Err(internal()));
            };
            let span = match frame.stage {
                0 => {
                    frame.stage = 1;
                    node.envelope.clone()
                }
                1 => {
                    if let Some(field) = node.fields.get(frame.field) {
                        frame.field += 1;
                        if stale(self.source, frame.id, proof.changed, &field.name) {
                            continue;
                        }
                        field.span.clone()
                    } else {
                        frame.stage = 2;
                        continue;
                    }
                }
                2 => {
                    frame.stage = 3;
                    node.separator.clone()
                }
                3 => match &node.kind {
                    Kind::Leaf => {
                        frame.stage = 7;
                        node.body.clone()
                    }
                    Kind::Message(_) => {
                        frame.stage = 7;
                        let Some(child) = proof.children.first() else {
                            self.stack.clear();
                            return Some(Err(internal()));
                        };
                        self.stack.push(Frame::new(*child));
                        continue;
                    }
                    Kind::Multipart { prefix, .. } => {
                        frame.stage = 4;
                        prefix.clone()
                    }
                },
                4..=6 => {
                    let Kind::Multipart {
                        children, suffix, ..
                    } = &node.kind
                    else {
                        self.stack.clear();
                        return Some(Err(internal()));
                    };
                    if frame.kept == proof.children.len() {
                        frame.stage = 7;
                        suffix.clone()
                    } else {
                        let id = proof.children[frame.kept];
                        while children
                            .get(frame.source_child)
                            .is_some_and(|child| child.id != id)
                        {
                            frame.source_child += 1;
                        }
                        let Some(child) = children.get(frame.source_child) else {
                            self.stack.clear();
                            return Some(Err(internal()));
                        };
                        match frame.stage {
                            4 => {
                                frame.stage = 5;
                                child.opening.clone()
                            }
                            5 => {
                                frame.stage = 6;
                                self.stack.push(Frame::new(id));
                                continue;
                            }
                            _ => {
                                frame.stage = 4;
                                frame.kept += 1;
                                frame.source_child += 1;
                                child.framing.clone()
                            }
                        }
                    }
                }
                _ => {
                    self.stack.pop();
                    continue;
                }
            };
            if !span.is_empty() {
                return Some(Ok(span));
            }
        }
    }
}
/// Actual authorization guard, targeted by the bounded mutation selection.
pub(crate) fn authorize_deletions(
    message: &Message<'_>,
    claims: &[Claim],
    control: &RunControl,
) -> Result<Authorization, PreparationError> {
    let permitted = independent(message, control)?;
    if permitted.claims != claims {
        return Err(internal());
    }
    Ok(permitted)
}
fn equal(left: &[u8], right: &[u8], control: &RunControl) -> Result<bool, PreparationError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (a, b) in left
        .chunks(crate::PREPARATION_CHUNK_BYTES)
        .zip(right.chunks(crate::PREPARATION_CHUNK_BYTES))
    {
        control.checkpoint()?;
        #[cfg(test)]
        crate::work_bounds::record(crate::work_bounds::Work::Comparison, a.len(), control)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn verify_candidate(
    source: &Message<'_>,
    authorization: &Authorization,
    bytes: &[u8],
    control: &RunControl,
) -> Result<(), PreparationError> {
    #[cfg(test)]
    crate::work_bounds::stage(crate::work_bounds::Stage::ExactComparison);
    if authorization.expected_length != bytes.len() {
        return Err(internal());
    }
    let mut offset = 0usize;
    for span in RetainedSpans::new(source, authorization) {
        control.checkpoint()?;
        let span = span?;
        let end = offset.checked_add(span.len()).ok_or_else(internal)?;
        let expected = source.input.get(span).ok_or_else(internal)?;
        let actual = bytes.get(offset..end).ok_or_else(internal)?;
        if !equal(expected, actual, control)? {
            return Err(internal());
        }
        offset = end;
    }
    if offset != bytes.len() {
        return Err(internal());
    }
    #[cfg(test)]
    crate::work_bounds::stage(crate::work_bounds::Stage::CandidateParse);
    let output = crate::message::parse(bytes, control).map_err(|e| {
        if e == PreparationError::Interrupted {
            e
        } else {
            internal()
        }
    })?;
    #[cfg(test)]
    crate::work_bounds::stage(crate::work_bounds::Stage::StructureComparison);
    compare(source, authorization, 0, &output, 0, control)?;
    #[cfg(test)]
    crate::work_bounds::stage(crate::work_bounds::Stage::FixedPoint);
    let fixed = independent(&output, control)?;
    if !fixed.claims.is_empty()
        || !fixed.stripped.is_empty()
        || fixed.expected_length != bytes.len()
    {
        return Err(internal());
    }
    let mut cursor = 0;
    for span in RetainedSpans::new(&output, &fixed) {
        control.checkpoint()?;
        let span = span?;
        if span.start != cursor {
            return Err(internal());
        }
        cursor = span.end;
    }
    if cursor != bytes.len() {
        return Err(internal());
    }
    Ok(())
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

fn compare(
    source: &Message<'_>,
    authorization: &Authorization,
    source_id: usize,
    output: &Message<'_>,
    output_id: usize,
    control: &RunControl,
) -> Result<(), PreparationError> {
    let mut pending = vec![(source_id, output_id)];
    while let Some((source_id, output_id)) = pending.pop() {
        control.checkpoint()?;
        let proof = authorization.nodes[source_id]
            .as_ref()
            .ok_or_else(internal)?;
        let original = &source.nodes[source_id];
        let candidate = &output.nodes[output_id];
        if original.media != candidate.media
            || original.root != candidate.root
            || !equal(
                &source.input[original.envelope.clone()],
                &output.input[candidate.envelope.clone()],
                control,
            )?
            || !equal(
                &source.input[original.separator.clone()],
                &output.input[candidate.separator.clone()],
                control,
            )?
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
            if !equal(
                &source.input[expected.span.clone()],
                &output.input[actual.span.clone()],
                control,
            )? {
                return Err(internal());
            }
        }
        match (&original.kind, &candidate.kind) {
            (Kind::Leaf, Kind::Leaf) => {
                if !equal(
                    &source.input[original.body.clone()],
                    &output.input[candidate.body.clone()],
                    control,
                )? {
                    return Err(internal());
                }
            }
            (Kind::Message(_), Kind::Message(child)) => {
                if proof.children.len() != 1 {
                    return Err(internal());
                }
                pending.push((proof.children[0], *child));
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
                if !equal(
                    &source.input[prefix.clone()],
                    &output.input[out_prefix.clone()],
                    control,
                )? || !equal(
                    &source.input[suffix.clone()],
                    &output.input[out_suffix.clone()],
                    control,
                )? || proof.children.len() != out_children.len()
                {
                    return Err(internal());
                }
                let mut source_frames = children.iter();
                for (kept, actual) in proof.children.iter().zip(out_children) {
                    let expected = source_frames
                        .by_ref()
                        .find(|frame| frame.id == *kept)
                        .ok_or_else(internal)?;
                    if !equal(
                        &source.input[expected.opening.clone()],
                        &output.input[actual.opening.clone()],
                        control,
                    )? || !equal(
                        &source.input[expected.framing.clone()],
                        &output.input[actual.framing.clone()],
                        control,
                    )? {
                        return Err(internal());
                    }
                    pending.push((*kept, actual.id));
                }
            }
            _ => return Err(internal()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn source(control: &RunControl) -> Message<'static> {
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

#[cfg(test)]
mod span_tests {
    use super::*;
    #[test]
    fn corrupt_retained_sequences_cannot_authorize_missing_duplicate_or_reordered_bytes() {
        let control = RunControl::default();
        let message = super::tests::source(&control);
        for mutation in 0..4 {
            let mut proof = authorize_deletions(&message, &[], &control).unwrap();
            let children = &mut proof.nodes[0].as_mut().unwrap().children;
            match mutation {
                0 => {
                    children.remove(0);
                }
                1 => children.push(children[0]),
                2 => children.swap(0, 1),
                _ => {
                    proof.expected_length += 1;
                }
            }
            assert!(verify_candidate(&message, &proof, message.input, &control).is_err());
        }
        let proof = authorize_deletions(&message, &[], &control).unwrap();
        for changed in [
            message.input[..message.input.len() - 1].to_vec(),
            [message.input, b"extra"].concat(),
        ] {
            assert!(verify_candidate(&message, &proof, &changed, &control).is_err());
        }
    }
}
