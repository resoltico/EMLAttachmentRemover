// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Native text accessibility over the existing Slint text model and renderer.
use i_slint_common::unicode_utils::{
    byte_offset_to_utf16_offset, utf16_offset_to_byte_offset_clamped,
};
use i_slint_core::SharedString;
use i_slint_core::accessibility::{
    AccessibilityAction, SupportedAccessibilityAction, find_exposed_text_input,
};
use i_slint_core::item_tree::{ItemRc, ItemWeak, ParentItemTraversalMode};
use i_slint_core::items::{AccessibleRole, Flickable, TextCursorAffinity, TextInput};
use i_slint_core::lengths::{LogicalLength, LogicalPoint, LogicalRect};
use std::pin::Pin;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub start: i32,
    pub end: i32,
}

pub(super) fn selection(item: &ItemWeak) -> Selection {
    snapshot(item).map_or(Selection { start: -1, end: -1 }, |value| Selection {
        start: value.start,
        end: value.end,
    })
}

pub(super) struct Snapshot {
    pub cursor: i32,
    pub start: i32,
    pub end: i32,
    pub read_only: bool,
    pub multi_line: bool,
}

fn with_input<R>(
    item: &ItemWeak,
    operation: impl FnOnce(&ItemRc, &ItemRc, Pin<&TextInput>) -> R,
) -> Option<R> {
    let wrapper = item.upgrade()?;
    if wrapper.accessible_role() != AccessibleRole::TextInput {
        return None;
    }
    let (inner, input) = find_exposed_text_input(&wrapper)?;
    // The read-only views in this application are not password fields. Never
    // expose an unmasked model if another consumer uses this adapter for one.
    if input.as_pin_ref().is_password() {
        return None;
    }
    Some(operation(&wrapper, &inner, input.as_pin_ref()))
}

fn qt_offset(text: &str, byte: usize) -> i32 {
    i32::try_from(byte_offset_to_utf16_offset(text, byte)).unwrap_or(i32::MAX)
}

pub(super) fn snapshot(item: &ItemWeak) -> Option<Snapshot> {
    with_input(item, |_, _, input| {
        let text = input.text();
        let (start, end) = input.selection_anchor_and_cursor();
        Snapshot {
            cursor: qt_offset(&text, input.cursor_position(&text)),
            start: qt_offset(&text, start),
            end: qt_offset(&text, end),
            read_only: input.read_only(),
            multi_line: !input.single_line(),
        }
    })
}

fn byte_offset(text: &str, offset: i32, cursor: usize) -> Option<usize> {
    let units = text.encode_utf16().count();
    match offset {
        -1 => Some(text.len()),
        -2 => Some(cursor),
        n if n >= 0 && (n as usize) <= units => {
            Some(utf16_offset_to_byte_offset_clamped(text, n as usize))
        }
        _ => None,
    }
}

pub(super) fn range(item: &ItemWeak, start: i32, end: i32) -> Option<SharedString> {
    with_input(item, |_, _, input| {
        let text = input.text();
        text_range(&text, start, end, input.cursor_position(&text)).map(Into::into)
    })
    .flatten()
}

fn text_range(text: &str, start: i32, end: i32, cursor: usize) -> Option<&str> {
    let start = byte_offset(text, start, cursor)?;
    let end = byte_offset(text, end, cursor)?;
    text.get(start..end)
}

fn selection_offsets(text: &str, anchor: i32, focus: i32) -> Option<(i32, i32)> {
    if anchor < 0 || focus < 0 {
        return None;
    }
    let anchor = byte_offset(text, anchor, 0)?;
    let focus = byte_offset(text, focus, 0)?;
    Some((i32::try_from(anchor).ok()?, i32::try_from(focus).ok()?))
}

pub(super) fn set_selection(item: &ItemWeak, anchor: i32, focus: i32) {
    let _ = with_input(item, |wrapper, inner, input| {
        let text = input.text();
        let Some((anchor, focus)) = selection_offsets(&text, anchor, focus) else {
            return;
        };
        if wrapper
            .supported_accessibility_actions()
            .contains(SupportedAccessibilityAction::SetSelectionOffsets)
        {
            wrapper.accessible_action(&AccessibilityAction::SetSelectionOffsets(anchor, focus));
        } else if let Some(adapter) = inner.window_adapter() {
            input.set_selection_offsets(&adapter, inner, anchor, focus);
        }
    });
}

fn local_caret(
    inner: &ItemRc,
    input: Pin<&TextInput>,
    byte: usize,
    affinity: TextCursorAffinity,
) -> Option<LogicalRect> {
    let adapter = inner.window_adapter()?;
    Some(
        adapter
            .renderer()
            .text_input_cursor_rect_for_byte_offset(input, inner, byte, affinity),
    )
}

fn native_caret(
    inner: &ItemRc,
    input: Pin<&TextInput>,
    byte: usize,
    affinity: TextCursorAffinity,
) -> Option<LogicalRect> {
    let mut rect = local_caret(inner, input, byte, affinity)?;
    rect.origin = inner.map_to_native_window(inner.geometry().origin + rect.origin.to_vector());
    Some(rect)
}

pub(super) fn character_rect(item: &ItemWeak, start: i32, end: i32) -> Option<qttypes::QRectF> {
    with_input(item, |_, inner, input| {
        let text = input.text();
        let cursor = input.cursor_position(&text);
        let a = byte_offset(&text, start, cursor)?;
        let b = byte_offset(&text, end, cursor)?;
        let first = native_caret(inner, input, a, TextCursorAffinity::NextCharacter)?;
        let last = native_caret(inner, input, b, TextCursorAffinity::PreviousCharacter)?;
        let (x, width) = if (first.origin.y - last.origin.y).abs() < 0.5 {
            (
                first.origin.x.min(last.origin.x),
                (first.origin.x - last.origin.x).abs().max(first.size.width),
            )
        } else {
            (first.origin.x, first.size.width)
        };
        Some(qttypes::QRectF {
            x: x as _,
            y: first.origin.y as _,
            width: width.max(1.) as _,
            height: first.size.height.max(1.) as _,
        })
    })
    .flatten()
}

