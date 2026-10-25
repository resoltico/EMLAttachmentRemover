//! Explicit parser frames over one physical cursor; all ranges borrow the input.
use super::scan::{ActiveBoundaries, PhysicalCursor, Registration, field_colon};
use super::{
    Child, Field, Kind, Message, Node, PROTECTED, Span, UNSUPPORTED_BODY_MEDIA, limit, located,
    malformed, previous_terminator, section,
};
use crate::{PreparationError, Refusal, RunControl, grammar};
struct Related {
    target: Option<Vec<u8>>,
    matched: usize,
}
struct Multipart {
    registration: Option<Registration>,
    closed: bool,
    first: Option<usize>,
    close: usize,
    children: Vec<Child>,
    pending: Option<(usize, Span)>,
    related: Option<Related>,
}
enum Body {
    Leaf,
    Message,
    Multipart(Multipart),
}
struct Frame {
    id: usize,
    depth: usize,
    body: Body,
}
struct Parser<'a, 'c> {
    message: Message<'a>,
    cursor: PhysicalCursor<'a>,
    active: ActiveBoundaries,
    frames: Vec<Frame>,
    control: &'c RunControl,
    fields: usize,
    header_bytes: usize,
    parameters: usize,
}
impl Parser<'_, '_> {
    fn matched(&self, span: Span) -> Result<Option<(usize, bool)>, PreparationError> {
        self.active.matches(&self.message.input[span], self.control)
    }
    fn headers(
        &mut self,
        start: usize,
        envelope_allowed: bool,
    ) -> Result<(Vec<Field>, Span, Span), PreparationError> {
        let mut envelope = start..start;
        if envelope_allowed
            && let Some(first) = self.cursor.peek(self.control)?
            && self.message.input[first.content.clone()].starts_with(b"From ")
            && let Some(next) = self.cursor.peek_following(self.control)?
            && field_colon(&self.message.input[next.content], self.control)?.is_some()
        {
            envelope.end = first.end;
            self.cursor.consume();
        }
        let header_start = self.cursor.offset;
        let mut fields: Vec<Field> = Vec::new();
        let mut bytes_used = 0usize;
        while let Some(current) = self.cursor.peek(self.control)? {
            self.control.checkpoint()?;
            if self.matched(current.content.clone())?.is_some() {
                break;
            }
            let bytes = &self.message.input[current.content.clone()];
            let mut end = current.end;
            if let Some(next) = self.cursor.peek_following(self.control)?
                && self.matched(next.content.clone())?.is_some()
            {
                end = previous_terminator(self.message.input, next.content.start, start);
            }
            if bytes.is_empty() {
                self.cursor.consume();
                return Ok((fields, current.content.start.min(end)..end, envelope));
            }
            let continuation = matches!(bytes.first(), Some(b' ' | b'\t'));
            let colon = field_colon(bytes, self.control)?;
            if fields.is_empty() && !continuation && colon.is_none() {
                return Ok((fields, header_start..header_start, envelope));
            }
            let charge = end.checked_sub(current.content.start).ok_or_else(limit)?;
            bytes_used = bytes_used.checked_add(charge).ok_or_else(limit)?;
            self.header_bytes = self.header_bytes.checked_add(charge).ok_or_else(limit)?;
            if bytes_used > 1024 * 1024 || self.header_bytes > 8 * 1024 * 1024 {
                return Err(limit());
            }
            if continuation {
                let previous = fields
                    .last_mut()
                    .ok_or_else(|| malformed("Orphan MIME header continuation"))?;
                previous.value.extend_from_slice(bytes);
                previous.span.end = end;
            } else {
                let colon =
                    colon.ok_or_else(|| malformed("Invalid line after MIME headers began"))?;
                if fields.len() >= 4096 || self.fields >= 65_536 {
                    return Err(limit());
                }
                self.fields += 1;
                fields.push(Field {
                    name: String::from_utf8(bytes[..colon].to_ascii_lowercase())
                        .map_err(|_| malformed("Invalid header name"))?,
                    value: bytes[colon + 1..].to_vec(),
                    span: current.content.start..end,
                });
            }
            self.cursor.consume();
        }
        let end = self.cursor.offset;
        Ok((fields, end..end, envelope))
    }
    fn shallow(
        &mut self,
        depth: usize,
        source_section: Option<String>,
        root: bool,
        envelope_allowed: bool,
        default: &str,
    ) -> Result<usize, PreparationError> {
        let location = source_section.clone();
        let result = (|| {
            self.control.checkpoint()?;
            if depth > 64 || self.message.nodes.len() >= crate::MAX_MESSAGE_PARTS {
                return Err(limit());
            }
            #[cfg(test)]
            crate::work_bounds::record(crate::work_bounds::Work::Node, 1, self.control)?;
            let start = self.cursor.offset;
            let (fields, separator, envelope) = self
                .headers(start, envelope_allowed)
                .map_err(|e| located(e, source_section.clone()))?;
            let unique = |name: &str| -> Result<Option<&Field>, PreparationError> {
                let mut values = fields.iter().filter(|field| field.name == name);
                let first = values.next();
                if values.next().is_some() {
                    return Err(malformed("Duplicate MIME control field"));
                }
                Ok(first)
            };
            let content_type = match unique("content-type")? {
                Some(field) => grammar::structured(&field.value, true, &mut self.parameters)?,
                None => grammar::Structured {
                    primary: default.to_owned(),
                    parameters: Vec::new(),
                },
            };
            let disposition = unique("content-disposition")?
                .map(|field| grammar::structured(&field.value, false, &mut self.parameters))
                .transpose()?;
            self.control.checkpoint()?;
            let filename = grammar::display_name(disposition.as_ref(), &content_type)?;
            self.control.checkpoint()?;
            let id = self.message.nodes.len();
            self.message.nodes.push(Node {
                span: start..self.message.input.len(),
                body: self.cursor.offset..self.message.input.len(),
                separator,
                envelope,
                fields,
                media: content_type.primary.clone(),
                disposition: disposition.map(|field| field.primary),
                filename,
                encoding: None,
                kind: Kind::Leaf,
                root,
                section: source_section,
                content_type,
            });
            Ok(id)
        })();
        result.map_err(|e| located(e, location))
    }
    fn encoding(&mut self, id: usize) -> Result<String, PreparationError> {
        let mut values = self.message.nodes[id]
            .fields
            .iter()
            .filter(|field| field.name == "content-transfer-encoding");
        let first = values.next();
        if values.next().is_some() {
            return Err(malformed("Duplicate structural transfer encoding"));
        }
        let encoding = first
            .map(|field| grammar::transfer(&field.value))
            .transpose()?
            .unwrap_or_else(|| "7bit".into());
        self.message.nodes[id].encoding = Some(encoding.clone());
        Ok(encoding)
    }
    fn push(
        &mut self,
        mut id: usize,
        mut depth: usize,
        mut opaque: bool,
    ) -> Result<(), PreparationError> {
        loop {
            let body = self
                .body(id, opaque)
                .map_err(|e| located(e, self.message.nodes[id].section.clone()))?;
            let embedded = matches!(body, Body::Message);
            self.frames.push(Frame { id, depth, body });
            if !embedded {
                return Ok(());
            }
            let prefix = self.message.nodes[id].section.clone();
            let child = self
                .shallow(depth + 1, prefix.clone(), true, false, "text/plain")
                .map_err(|e| located(e, prefix.clone()))?;
            if !self.message.nodes[child].media.starts_with("multipart/") {
                self.message.nodes[child].section = Some(section(prefix.as_deref(), 1));
            }
            self.message.nodes[id].kind = Kind::Message(child);
            id = child;
            depth += 1;
            opaque = false;
        }
    }
    fn body(&mut self, id: usize, opaque: bool) -> Result<Body, PreparationError> {
        let node = &self.message.nodes[id];
        if opaque
            || (!node.root
                && node
                    .disposition
                    .as_deref()
                    .is_some_and(|token| token != "inline"))
        {
            return Ok(Body::Leaf);
        }
        if PROTECTED.contains(&node.media.as_str()) {
            return Err(PreparationError::Refused {
                kind: Refusal::Protected,
                section: node.section.clone(),
                detail: "Protected MIME reached in retained body context",
            });
        }
        if UNSUPPORTED_BODY_MEDIA.contains(&node.media.as_str()) {
            return Err(PreparationError::Refused {
                kind: Refusal::UnsupportedMime,
                section: node.section.clone(),
                detail: "multipart/report requires parts that the body-only policy would remove.",
            });
        }
        if node.media.starts_with("multipart/") {
            let boundary =
                grammar::parameter_value(&node.content_type.parameters, b"boundary", true)?
                    .ok_or_else(|| malformed("Retained multipart has no boundary"))?
                    .bytes;
            if boundary.is_empty() || boundary.len() > 200 {
                return Err(malformed("Multipart boundary length is invalid"));
            }
            let related = if node.media == "multipart/related" {
                let target =
                    grammar::parameter_value(&node.content_type.parameters, b"start", true)?
                        .map(|p| grammar::identifier(&p.bytes))
                        .transpose()?;
                if target.as_ref().is_some_and(|target| target.len() > 996) {
                    return Err(limit());
                }
                Some(Related { target, matched: 0 })
            } else {
                None
            };
            if !matches!(self.encoding(id)?.as_str(), "7bit" | "8bit" | "binary") {
                return Err(malformed("Retained multipart is transfer encoded"));
            }
            let registration = self.active.register(&boundary, self.frames.len())?;
            Ok(Body::Multipart(Multipart {
                registration: Some(registration),
                closed: false,
                first: None,
                close: 0,
                children: Vec::new(),
                pending: None,
                related,
            }))
        } else if matches!(node.media.as_str(), "message/rfc822" | "message/global")
            && matches!(self.encoding(id)?.as_str(), "7bit" | "8bit" | "binary")
        {
            Ok(Body::Message)
        } else {
            Ok(Body::Leaf)
        }
    }
    fn finish(&mut self, end: usize) -> Result<(), PreparationError> {
        let frame = self
            .frames
            .pop()
            .ok_or_else(|| malformed("Missing parser frame"))?;
        let node = &mut self.message.nodes[frame.id];
        if end < node.span.start {
            return Err(malformed("Entity framing precedes its content"));
        }
        if let Body::Multipart(mut multi) = frame.body {
            if !multi.closed || multi.children.is_empty() {
                return Err(malformed("Multipart lacks children or closing delimiter"));
            }
            if multi
                .related
                .as_ref()
                .is_some_and(|r| r.target.is_some() && r.matched != 1)
            {
                return Err(malformed(
                    "Related start does not select exactly one direct child",
                ));
            }
            self.active
                .unregister(multi.registration.take().expect("registered multipart"));
            node.kind = Kind::Multipart {
                prefix: node.body.start..multi.first.expect("closed multipart has an opening"),
                children: multi.children,
                suffix: multi.close..end,
            };
        }
        node.span.end = end;
        node.body.start = node.body.start.min(end);
        node.body.end = end;
        node.separator.start = node.separator.start.min(end);
        node.separator.end = node.separator.end.min(end);
        node.envelope.end = node.envelope.end.min(end);
        for field in &mut node.fields {
            field.span.end = field.span.end.min(end);
        }
        Ok(())
    }
    fn delimiter(
        &mut self,
        owner: usize,
        closing: bool,
        line: super::scan::PhysicalLine,
    ) -> Result<(), PreparationError> {
        while self.frames.len() > owner + 1 {
            let start = self.message.nodes[self.frames.last().expect("frame").id]
                .span
                .start;
            self.finish(previous_terminator(
                self.message.input,
                line.content.start,
                start,
            ))?;
        }
        let frame = self
            .frames
            .get_mut(owner)
            .ok_or_else(|| malformed("Boundary owner is absent"))?;
        let Body::Multipart(multi) = &mut frame.body else {
            return Err(malformed("Boundary owner is not a multipart"));
        };
        if multi.closed || (closing && multi.first.is_none()) {
            return Err(malformed(
                "Unexpected multipart closing or epilogue delimiter",
            ));
        }
        if let Some((id, opening)) = multi.pending.take() {
            let end = self.message.nodes[id].span.end;
            multi.children.push(Child {
                id,
                deletion: opening.start..line.content.start,
                opening,
                framing: end..line.content.start,
            });
        }
        if multi.first.is_none() {
            multi.first = Some(line.content.start);
        }
        self.cursor.consume();
        if closing {
            multi.closed = true;
            multi.close = line.content.start;
            return Ok(());
        }
        let depth = frame.depth;
        let parent = frame.id;
        let number = multi.children.len() + 1;
        let default = if self.message.nodes[parent].media == "multipart/digest" {
            "message/rfc822"
        } else {
            "text/plain"
        };
        let source_section = Some(section(
            self.message.nodes[parent].section.as_deref(),
            number,
        ));
        let child = self
            .shallow(depth + 1, source_section.clone(), false, false, default)
            .map_err(|e| located(e, source_section))?;
        let Body::Multipart(multi) = &mut self.frames[owner].body else {
            unreachable!()
        };
        multi.pending = Some((child, line.content.start..line.end));
        let opaque = if let Some(related) = &mut multi.related {
            if let Some(target) = &related.target {
                let mut fields = self.message.nodes[child]
                    .fields
                    .iter()
                    .filter(|field| field.name == "content-id");
                let first = fields.next();
                if fields.next().is_some() {
                    return Err(malformed("Duplicate related Content-ID field"));
                }
                let matched = first
                    .map(|field| grammar::identifier(&field.value))
                    .transpose()?
                    .as_ref()
                    == Some(target);
                if matched {
                    related.matched += 1;
                    if related.matched > 1 {
                        return Err(malformed("Related start selects more than one child"));
                    }
                }
                !matched
            } else {
                number != 1
            }
        } else {
            false
        };
        self.push(child, depth + 1, opaque)
    }
}
pub(crate) fn parse<'a>(
    input: &'a [u8],
    control: &RunControl,
) -> Result<Message<'a>, PreparationError> {
    let mut parser = Parser {
        message: Message {
            input,
            nodes: Vec::new(),
        },
        cursor: PhysicalCursor::new(input),
        active: ActiveBoundaries::new(),
        frames: Vec::new(),
        control,
        fields: 0,
        header_bytes: 0,
        parameters: 0,
    };
    let root = parser.shallow(1, None, true, true, "text/plain")?;
    parser.push(root, 1, false)?;
    while let Some(line) = parser.cursor.peek(control)? {
        control.checkpoint()?;
        if let Some((owner, closing)) = parser.matched(line.content.clone())? {
            parser.delimiter(owner, closing, line)?;
        } else {
            parser.cursor.consume();
        }
    }
    while !parser.frames.is_empty() {
        parser.finish(input.len())?;
    }
    Ok(parser.message)
}
