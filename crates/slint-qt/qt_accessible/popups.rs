// Copyright 2026 Ervins Strauhmanis
// SPDX-License-Identifier: MPL-2.0

//! Temporary accessibility children and native geometry for embedded Slint popups.
use i_slint_core::{
    SharedVector,
    accessibility::{accessible_descendents, nearest_accessible_item},
    item_tree::{ItemRc, ItemWeak, ParentItemTraversalMode},
    lengths::LogicalRect,
    window::{PopupWindowLocation, WindowInner},
};

/// Include embedded popup roots under their accessible invocation parent, as the
/// Winit bridge does. The caller must keep this strong snapshot temporary; Qt
/// accessibility objects retain only ItemWeak references after the query.
pub(super) fn children(root: &ItemRc) -> SharedVector<ItemRc> {
    let mut children: Vec<_> = accessible_descendents(root).collect();
    let Some(adapter) = root.window_adapter() else {
        return children.into_iter().collect();
    };
    let window = WindowInner::from_pub(adapter.window());
    let Some(component) = window.try_component() else {
        return children.into_iter().collect();
    };
    let window_root = ItemRc::new_root(component);
    let popups = snapshot(window);
    let mut ancestors = Vec::new();
    let mut ancestor = Some(root.clone());
    while let Some(item) = ancestor {
        ancestor = item.parent_item(ParentItemTraversalMode::StopAtPopups);
        ancestors.push(item);
    }
    for (popup, parent) in popups {
        let Some(parent) = parent.upgrade() else {
            continue;
        };
        let parent = nearest_accessible_item(parent);
        let parent = if parent.is_accessible() {
            parent
        } else {
            window_root.clone()
        };
        if parent == *root && !ancestors.contains(&popup) && !children.contains(&popup) {
            children.push(popup);
        }
    }
    children.into_iter().collect()
}

/// Map into the owning native window, including embedded popup placement. Qt
/// adds the native widget's global origin after this callback returns.
pub(super) fn native_rect(item: &ItemRc) -> qttypes::QRectF {
    qrect(native_geometry(item))
}

/// Return embedded popup roots in Core paint/stack order. The caller reverses
/// this temporary snapshot for topmost-first point queries.
pub(super) fn popup_roots(root: &ItemRc) -> SharedVector<ItemRc> {
    let Some(adapter) = root.window_adapter() else {
        return SharedVector::default();
    };
    let window = WindowInner::from_pub(adapter.window());
    let mut roots = Vec::new();
    for (popup, _) in snapshot(window) {
        if !roots.contains(&popup) {
            roots.push(popup);
        }
    }
    roots.into_iter().collect()
}

/// Clip point-hit bounds to actual clipping ancestors within this popup/window.
/// Popup overflow must not be clipped to its invocation button's geometry.
/// Like native_rect, size is axis-aligned for the application's untransformed UI.
pub(super) fn hit_rect(item: &ItemRc) -> qttypes::QRectF {
    if !item.is_visible() {
        return qttypes::QRectF::default();
    }
    let mut rect = native_geometry(item);
    let mut parent = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    while let Some(ancestor) = parent {
        if ancestor.borrow().as_ref().clips_children() {
            rect = rect
                .intersection(&native_geometry(&ancestor))
                .unwrap_or_default();
        }
        parent = ancestor.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    qrect(rect)
}

fn snapshot(window: &WindowInner) -> Vec<(ItemRc, ItemWeak)> {
    // Release the active-popup borrow before querying item properties or walking
    // ancestors. Lazy tree updates must not retain a RefCell borrow into popups.
    window
        .active_popups()
        .iter()
        .filter(|popup| matches!(popup.location, PopupWindowLocation::ChildWindow(_)))
        .map(|popup| {
            (
                ItemRc::new_root(popup.component.clone()),
                popup.parent_item.clone(),
            )
        })
        .collect()
}

fn native_geometry(item: &ItemRc) -> LogicalRect {
    let geometry = item.geometry();
    LogicalRect::new(item.map_to_native_window(geometry.origin), geometry.size)
}

fn qrect(rect: LogicalRect) -> qttypes::QRectF {
    qttypes::QRectF {
        x: rect.origin.x as _,
        y: rect.origin.y as _,
        width: rect.width() as _,
        height: rect.height() as _,
    }
}
