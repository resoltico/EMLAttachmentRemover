//! One cached physical cursor and stack-scoped active-boundary trie.
use super::{Span, malformed};
use crate::{PREPARATION_CHUNK_BYTES, PreparationError, RunControl};
use std::collections::BTreeMap;
#[derive(Clone)]
pub(super) struct PhysicalLine {
    pub(super) content: Span,
    pub(super) end: usize,
}
pub(super) struct PhysicalCursor<'a> {
    pub(super) input: &'a [u8],
    pub(super) offset: usize,
    current: Option<PhysicalLine>,
    following: Option<PhysicalLine>,
}
impl<'a> PhysicalCursor<'a> {
    pub(super) fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            offset: 0,
            current: None,
            following: None,
        }
    }
    fn line(
        &self,
        start: usize,
        control: &RunControl,
    ) -> Result<Option<PhysicalLine>, PreparationError> {
        if start == self.input.len() {
            return Ok(None);
        }
        let mut end = start;
        while end < self.input.len() {
            control.checkpoint()?;
            let limit = (end + PREPARATION_CHUNK_BYTES).min(self.input.len());
            if let Some(offset) = memchr::memchr2(b'\r', b'\n', &self.input[end..limit]) {
                #[cfg(test)]
                crate::work_bounds::record(
                    crate::work_bounds::Work::Physical,
                    offset + 1,
                    control,
                )?;
                end += offset;
                break;
            }
            #[cfg(test)]
            crate::work_bounds::record(crate::work_bounds::Work::Physical, limit - end, control)?;
            end = limit;
        }
        let next = if end == self.input.len() {
            end
        } else if self.input[end] == b'\r' && self.input.get(end + 1) == Some(&b'\n') {
            end + 2
        } else {
            end + 1
        };
        Ok(Some(PhysicalLine {
            content: start..end,
            end: next,
        }))
    }
    pub(super) fn peek(
        &mut self,
        control: &RunControl,
    ) -> Result<Option<PhysicalLine>, PreparationError> {
        if self.current.is_none() {
            self.current = self.line(self.offset, control)?;
        }
        Ok(self.current.clone())
    }
    pub(super) fn peek_following(
        &mut self,
        control: &RunControl,
    ) -> Result<Option<PhysicalLine>, PreparationError> {
        let Some(current) = self.peek(control)? else {
            return Ok(None);
        };
        if self.following.is_none() {
            self.following = self.line(current.end, control)?;
        }
        Ok(self.following.clone())
    }
    pub(super) fn consume(&mut self) {
        if let Some(current) = self.current.take() {
            self.offset = current.end;
            self.current = self.following.take();
        }
    }
}
pub(super) fn field_colon(
    bytes: &[u8],
    control: &RunControl,
) -> Result<Option<usize>, PreparationError> {
    let mut offset = 0;
    for chunk in bytes.chunks(PREPARATION_CHUNK_BYTES) {
        control.checkpoint()?;
        for (visited, byte) in chunk.iter().enumerate() {
            if *byte == b':' {
                #[cfg(test)]
                crate::work_bounds::record(
                    crate::work_bounds::Work::Classification,
                    visited + 1,
                    control,
                )?;
                return Ok((offset > 0).then_some(offset));
            }
            if !matches!(byte, 0x21..=0x39 | 0x3b..=0x7e) {
                #[cfg(test)]
                crate::work_bounds::record(
                    crate::work_bounds::Work::Classification,
                    visited + 1,
                    control,
                )?;
                return Ok(None);
            }
            let _ = visited;
            offset += 1;
        }
        #[cfg(test)]
        crate::work_bounds::record(
            crate::work_bounds::Work::Classification,
            chunk.len(),
            control,
        )?;
    }
    Ok(None)
}
#[derive(Default)]
struct TrieNode {
    edges: BTreeMap<u8, usize>,
    owner: Option<usize>,
}
pub(super) struct Registration {
    mark: usize,
    link: Option<(usize, u8)>,
    terminal: usize,
}
pub(super) struct ActiveBoundaries {
    nodes: Vec<TrieNode>,
}
impl ActiveBoundaries {
    pub(super) fn new() -> Self {
        Self {
            nodes: vec![TrieNode::default()],
        }
    }
    pub(super) fn register(
        &mut self,
        boundary: &[u8],
        owner: usize,
    ) -> Result<Registration, PreparationError> {
        let mark = self.nodes.len();
        let mut link = None;
        let mut node = 0;
        for byte in boundary {
            node = match self.nodes[node].edges.get(byte).copied() {
                Some(next) => next,
                None => {
                    let next = self.nodes.len();
                    self.nodes.push(TrieNode::default());
                    self.nodes[node].edges.insert(*byte, next);
                    if link.is_none() {
                        link = Some((node, *byte));
                    }
                    next
                }
            };
        }
        if self.nodes[node].owner.is_some() {
            return Err(malformed("Duplicate active multipart boundary"));
        }
        self.nodes[node].owner = Some(owner);
        Ok(Registration {
            mark,
            link,
            terminal: node,
        })
    }
    pub(super) fn unregister(&mut self, registration: Registration) {
        self.nodes[registration.terminal].owner = None;
        if let Some((parent, byte)) = registration.link {
            self.nodes[parent].edges.remove(&byte);
        }
        self.nodes.truncate(registration.mark);
    }
    pub(super) fn matches(
        &self,
        bytes: &[u8],
        control: &RunControl,
    ) -> Result<Option<(usize, bool)>, PreparationError> {
        let Some(bytes) = bytes.strip_prefix(b"--") else {
            return Ok(None);
        };
        let mut padding = bytes.len();
        'padding: for chunk in bytes.rchunks(PREPARATION_CHUNK_BYTES) {
            control.checkpoint()?;
            for (visited, byte) in chunk.iter().rev().enumerate() {
                if !matches!(byte, b' ' | b'\t') {
                    #[cfg(test)]
                    crate::work_bounds::record(
                        crate::work_bounds::Work::Padding,
                        visited + 1,
                        control,
                    )?;
                    break 'padding;
                }
                let _ = visited;
                padding -= 1;
            }
            #[cfg(test)]
            crate::work_bounds::record(crate::work_bounds::Work::Padding, chunk.len(), control)?;
        }
        let mut node = 0;
        let mut found: Option<(usize, bool)> = None;
        for (offset, byte) in bytes.iter().take(200).enumerate() {
            let Some(next) = self.nodes[node].edges.get(byte) else {
                break;
            };
            node = *next;
            if let Some(owner) = self.nodes[node].owner {
                let end = offset + 1;
                let closing = bytes.get(end..end + 2) == Some(b"--") && end + 2 >= padding;
                if (end >= padding || closing) && found.is_none_or(|(previous, _)| owner < previous)
                {
                    found = Some((owner, closing));
                }
            }
        }
        Ok(found)
    }
}
