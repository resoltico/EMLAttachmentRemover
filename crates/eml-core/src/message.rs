//! Immutable physical MIME facts. Opaque removal interiors are never parsed.
use crate::{PreparationError, Refusal, RunControl, error, grammar};
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
struct Parser<'a, 'control> {
    message: Message<'a>,
    control: &'control RunControl,
    fields: usize,
    header_bytes: usize,
    parameters: usize,
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
#[derive(Clone)]
struct Line {
    content: Span,
    end: usize,
}
fn line(input: &[u8], start: usize, end: usize) -> Line {
    let content_end =
        memchr::memchr2(b'\r', b'\n', &input[start..end]).map_or(end, |offset| start + offset);
    let next = if content_end == end {
        end
    } else if input[content_end] == b'\r'
        && input.get(content_end + 1) == Some(&b'\n')
        && content_end + 1 < end
    {
        content_end + 2
    } else {
        content_end + 1
    };
    Line {
        content: start..content_end,
        end: next,
    }
}
fn field_colon(bytes: &[u8]) -> Option<usize> {
    let colon = memchr::memchr(b':', bytes)?;
    (colon > 0
        && bytes[..colon]
            .iter()
            .all(|byte| matches!(byte, 0x21..=0x39 | 0x3b..=0x7e)))
    .then_some(colon)
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
impl Parser<'_, '_> {
    fn headers(
        &mut self,
        span: Span,
        envelope_allowed: bool,
    ) -> Result<(Vec<Field>, Span, Span, Span), PreparationError> {
        let input = self.message.input;
        let mut cursor = span.start;
        let mut envelope = cursor..cursor;
        if envelope_allowed && cursor < span.end {
            let first = line(input, cursor, span.end);
            if input[first.content.clone()].starts_with(b"From ") && first.end < span.end {
                let next = line(input, first.end, span.end);
                if field_colon(&input[next.content]).is_some() {
                    envelope = cursor..first.end;
                    cursor = first.end;
                }
            }
        }
        let header_start = cursor;
        let mut fields: Vec<Field> = Vec::new();
        let mut header_bytes = 0usize;
        while cursor < span.end {
            self.control.checkpoint()?;
            let current = line(input, cursor, span.end);
            let bytes = &input[current.content.clone()];
            if bytes.is_empty() {
                return Ok((fields, cursor..current.end, current.end..span.end, envelope));
            }
            let continuation = matches!(bytes.first(), Some(b' ' | b'\t'));
            let colon = field_colon(bytes);
            if fields.is_empty() && !continuation && colon.is_none() {
                return Ok((
                    fields,
                    header_start..header_start,
                    header_start..span.end,
                    envelope,
                ));
            }
            let addition = current.end - cursor;
            header_bytes = header_bytes.checked_add(addition).ok_or_else(limit)?;
            self.header_bytes = self.header_bytes.checked_add(addition).ok_or_else(limit)?;
            if header_bytes > 1024 * 1024 || self.header_bytes > 8 * 1024 * 1024 {
                return Err(limit());
            }
            if continuation {
                let previous = fields
                    .last_mut()
                    .ok_or_else(|| malformed("Orphan MIME header continuation"))?;
                previous.value.extend_from_slice(bytes);
                previous.span.end = current.end;
            } else {
                let colon =
                    colon.ok_or_else(|| malformed("Invalid line after MIME headers began"))?;
                self.fields += 1;
                if fields.len() >= 4096 || self.fields > 65_536 {
                    return Err(limit());
                }
                let name = String::from_utf8(bytes[..colon].to_ascii_lowercase())
                    .map_err(|_| malformed("Invalid header name"))?;
                fields.push(Field {
                    name,
                    value: bytes[colon + 1..].to_vec(),
                    span: cursor..current.end,
                });
            }
            if current.end <= cursor {
                return Err(error(Refusal::Internal, "Header scanner did not advance"));
            }
            cursor = current.end;
        }
        Ok((fields, span.end..span.end, span.end..span.end, envelope))
    }
    fn shallow(
        &mut self,
        span: Span,
        depth: usize,
        source_section: Option<String>,
        root: bool,
        envelope: bool,
        default: &str,
    ) -> Result<usize, PreparationError> {
        self.control.checkpoint()?;
        if depth > 64 || self.message.nodes.len() >= 10_000 {
            return Err(limit());
        }
        let (fields, separator, body, envelope) = self.headers(span.clone(), envelope)?;
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
        let filename = grammar::display_name(disposition.as_ref(), &content_type)?;
        let id = self.message.nodes.len();
        self.message.nodes.push(Node {
            span,
            body,
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
        let value = match first {
            Some(field) => grammar::transfer(&field.value)?,
            None => "7bit".to_owned(),
        };
        self.message.nodes[id].encoding = Some(value.clone());
        Ok(value)
    }
    fn multipart(&mut self, id: usize, depth: usize) -> Result<Vec<Child>, PreparationError> {
        let node = &self.message.nodes[id];
        let boundary = grammar::parameter_value(&node.content_type.parameters, b"boundary", true)?
            .ok_or_else(|| malformed("Retained multipart has no boundary"))?
            .bytes;
        if boundary.is_empty() || boundary.len() > 200 {
            return Err(malformed("Multipart boundary length is invalid"));
        }
        let body = node.body.clone();
        let prefix = node.section.clone();
        let default = if node.media == "multipart/digest" {
            "message/rfc822"
        } else {
            "text/plain"
        };
        let input = self.message.input;
        let mut cursor = body.start;
        let mut delimiters = Vec::new();
        let mut closed = false;
        while cursor < body.end {
            self.control.checkpoint()?;
            let current = line(input, cursor, body.end);
            let bytes = &input[current.content.clone()];
            if let Some(tail) = bytes
                .strip_prefix(b"--")
                .and_then(|rest| rest.strip_prefix(boundary.as_slice()))
            {
                let (closing, tail) = tail
                    .strip_prefix(b"--")
                    .map_or((false, tail), |tail| (true, tail));
                if tail.iter().all(|b| matches!(b, b' ' | b'\t')) {
                    if closed || (closing && delimiters.is_empty()) {
                        return Err(malformed(
                            "Unexpected multipart closing or epilogue delimiter",
                        ));
                    }
                    closed = closing;
                    if delimiters.len() >= 10_001 {
                        return Err(limit());
                    }
                    delimiters.push((cursor, current.end, closing));
                }
            }
            if current.end <= cursor {
                return Err(error(
                    Refusal::Internal,
                    "Delimiter scanner did not advance",
                ));
            }
            cursor = current.end;
        }
        if !closed || delimiters.len() < 2 {
            return Err(malformed("Multipart lacks children or closing delimiter"));
        }
        let mut children = Vec::new();
        for pair in delimiters.windows(2) {
            let (start, entity_start, closing) = pair[0];
            if closing {
                return Err(malformed("Unexpected delimiter after closure"));
            }
            let next = pair[1].0;
            let entity_end = previous_terminator(input, next, entity_start);
            let child = self.shallow(
                entity_start..entity_end,
                depth + 1,
                Some(section(prefix.as_deref(), children.len() + 1)),
                false,
                false,
                default,
            )?;
            children.push(Child {
                id: child,
                opening: start..entity_start,
                framing: entity_end..next,
                deletion: start..next,
            });
        }
        let first = delimiters[0].0;
        let last = delimiters
            .last()
            .ok_or_else(|| malformed("Missing multipart closure"))?
            .0;
        self.message.nodes[id].kind = Kind::Multipart {
            prefix: body.start..first,
            children: children.clone(),
            suffix: last..body.end,
        };
        Ok(children)
    }
    fn expand(&mut self, id: usize, depth: usize) -> Result<(), PreparationError> {
        self.control.checkpoint()?;
        let node = &self.message.nodes[id];
        if !node.root && node.disposition.as_deref() == Some("attachment") {
            return Ok(());
        }
        if PROTECTED.contains(&node.media.as_str()) {
            return Err(PreparationError::Refused {
                kind: Refusal::Protected,
                section: node.section.clone(),
                detail: "Protected MIME reached in retained body context",
            });
        }
        let media = node.media.clone();
        if media.starts_with("multipart/") {
            let encoding = self.encoding(id)?;
            if !matches!(encoding.as_str(), "7bit" | "8bit" | "binary") {
                return Err(malformed("Retained multipart is transfer encoded"));
            }
            let children = self.multipart(id, depth)?;
            if media == "multipart/related" {
                let start = grammar::parameter_value(
                    &self.message.nodes[id].content_type.parameters,
                    b"start",
                    true,
                )?;
                let selected = if let Some(grammar::DecodedParameter { bytes: start, .. }) = start {
                    let required = grammar::identifier(&start)?;
                    if required.len() > 996 {
                        return Err(limit());
                    }
                    let mut matched = None;
                    for child in &children {
                        let mut ids = self.message.nodes[child.id]
                            .fields
                            .iter()
                            .filter(|field| field.name == "content-id");
                        let first = ids.next();
                        if ids.next().is_some() {
                            return Err(malformed("Duplicate related Content-ID field"));
                        }
                        if let Some(first) = first
                            && grammar::identifier(&first.value)? == required
                            && matched.replace(child.id).is_some()
                        {
                            return Err(malformed("Related start selects more than one child"));
                        }
                    }
                    matched
                        .ok_or_else(|| malformed("Related start does not select a direct child"))?
                } else {
                    children
                        .first()
                        .ok_or_else(|| malformed("Related has no first child"))?
                        .id
                };
                self.expand(selected, depth + 1)?;
            } else {
                for child in children {
                    self.expand(child.id, depth + 1)?;
                }
            }
        } else if matches!(media.as_str(), "message/rfc822" | "message/global") {
            let encoding = self.encoding(id)?;
            if matches!(encoding.as_str(), "7bit" | "8bit" | "binary") {
                let node = &self.message.nodes[id];
                let body = node.body.clone();
                let prefix = node.section.clone();
                let child =
                    self.shallow(body, depth + 1, prefix.clone(), true, false, "text/plain")?;
                if !self.message.nodes[child].media.starts_with("multipart/") {
                    self.message.nodes[child].section = Some(section(prefix.as_deref(), 1));
                }
                self.message.nodes[id].kind = Kind::Message(child);
                self.expand(child, depth + 1)?;
            }
        }
        Ok(())
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
        control,
        fields: 0,
        header_bytes: 0,
        parameters: 0,
    };
    let root = parser.shallow(0..input.len(), 1, None, true, true, "text/plain")?;
    parser.expand(root, 1)?;
    Ok(parser.message)
}
