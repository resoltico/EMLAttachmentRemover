//! Menu producer contracts independent of GUI layout and toolkit widget dispatch.
use i_slint_core::{
    SharedString, SharedVector,
    accessibility::{AccessibilityAction, AccessibleStringProperty, SupportedAccessibilityAction},
    item_tree::{
        IndexRange, ItemRc, ItemTree, ItemTreeNode, ItemTreeVTable, ItemTreeWeak,
        ItemVisitorVTable, ItemWeak, TraversalOrder, VisitChildrenResult,
    },
    items::{AccessibleRole, ItemVTable, MenuEntry},
    layout::{LayoutInfo, Orientation},
    lengths::LogicalRect,
    menus::{Menu, MenuFromItemTree, MenuItem},
    properties::Property,
    slice::Slice,
    window::WindowAdapterRc,
};
use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
};
use vtable::{VRc, VRcMapped, VRef, VRefMut};

// Two actual runtime items: a root and a conditionally exposed checkable command.
// The visitor changes membership; identity, tracking and activation remain Core's.
struct MenuTree {
    items: [MenuItem; 2],
    present: Pin<Box<Property<bool>>>,
    weak: RefCell<ItemTreeWeak>,
}
impl ItemTree for MenuTree {
    fn visit_children_item(
        self: Pin<&Self>,
        index: isize,
        _: TraversalOrder,
        mut visitor: VRefMut<ItemVisitorVTable>,
    ) -> VisitChildrenResult {
        if index == 0 && self.present.as_ref().get() {
            let tree = self.weak.borrow().upgrade().unwrap();
            return visitor.visit_item(&tree, 1, self.get_item_ref(1));
        }
        VisitChildrenResult::CONTINUE
    }
    fn get_item_ref(self: Pin<&Self>, index: u32) -> Pin<VRef<'_, ItemVTable>> {
        Pin::new(VRef::new(&self.get_ref().items[index as usize]))
    }
    fn get_item_tree(self: Pin<&Self>) -> Slice<'_, i_slint_core::item_tree::ItemTreeNode> {
        Slice::from_slice(&[])
    }
    fn parent_node(self: Pin<&Self>, result: &mut ItemWeak) {
        *result = ItemWeak::default();
    }
    fn embed_component(self: Pin<&Self>, _: &ItemTreeWeak, _: u32) -> bool {
        false
    }
    fn ensure_instantiated(self: Pin<&Self>) -> bool {
        false
    }
    fn layout_info(self: Pin<&Self>, _: Orientation) -> LayoutInfo {
        LayoutInfo::default()
    }
    fn subtree_index(self: Pin<&Self>) -> usize {
        usize::MAX
    }
    fn get_subtree_range(self: Pin<&Self>, _: u32) -> IndexRange {
        (0..0).into()
    }
    fn get_subtree(self: Pin<&Self>, _: u32, _: usize, result: &mut ItemTreeWeak) {
        *result = ItemTreeWeak::default();
    }
    fn accessible_role(self: Pin<&Self>, _: u32) -> AccessibleRole {
        AccessibleRole::None
    }
    fn accessible_string_property(
        self: Pin<&Self>,
        _: u32,
        _: AccessibleStringProperty,
        _: &mut SharedString,
    ) -> bool {
        false
    }
    fn item_element_infos(self: Pin<&Self>, _: u32, _: &mut SharedString) -> bool {
        false
    }
    fn window_adapter(self: Pin<&Self>, _: bool, result: &mut Option<WindowAdapterRc>) {
        *result = None;
    }
    fn item_geometry(self: Pin<&Self>, _: u32) -> LogicalRect {
        LogicalRect::default()
    }
    fn accessibility_action(self: Pin<&Self>, _: u32, _: &AccessibilityAction) {}
    fn supported_accessibility_actions(self: Pin<&Self>, _: u32) -> SupportedAccessibilityAction {
        SupportedAccessibilityAction::default()
    }
}
i_slint_core::ItemTreeVTable_static!(static MENU_TREE_VTABLE for MenuTree);

