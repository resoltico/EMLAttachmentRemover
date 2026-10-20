//! Source-bound deletion planning; independent permission is in verify.rs.
use crate::{
    PreparationError, Refusal, RemovalReason, RunControl, error, grammar,
    message::{Child, Kind, Message, PROTECTED, Span},
};
use std::collections::BTreeSet;
pub(crate) const CONTAINER_STALE: &[&str] = &["content-length", "lines", "content-md5"];
pub(crate) const ROOT_STALE: &[&str] = &[
    "dkim-signature",
    "domainkey-signature",
    "arc-seal",
    "arc-message-signature",
    "arc-authentication-results",
    "x-ms-has-attach",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Claim {
    pub id: usize,
    pub reason: RemovalReason,
}
pub(crate) struct Plan {
    pub claims: Vec<Claim>,
    pub edits: Vec<Span>,
    pub stripped: BTreeSet<String>,
}
fn internal() -> PreparationError {
    error(
        Refusal::Internal,
        "Deletion plan is not a valid source-bound edit",
    )
}
fn related_root(
    message: &Message<'_>,
    id: usize,
    children: &[Child],
) -> Result<usize, PreparationError> {
    let Some(grammar::DecodedParameter { bytes: start, .. }) =
        grammar::parameter_value(&message.nodes[id].content_type.parameters, b"start", true)?
    else {
        return children.first().map(|child| child.id).ok_or_else(internal);
    };
    let target = grammar::identifier(&start)?;
    let mut selected = None;
    for child in children {
        let mut ids = message.nodes[child.id]
            .fields
            .iter()
            .filter(|field| field.name == "content-id");
        let first = ids.next();
        if ids.next().is_some() {
            return Err(internal());
        }
        if first
            .map(|field| grammar::identifier(&field.value))
            .transpose()?
            .as_ref()
            == Some(&target)
            && selected.replace(child.id).is_some()
        {
            return Err(internal());
        }
    }
    selected.ok_or_else(internal)
}
fn visit(
    message: &Message<'_>,
    id: usize,
    plan: &mut Plan,
    control: &RunControl,
) -> Result<(bool, bool), PreparationError> {
    control.checkpoint()?;
    let node = &message.nodes[id];
    if !node.root && node.disposition.as_deref() == Some("attachment") {
        plan.claims.push(Claim {
            id,
            reason: RemovalReason::Attachment,
        });
        return Ok((false, true));
    }
    if PROTECTED.contains(&node.media.as_str()) {
        return Err(error(
            Refusal::Protected,
            "Protected MIME reached by planner",
        ));
    }
    if matches!(node.media.as_str(), "text/plain" | "text/html") {
        return Ok((true, false));
    }
    let (keep, changed) = match &node.kind {
        Kind::Multipart { children, .. } if node.media.starts_with("multipart/") => {
            let selected = if node.media == "multipart/related" {
                Some(related_root(message, id, children)?)
            } else {
                None
            };
            let mut keep = false;
            let mut changed = false;
            for child in children {
                let before = plan.edits.len();
                let (child_keep, child_changed) =
                    if selected.is_some_and(|selected| selected != child.id) {
                        plan.claims.push(Claim {
                            id: child.id,
                            reason: RemovalReason::RelatedResource,
                        });
                        (false, true)
                    } else {
                        visit(message, child.id, plan, control)?
                    };
                if !child_keep {
                    plan.edits.truncate(before);
                    plan.edits.push(child.deletion.clone());
                }
                keep |= child_keep;
                changed |= child_changed;
            }
            (keep, changed)
        }
        Kind::Message(child)
            if matches!(node.media.as_str(), "message/rfc822" | "message/global") =>
        {
            visit(message, *child, plan, control)?
        }
        _ => {
            if id != 0 {
                plan.claims.push(Claim {
                    id,
                    reason: RemovalReason::NonBodyPart,
                });
            }
            (false, true)
        }
    };
    if keep && changed {
        for field in &node.fields {
            if CONTAINER_STALE.contains(&field.name.as_str())
                || (node.root && ROOT_STALE.contains(&field.name.as_str()))
            {
                plan.edits.push(field.span.clone());
                plan.stripped.insert(field.name.clone());
            }
        }
    }
    Ok((keep, changed))
}
pub(crate) fn build(message: &Message<'_>, control: &RunControl) -> Result<Plan, PreparationError> {
    let mut plan = Plan {
        claims: Vec::new(),
        edits: Vec::new(),
        stripped: BTreeSet::new(),
    };
    if !visit(message, 0, &mut plan, control)?.0 {
        return Err(error(
            Refusal::NoBody,
            "No supported text body entity remains",
        ));
    }
    plan.edits.sort_by_key(|span| span.start);
    check_edit_ranges(message.input.len(), &plan.edits)?;
    Ok(plan)
}
/// Guard actual edit arithmetic; permission and ownership are independently reconstructed.
pub(crate) fn check_edit_ranges(input_len: usize, edits: &[Span]) -> Result<(), PreparationError> {
    let mut previous_end = 0;
    for span in edits {
        if span.start < previous_end || span.start >= span.end || span.end > input_len {
            return Err(internal());
        }
        previous_end = span.end;
    }
    Ok(())
}
pub(crate) fn apply(
    input: &[u8],
    edits: &[Span],
    control: &RunControl,
) -> Result<Vec<u8>, PreparationError> {
    check_edit_ranges(input.len(), edits)?;
    let removed = edits.iter().try_fold(0usize, |sum, span| {
        sum.checked_add(span.len()).ok_or_else(internal)
    })?;
    let mut output = Vec::with_capacity(input.len().checked_sub(removed).ok_or_else(internal)?);
    let mut cursor = 0;
    for span in edits
        .iter()
        .cloned()
        .chain(std::iter::once(input.len()..input.len()))
    {
        for chunk in input[cursor..span.start].chunks(1024 * 1024) {
            control.checkpoint()?;
            output.extend_from_slice(chunk);
        }
        cursor = span.end;
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_guard_rejects_empty_reverse_overlap_and_out_of_bounds() {
        for spans in [
            vec![(2, 2)],
            vec![(4, 3)],
            vec![(8, 11)],
            vec![(1, 5), (4, 6)],
            vec![(5, 6), (1, 2)],
        ] {
            let spans: Vec<_> = spans.into_iter().map(|(start, end)| start..end).collect();
            assert!(check_edit_ranges(10, &spans).is_err(), "{spans:?}");
        }
        assert!(check_edit_ranges(10, &[0..1, 1..2, 9..10]).is_ok());
        assert!(check_edit_ranges(0, &[]).is_ok());
    }
}