pub(super) fn offset_at_point(item: &ItemWeak, x: f32, y: f32) -> i32 {
    with_input(item, |_, inner, input| {
        let point = LogicalPoint::new(x, y);
        let geometry = inner.geometry();
        let origin = inner.map_to_native_window(geometry.origin);
        let mut viewport = LogicalRect::new(origin, geometry.size);
        let mut parent = inner.parent_item(ParentItemTraversalMode::StopAtPopups);
        while let Some(ancestor) = parent {
            parent = ancestor.parent_item(ParentItemTraversalMode::StopAtPopups);
            if ancestor.borrow().as_ref().clips_children() {
                let geometry = ancestor.geometry();
                let clip = LogicalRect::new(
                    ancestor.map_to_native_window(geometry.origin),
                    geometry.size,
                );
                let Some(intersection) = viewport.intersection(&clip) else {
                    return -1;
                };
                viewport = intersection;
            }
        }
        if !viewport.contains(point) {
            return -1;
        }
        let Some(adapter) = inner.window_adapter() else {
            return -1;
        };
        let (byte, _) = adapter.renderer().text_input_byte_offset_for_position(
            input,
            inner,
            point - origin.to_vector(),
        );
        let text = input.text();
        if byte > text.len() || !text.is_char_boundary(byte) {
            return -1;
        }
        qt_offset(&text, byte)
    })
    .unwrap_or(-1)
}

// A range larger than the viewport is aligned to its start; smaller ranges
// move only as far as needed. This does not mutate the text's selection/caret.
fn reveal_axis(current: f32, viewport: f32, content: f32, start: f32, end: f32) -> f32 {
    let shift = if start < 0. || end - start > viewport {
        -start
    } else if end > viewport {
        viewport - end
    } else {
        0.
    };
    (current + shift).clamp((viewport - content).min(0.), 0.)
}

pub(super) fn scroll_to_range(item: &ItemWeak, start: i32, end: i32) {
    if start < 0 || end < start {
        return;
    }
    let _ = with_input(item, |_, inner, input| {
        let text = input.text();
        let cursor = input.cursor_position(&text);
        let (Some(start), Some(end)) = (
            byte_offset(&text, start, cursor),
            byte_offset(&text, end, cursor),
        ) else {
            return;
        };
        if start > end {
            return;
        }
        let mut parent = inner.parent_item(ParentItemTraversalMode::StopAtPopups);
        while let Some(ancestor) = parent {
            parent = ancestor.parent_item(ParentItemTraversalMode::StopAtPopups);
            let Some(flick) = ancestor.clone().downcast::<Flickable>() else {
                continue;
            };
            let flick = flick.as_pin_ref();
            let (Some(a), Some(b)) = (
                native_caret(inner, input, start, TextCursorAffinity::NextCharacter),
                native_caret(inner, input, end, TextCursorAffinity::PreviousCharacter),
            ) else {
                return;
            };
            let geometry = ancestor.geometry();
            let origin = ancestor.map_to_native_window(geometry.origin);
            let x = reveal_axis(
                flick.content_x().get(),
                geometry.size.width,
                flick.content_width().get(),
                a.min_x().min(b.min_x()) - origin.x,
                a.max_x().max(b.max_x()) - origin.x,
            );
            let y = reveal_axis(
                flick.content_y().get(),
                geometry.size.height,
                flick.content_height().get(),
                a.min_y().min(b.min_y()) - origin.y,
                a.max_y().max(b.max_y()) - origin.y,
            );
            flick.content_x.set(LogicalLength::new(x));
            flick.content_y.set(LogicalLength::new(y));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qt_offsets_ranges_and_mid_surrogates_keep_complete_unicode() {
        let text = "Aé😀Z\n日";
        for (qt, byte) in [(0, 0), (1, 1), (2, 3), (4, 7), (5, 8), (6, 9), (7, 12)] {
            assert_eq!(byte_offset(text, qt, 0), Some(byte));
            assert_eq!(qt_offset(text, byte), qt);
        }
        assert_eq!(byte_offset(text, 3, 0), Some(7));
        assert_eq!(text_range(text, 1, 4, 0), Some("é😀"));
        assert_eq!(selection_offsets(text, 4, 1), Some((7, 1)));
        assert_eq!(selection_offsets(text, 3, 3), Some((7, 7)));
        for pair in [(-1, 0), (0, -2), (0, 8)] {
            assert!(selection_offsets(text, pair.0, pair.1).is_none());
        }
        assert_eq!(text_range(text, 0, -1, 0), Some(text));
        assert_eq!(text_range(text, 1, -2, 7), Some("é😀"));
        for (a, b) in [(-3, 0), (0, 8), (4, 1)] {
            assert!(text_range(text, a, b, 0).is_none());
        }
    }
    #[test]
    fn reveal_offsets_clamp_to_content_and_keep_visible_targets() {
        assert_eq!(reveal_axis(-10., 100., 300., 20., 40.), -10.);
        assert_eq!(reveal_axis(-10., 100., 300., -5., 20.), -5.);
        assert_eq!(reveal_axis(-10., 100., 300., 90., 120.), -30.);
        assert_eq!(reveal_axis(-190., 100., 300., 90., 120.), -200.);
        assert_eq!(reveal_axis(0., 100., 50., 90., 120.), 0.);
    }
}