struct Fixture {
    tree: VRc<ItemTreeVTable, MenuTree>,
    menu: Rc<MenuFromItemTree>,
    calls: Rc<Cell<u32>>,
}
impl Fixture {
    fn new() -> Self {
        let tree = VRc::new(MenuTree {
            items: [MenuItem::default(), MenuItem::default()],
            present: Box::pin(Property::new(true)),
            weak: RefCell::default(),
        });
        let dynamic = VRc::into_dyn(tree.clone());
        *tree.weak.borrow_mut() = VRc::downgrade(&dynamic);
        let calls = Rc::new(Cell::new(0));
        let fixture = Self {
            tree,
            menu: Rc::new(MenuFromItemTree::new(dynamic)),
            calls,
        };
        let item = fixture.item();
        item.as_pin_ref().title.set("Command".into());
        item.as_pin_ref().enabled.set(true);
        item.as_pin_ref().checkable.set(true);
        let calls = fixture.calls.clone();
        item.as_pin_ref()
            .activated
            .set_handler(move |()| calls.set(calls.get() + 1));
        fixture
    }
    fn item(&self) -> VRcMapped<ItemTreeVTable, MenuItem> {
        ItemRc::new(VRc::into_dyn(self.tree.clone()), 1)
            .downcast::<MenuItem>()
            .unwrap()
    }
    fn entries(&self) -> SharedVector<MenuEntry> {
        let mut entries = SharedVector::default();
        self.menu.sub_menu(None, &mut entries);
        entries
    }
    fn set_present(&self, value: bool) {
        self.tree.present.as_ref().get_ref().set(value);
    }
}

#[test]
fn displayed_entry_keeps_item_identity_across_rebuilds() {
    let fixture = Fixture::new();
    let old = fixture.entries()[0].clone();
    fixture
        .item()
        .as_pin_ref()
        .title
        .set("Updated command".into());
    let current = fixture.entries()[0].clone();
    assert_eq!(old.id, current.id);
    assert_eq!(current.title.as_str(), "Updated command");
    fixture.menu.activate(&old);
    assert_eq!(fixture.calls.get(), 1);
    assert!(fixture.item().as_pin_ref().checked());
}

#[test]
fn stale_enabled_entry_cannot_toggle_or_activate_a_now_disabled_item() {
    let fixture = Fixture::new();
    let old = fixture.entries()[0].clone();
    assert!(old.enabled);
    fixture.item().as_pin_ref().enabled.set(false);
    fixture.menu.activate(&old);
    assert_eq!(fixture.calls.get(), 0);
    assert!(!fixture.item().as_pin_ref().checked());
    let current = fixture.entries()[0].clone();
    assert_eq!(old.id, current.id);
    assert!(!current.enabled);
}

#[test]
fn removed_entry_id_stays_retired_when_the_same_item_returns() {
    let fixture = Fixture::new();
    let old = fixture.entries()[0].clone();
    fixture.set_present(false);
    fixture.menu.activate(&old);
    assert!(fixture.entries().is_empty());
    assert_eq!(fixture.calls.get(), 0);
    fixture.set_present(true);
    let current = fixture.entries()[0].clone();
    assert_ne!(old.id, current.id);
    fixture.menu.activate(&old);
    assert_eq!(fixture.calls.get(), 0);
    fixture.menu.activate(&current);
    assert_eq!(fixture.calls.get(), 1);
}

#[test]
fn activation_callback_can_rebuild_and_query_its_menu() {
    let fixture = Fixture::new();
    let old = fixture.entries()[0].clone();
    let menu = Rc::downgrade(&fixture.menu);
    let calls = fixture.calls.clone();
    fixture
        .item()
        .as_pin_ref()
        .activated
        .set_handler(move |()| {
            let menu = menu.upgrade().unwrap();
            let mut current = SharedVector::default();
            menu.sub_menu(None, &mut current);
            assert_eq!(current.len(), 1);
            assert!(current[0].checked);
            calls.set(calls.get() + 1);
        });
    fixture.menu.activate(&old);
    assert_eq!(fixture.calls.get(), 1);
    assert_eq!(fixture.entries()[0].id, old.id);
}
