// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cspell:ignore descendents qobject qwidget

use crate::accessible_generated::*;
use crate::qt_window::QtWindow;

mod text;
mod popups;

use i_slint_core::SharedVector;
use i_slint_core::accessibility::{
    AccessibilityAction, AccessibleStringProperty, SupportedAccessibilityAction,
};
use i_slint_core::item_tree::{ItemRc, ItemWeak};
use i_slint_core::properties::{PropertyDirtyHandler, PropertyTracker};
use i_slint_core::window::WindowInner;

use cpp::*;
use pin_project::pin_project;
use qttypes::QString;

use core::ffi::c_void;
use std::pin::Pin;
use std::mem::ManuallyDrop;
use std::rc::{Rc, Weak};

/// Borrow an owned Weak::into_raw pointer without consuming its weak reference.
/// The caller keeps that original raw weak ownership alive for this call.
unsafe fn upgrade_borrowed_weak<T>(pointer: *const T) -> Option<Rc<T>> {
    if pointer.is_null() { return None; }
    let weak = ManuallyDrop::new(unsafe { Weak::from_raw(pointer) });
    weak.upgrade()
}

// KEEP IN SYNC WITH CONSTANTS IN C++
const NAME: u32 = QAccessible_Text_Name;
const DESCRIPTION: u32 = QAccessible_Text_Description;
const VALUE: u32 = QAccessible_Text_Value;
const CHECKED: u32 = QAccessible_Text_UserText;
const VALUE_MINIMUM: u32 = CHECKED + 1;
const VALUE_MAXIMUM: u32 = VALUE_MINIMUM + 1;
const VALUE_STEP: u32 = VALUE_MAXIMUM + 1;
const CHECKABLE: u32 = VALUE_STEP + 1;
const EXPANDABLE: u32 = CHECKABLE + 1;
const EXPANDED: u32 = EXPANDABLE + 1;
const READ_ONLY: u32 = EXPANDED + 1;
const ENABLED: u32 = READ_ONLY + 1;
const ITEM_SELECTABLE: u32 = ENABLED + 1;
const ITEM_SELECTED: u32 = ITEM_SELECTABLE + 1;
const VISIBLE: u32 = ITEM_SELECTED + 1;
const LIVE_REGION: u32 = VISIBLE + 1;

pub struct AccessibleItemPropertiesTracker {
    obj: *mut c_void,
}

impl PropertyDirtyHandler for AccessibleItemPropertiesTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [obj = QPointer(obj)]() {
                if (!obj)
                    return;

                auto accessible_item = live_accessible_item(obj);
                if (!accessible_item) return;
                auto data = accessible_item->data();
                rust!(AccessibleItemPropertiesTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_state_tracker();
                });

                accessible_item->sendStateChangeEvent();
            });
        });
    }
}

pub struct ValuePropertyTracker {
    obj: *mut c_void,
}

impl PropertyDirtyHandler for ValuePropertyTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [ obj = QPointer(obj)]() {
                if (!obj)
                    return;

                auto accessible_item = live_accessible_item(obj);
                if (!accessible_item) return;
                auto data = accessible_item->data();
                rust!(ValuePropertyTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_value_tracker();
                });

                if (accessible_item->hasTextInterface()) return;
                auto event = QAccessibleValueChangeEvent(obj, accessible_item->currentValue());
                QAccessible::updateAccessibility(&event);
            });
        });
    }
}

pub struct TextPropertyTracker {
    obj: *mut c_void,
}
impl PropertyDirtyHandler for TextPropertyTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [obj = QPointer(obj)]() {
                if (!obj) return;
                auto item = live_accessible_item(obj);
                if (!item) return;
                auto data = item->data();
                rust!(TextPropertyTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_text_tracker();
                });
                item->sendTextChangeEvents();
                item->sendStateChangeEvent();
            });
        });
    }
}

pub struct LabelPropertyTracker {
    obj: *mut c_void,
}

impl PropertyDirtyHandler for LabelPropertyTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [obj = QPointer(obj)]() {
                if (!obj)
                    return;

                auto accessible_item = live_accessible_item(obj);
                if (!accessible_item) return;
                auto data = accessible_item->data();
                rust!(LabelPropertyTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_label_tracker();
                });

                accessible_item->sendNameChangeEvent();
            });
        });
    }
}

pub struct DescriptionPropertyTracker {
    obj: *mut c_void,
}

impl PropertyDirtyHandler for DescriptionPropertyTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [obj = QPointer(obj)]() {
                if (!obj)
                    return;

                auto accessible_item = live_accessible_item(obj);
                if (!accessible_item) return;
                auto data = accessible_item->data();
                rust!(DescriptionPropertyTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_description_tracker();
                });

                auto event = QAccessibleEvent(obj, QAccessible::DescriptionChanged);
                QAccessible::updateAccessibility(&event);
            });
        });
    }
}

pub struct FocusDelegationPropertyTracker {
    obj: *mut c_void,
}

impl PropertyDirtyHandler for FocusDelegationPropertyTracker {
    fn notify(self: Pin<&Self>) {
        let obj = self.obj;
        cpp!(unsafe [obj as "QObject*"] {
            QTimer::singleShot(0, [obj = QPointer(obj)]() {
                if (!obj)
                    return;

                auto accessible_item = live_accessible_item(obj);
                if (!accessible_item) return;
                auto data = accessible_item->data();
                rust!(FocusDelegationPropertyTracker_rearm [data: Pin<&SlintAccessibleItemData> as "void*"] {
                    data.arm_focus_delegation_tracker();
                });

                accessible_item->delegateFocus();
            });
        });
    }
}

#[pin_project]
pub struct SlintAccessibleItemData {
    #[pin]
    state_tracker: PropertyTracker<false, AccessibleItemPropertiesTracker>,
    #[pin]
    value_tracker: PropertyTracker<false, ValuePropertyTracker>,
    #[pin]
    text_tracker: PropertyTracker<false, TextPropertyTracker>,
    #[pin]
    label_tracker: PropertyTracker<false, LabelPropertyTracker>,
    #[pin]
    description_tracker: PropertyTracker<false, DescriptionPropertyTracker>,
    #[pin]
    focus_delegation_tracker: PropertyTracker<false, FocusDelegationPropertyTracker>,
    item: ItemWeak,
}

impl SlintAccessibleItemData {
    fn new_pin_box(obj: *mut c_void, item: &ItemWeak) -> Pin<Box<Self>> {
        let state_tracker =
            PropertyTracker::new_with_dirty_handler(AccessibleItemPropertiesTracker { obj });
        let value_tracker = PropertyTracker::new_with_dirty_handler(ValuePropertyTracker { obj });
        let text_tracker = PropertyTracker::new_with_dirty_handler(TextPropertyTracker { obj });
        let label_tracker = PropertyTracker::new_with_dirty_handler(LabelPropertyTracker { obj });
        let description_tracker =
            PropertyTracker::new_with_dirty_handler(DescriptionPropertyTracker { obj });
        let focus_delegation_tracker =
            PropertyTracker::new_with_dirty_handler(FocusDelegationPropertyTracker { obj });

        let result = Box::pin(Self {
            state_tracker,
            value_tracker,
            text_tracker,
            label_tracker,
            description_tracker,
            focus_delegation_tracker,
            item: item.clone(),
        });

        result.as_ref().arm_state_tracker();
        result.as_ref().arm_value_tracker();
        result.as_ref().arm_text_tracker();
        result.as_ref().arm_label_tracker();
        result.as_ref().arm_description_tracker();
        result.as_ref().arm_focus_delegation_tracker();

        result
    }

    fn arm_state_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        let p = self.project_ref();
        p.state_tracker.evaluate_as_dependency_root(move || {
            if let Some(item_rc) = item.upgrade() {
                item_rc.accessible_string_property(AccessibleStringProperty::Enabled);
                item_rc.accessible_string_property(AccessibleStringProperty::ItemSelectable);
                item_rc.accessible_string_property(AccessibleStringProperty::ItemSelected);
                item_rc.is_visible();
                item_rc.accessible_string_property(AccessibleStringProperty::Checkable);
                item_rc.accessible_string_property(AccessibleStringProperty::Checked);
                item_rc.accessible_string_property(AccessibleStringProperty::Expandable);
                item_rc.accessible_string_property(AccessibleStringProperty::Expanded);
                item_rc.accessible_string_property(AccessibleStringProperty::ReadOnly);
                item_rc.accessible_string_property(AccessibleStringProperty::Orientation);
            }
        });
    }

    fn arm_value_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        let p = self.project_ref();
        p.value_tracker.evaluate_as_dependency_root(move || {
            if let Some(item_rc) = item.upgrade() {
                item_rc.accessible_string_property(AccessibleStringProperty::Value);
                item_rc.accessible_string_property(AccessibleStringProperty::ValueMinimum);
                item_rc.accessible_string_property(AccessibleStringProperty::ValueMaximum);
                item_rc.accessible_string_property(AccessibleStringProperty::ValueStep);
            }
        });
    }

    fn arm_text_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        self.project_ref().text_tracker.evaluate_as_dependency_root(move || {
            let _ = text::snapshot(&item);
        });
    }

    fn arm_label_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        let p = self.project_ref();
        p.label_tracker.evaluate_as_dependency_root(move || {
            if let Some(item_rc) = item.upgrade() {
                item_rc.accessible_string_property(AccessibleStringProperty::Label);
                item_rc.accessible_string_property(AccessibleStringProperty::LiveRegion);
                item_rc.is_visible();
            }
        });
    }

    fn arm_description_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        let p = self.project_ref();
        p.description_tracker.evaluate_as_dependency_root(move || {
            if let Some(item_rc) = item.upgrade() {
                item_rc.accessible_string_property(AccessibleStringProperty::Description);
            }
        });
    }

    fn arm_focus_delegation_tracker(self: Pin<&Self>) {
        let item = self.item.clone();
        let p = self.project_ref();
        p.focus_delegation_tracker.evaluate_as_dependency_root(move || {
            if let Some(item_rc) = item.upgrade() {
                item_rc.accessible_string_property(AccessibleStringProperty::DelegateFocus);
            }
        });
    }
}

cpp! {{
    // Note: Do not include <QtWidgets> to avoid inclusion of gl.h (see #10989).
    #include <QtGui/QAccessible>
    #include <QtCore/QTimer>
    #include <QtCore/QPointer>
    #include <QtCore/QTextBoundaryFinder>
    #include <QtWidgets/QWidget>

    #include <memory>

    /// KEEP IN SYNC WITH CONSTANTS IN RUST!
    const uint32_t CHECKED { QAccessible::UserText };
    const uint32_t VALUE_MINIMUM { CHECKED + 1 };
    const uint32_t VALUE_MAXIMUM { VALUE_MINIMUM + 1 };
    const uint32_t VALUE_STEP { VALUE_MAXIMUM + 1 };
    const uint32_t CHECKABLE { VALUE_STEP + 1 };
    const uint32_t EXPANDABLE { CHECKABLE + 1 };
    const uint32_t EXPANDED { EXPANDABLE + 1 };
    const uint32_t READ_ONLY { EXPANDED + 1 };
    const uint32_t ENABLED { READ_ONLY + 1 };
    const uint32_t ITEM_SELECTABLE { ENABLED + 1 };
    const uint32_t ITEM_SELECTED { ITEM_SELECTABLE + 1 };
    const uint32_t VISIBLE { ITEM_SELECTED + 1 };
    const uint32_t LIVE_REGION { VISIBLE + 1 };

    struct SlintTextSelection { int start; int end; };

    // Pure native adapter mappings, also exercised without an OS accessibility service.
    static QAccessible::State slint_item_state(QAccessible::Role role, bool focused,
        bool enabled, bool visible, bool checkable, bool checked,
        bool expandable, bool expanded, bool readOnly, bool selectable, bool selected) {
        QAccessible::State state;
        state.active = true;
        state.disabled = !enabled;
        state.invisible = !visible;
        state.focusable = role == QAccessible::Button || role == QAccessible::CheckBox
            || role == QAccessible::ComboBox || role == QAccessible::ListItem
            || role == QAccessible::EditableText || role == QAccessible::Slider
            || role == QAccessible::SpinBox || role == QAccessible::PageTab
            || role == QAccessible::RadioButton;
        state.focused = focused;
        state.checkable = checkable;
        state.checked = checked;
        state.expandable = expandable;
        state.expanded = expandable && expanded;
        state.collapsed = expandable && !expanded;
        state.readOnly = readOnly;
        state.selectable = selectable;
        state.selected = selectable && selected;
        return state;
    }

    static QAccessible::State slint_changed_item_states(const QAccessible::State &before,
        const QAccessible::State &after) {
        QAccessible::State changed;
        changed.disabled = before.disabled != after.disabled;
        changed.invisible = before.invisible != after.invisible;
        changed.focusable = before.focusable != after.focusable;
        changed.focused = before.focused != after.focused;
        changed.checkable = before.checkable != after.checkable;
        changed.checked = before.checked != after.checked;
        changed.expandable = before.expandable != after.expandable;
        changed.expanded = before.expanded != after.expanded;
        changed.collapsed = before.collapsed != after.collapsed;
        changed.readOnly = before.readOnly != after.readOnly;
        changed.selectable = before.selectable != after.selectable;
        changed.selected = before.selected != after.selected;
        changed.selectableText = before.selectableText != after.selectableText;
        changed.multiLine = before.multiLine != after.multiLine;
        return changed;
    }

    static int slint_announcement_kind(const QString &live, bool visible,
        const QString &before, const QString &after) {
        if (!visible || after.isEmpty() || before == after) return 0;
        return live == "polite" ? 1 : live == "assertive" ? 2 : 0;
    }

    // ------------------------------------------------------------------------------
    // Helper:
    // ------------------------------------------------------------------------------

    class Descendents {
    public:
        Descendents(void *root_item, bool popupRootsOnly = false) {
            rustDescendents = rust!(Descendents_ctor [root_item: &ItemWeak as "void*", popupRootsOnly: bool as "bool"] ->
                    SharedVector<ItemRc> as "void*" {
                root_item.upgrade().map_or_else(SharedVector::default, |root| {
                    if popupRootsOnly { popups::popup_roots(&root) } else { popups::children(&root) }
                })
            });
        }

        size_t count() const {
            return rust!(Descendents_count [rustDescendents: SharedVector<ItemRc> as "void*"] -> usize as "size_t" {
               rustDescendents.len()
            });
        }

        void* itemAt(size_t index) {
            return rust!(Descendents_itemAt [rustDescendents: SharedVector<ItemRc> as "void*",
                                             index: usize as "size_t"]
                    -> *mut ItemWeak as "void*" {
                let item_rc = rustDescendents[index].clone();
                let mut item_weak = Box::new(item_rc.downgrade());

                Box::into_raw(item_weak)
            });
        }

        QAccessible::Role roleAt(size_t index) const {
            return rust!(Descendents_roleAt [rustDescendents: SharedVector<ItemRc> as "void*",
                                             index: usize as "size_t"]
                    -> u32 as "QAccessible::Role" {
                match rustDescendents[index].accessible_role() {
                    i_slint_core::items::AccessibleRole::None => QAccessible_Role_NoRole,
                    i_slint_core::items::AccessibleRole::Button => QAccessible_Role_Button,
                    i_slint_core::items::AccessibleRole::Checkbox => QAccessible_Role_CheckBox,
                    i_slint_core::items::AccessibleRole::Combobox => QAccessible_Role_ComboBox,
                    i_slint_core::items::AccessibleRole::List => QAccessible_Role_List,
                    i_slint_core::items::AccessibleRole::Slider => QAccessible_Role_Slider,
                    i_slint_core::items::AccessibleRole::Spinbox => QAccessible_Role_SpinBox,
                    i_slint_core::items::AccessibleRole::Tab => QAccessible_Role_PageTab,
                    i_slint_core::items::AccessibleRole::TabList => QAccessible_Role_PageTabList,
                    i_slint_core::items::AccessibleRole::Text => QAccessible_Role_StaticText,
                    i_slint_core::items::AccessibleRole::ProgressIndicator => QAccessible_Role_ProgressBar,
                    i_slint_core::items::AccessibleRole::Table => QAccessible_Role_Table,
                    i_slint_core::items::AccessibleRole::Tree => QAccessible_Role_Tree,
                    i_slint_core::items::AccessibleRole::TextInput => QAccessible_Role_EditableText,
                    i_slint_core::items::AccessibleRole::Switch => QAccessible_Role_CheckBox,
                    i_slint_core::items::AccessibleRole::ListItem => QAccessible_Role_ListItem,
                    i_slint_core::items::AccessibleRole::TabPanel => QAccessible_Role_Pane,
                    i_slint_core::items::AccessibleRole::Groupbox => QAccessible_Role_Grouping,
                    i_slint_core::items::AccessibleRole::Image => QAccessible_Role_Graphic,
                    i_slint_core::items::AccessibleRole::RadioButton => QAccessible_Role_RadioButton,
                    i_slint_core::items::AccessibleRole::RadioGroup => QAccessible_Role_Grouping,
                    i_slint_core::items::AccessibleRole::WindowTitleBar => QAccessible_Role_TitleBar,
                    i_slint_core::items::AccessibleRole::Banner => QAccessible_Role_Section,
                    i_slint_core::items::AccessibleRole::Complementary => QAccessible_Role_ComplementaryContent,
                    i_slint_core::items::AccessibleRole::ContentInfo => QAccessible_Role_Footer,
                    i_slint_core::items::AccessibleRole::Form => QAccessible_Role_Form,
                    i_slint_core::items::AccessibleRole::Main => QAccessible_Role_Grouping,
                    i_slint_core::items::AccessibleRole::Navigation => QAccessible_Role_Grouping,
                    i_slint_core::items::AccessibleRole::Region => QAccessible_Role_Section,
                    i_slint_core::items::AccessibleRole::Search => QAccessible_Role_Grouping,
                    _ => QAccessible_Role_NoRole,
                }
            });
        }

        ~Descendents() {
            auto descendentsPtr = &rustDescendents;
            rust!(Descendents_dtor [descendentsPtr: *mut SharedVector<ItemRc> as "void**"] {
                unsafe { core::ptr::read(descendentsPtr); }
            });
        }

    private:
        void *rustDescendents;
    };

    void *root_item_for_window(void *rustWindow, void *existing) {
        return rust!(root_item_for_window_ [rustWindow: *const crate::qt_window::QtWindow as "void*", existing: *mut ItemWeak as "void*"]
                -> *mut ItemWeak as "void*" {
            let window = unsafe { upgrade_borrowed_weak(rustWindow) };
            let current = window.as_ref().and_then(|window| WindowInner::from_pub(&window.window).try_component())
                .map(|component| ItemRc::new_root(component).downgrade()).unwrap_or_default();
            if existing.is_null() {
                Box::into_raw(Box::new(current))
            } else {
                unsafe { *existing = current; }
                existing
            }
        });
    }

    // Returns the orientation of the item as a Qt::Orientation value
    // (Qt::Horizontal=1, Qt::Vertical=2), or 0 if the property is not set.
    int item_qt_orientation(void *data) {
        return rust!(item_qt_orientation_
            [data: &SlintAccessibleItemData as "void*"]
                -> i32 as "int" {
            data.item.upgrade()
                .and_then(|i| i.accessible_string_property(AccessibleStringProperty::Orientation))
                .and_then(|s| s.parse::<i_slint_core::items::Orientation>().ok())
                .map(|o| match o {
                    i_slint_core::items::Orientation::Horizontal => 1,
                    i_slint_core::items::Orientation::Vertical => 2,
                })
                .unwrap_or(0)
        });
    }

    QString item_string_property(void *data, uint32_t what) {
        return rust!(item_string_property_
            [data: &SlintAccessibleItemData as "void*", what: u32 as "uint32_t"]
                -> QString as "QString" {

            if let Some(item) = data.item.upgrade() {
                let string = match what {
                    NAME => item.accessible_string_property(AccessibleStringProperty::Label),
                    DESCRIPTION => item.accessible_string_property(AccessibleStringProperty::Description),
                    VALUE => item.accessible_string_property(AccessibleStringProperty::Value),
                    CHECKED => item.accessible_string_property(AccessibleStringProperty::Checked),
                    VALUE_MINIMUM => item.accessible_string_property(AccessibleStringProperty::ValueMinimum),
                    VALUE_MAXIMUM => item.accessible_string_property(AccessibleStringProperty::ValueMaximum),
                    VALUE_STEP => item.accessible_string_property(AccessibleStringProperty::ValueStep),
                    CHECKABLE => item.accessible_string_property(AccessibleStringProperty::Checkable),
                    EXPANDABLE => item.accessible_string_property(AccessibleStringProperty::Expandable),
                    EXPANDED => item.accessible_string_property(AccessibleStringProperty::Expanded),
                    READ_ONLY => item.accessible_string_property(AccessibleStringProperty::ReadOnly),
                    ENABLED => Some(item.accessible_string_property(AccessibleStringProperty::Enabled)
                        .unwrap_or_else(|| "true".into())),
                    ITEM_SELECTABLE => item.accessible_string_property(AccessibleStringProperty::ItemSelectable),
                    ITEM_SELECTED => item.accessible_string_property(AccessibleStringProperty::ItemSelected),
                    VISIBLE => Some(if item.is_visible() { "true".into() } else { "false".into() }),
                    LIVE_REGION => item.accessible_string_property(AccessibleStringProperty::LiveRegion),
                    _ => None,
                };
                if let Some(string) = string {
                    return QString::from(string.as_ref())
                }
            };
            QString::default()
        });
    }

    // ------------------------------------------------------------------------------
    // Slint_accessible:
    // ------------------------------------------------------------------------------

    // Base object for accessibility support
    class Slint_accessible_item;
    static Slint_accessible_item *live_accessible_item(QObject *object);

    class Slint_accessible : public QAccessibleInterface {
    public:
        Slint_accessible(QAccessible::Role role, QAccessibleInterface *parent) :
             has_focus(false), has_focus_delegation(false), m_role(role), m_parent(parent)
        { }

        ~Slint_accessible() {
            qDeleteAll(m_children);
        }

        virtual void *rustItem() const = 0;

        // Returns the SlintWidget of the window... we have no other.
        virtual QWidget *qwidget() const = 0;

        QPoint mapToGlobal(const QPoint p) const {
            return qwidget()->mapToGlobal(p);
        }

        QPoint mapFromGlobal(const QPoint p) const {
            return qwidget()->mapFromGlobal(p);
        }

        void clearFocus() {
            has_focus = false;
            has_focus_delegation = false;

            for (int i = 0; i < rawChildCount(); ++i) {
                static_cast<Slint_accessible *>(child(i))->clearFocus();
            }
        }

        virtual void delegateFocus() const {
            sendFocusChangeEvent();
        }

        // Returns true if the item accepted the focus; false otherwise.
        bool focusItem(void *item) const {
            auto my_item = rustItem();
            if (rust!(Slint_accessible_findItem [item: &ItemWeak as "void *", my_item: &ItemWeak as "void*"] -> bool as "bool" {
                item == my_item
            })) {
                has_focus = true;

                delegateFocus();
                return true;
            }

            for (int i = 0; i < rawChildCount(); ++i) {
                if (static_cast<Slint_accessible *>(child(i))->focusItem(item)) {
                    return true;
                }
            }
            return false;
        }

        void sendFocusChangeEvent() const {
            auto event = QAccessibleEvent(object(), QAccessible::Focus);
            QAccessible::updateAccessibility(&event);
            has_focus_delegation = true;
        }

        bool isValid() const override {
            return m_attached;
        }

        void detach() {
            if (!m_attached) return;
            for (int i = 0; i < rawChildCount(); ++i) {
                if (auto child = dynamic_cast<Slint_accessible*>(rawChild(i))) child->detach();
            }
            m_attached = false;
        }

        Slint_accessible *findItemInterface(void *item) const {
            if (!isValid()) return nullptr;
            auto mine = rustItem();
            if (rust!(Slint_accessible_find_interface [item: &ItemWeak as "void*", mine: &ItemWeak as "void*"] -> bool as "bool" {
                item == mine
            })) return const_cast<Slint_accessible*>(this);
            for (int i = rawChildCount() - 1; i >= 0; --i) {
                auto child = dynamic_cast<Slint_accessible*>(rawChild(i));
                if (child) {
                    if (auto found = child->findItemInterface(item)) return found;
                }
            }
            return nullptr;
        }

        // navigation, hierarchy
        QAccessibleInterface *parent() const override {
            return m_parent;
        }

        QAccessibleInterface *focusChild() const override {
            if (has_focus_delegation) {
                return const_cast<QAccessibleInterface *>(static_cast<const QAccessibleInterface *>(this));
            }
            for (int i = 0; i < childCount(); ++i)  {
                if (auto focus = child(i)->focusChild()) return focus;
            }
            return nullptr;
        }

        int indexOfChild(const QAccessibleInterface *child) const override {
            return m_children.indexOf(child->object()); // FIXME: Theoretically we can have several QAIs per QObject!
        }

        // Will *not* trigger a build of the accessibility item tree!
        // Use this from the Slint side to make sure the accessibility
        // item tree is not generated needlessly.
        int rawChildCount() const {
            return m_children.count();
        }

        /// Will *not* trigger a build of the accessibility tree!
        QAccessibleInterface *rawChild(int index) const {
            if (0 <= index && index < rawChildCount())
                return QAccessible::queryAccessibleInterface(m_children[index]);
            return nullptr;
        }

        // May trigger a build of the accessibility item tree!
        // Use this from the Qt API side (which is triggered by the OS accessibility
        // layer to make sure accessibility information is up-to-date.
        int childCount() const override {
            return rawChildCount();
        }

        QAccessibleInterface *child(int index) const override {
            if (0 <= index && index < childCount())
                return QAccessible::queryAccessibleInterface(m_children[index]);
            return nullptr;
        }

        void setText(QAccessible::Text t, const QString &text) override {
            Q_UNUSED(t); Q_UNUSED(text);
        }

        QAccessible::Role role() const override {
            return m_role;
        }

        QRect rect() const override {
            auto item = rustItem();
            QRectF r = rust!(Slint_accessible_item_rect
                [item: *const ItemWeak as "void*"] -> qttypes::QRectF as "QRectF" {
                    if let Some(item_rc) = unsafe { item.as_ref().unwrap().upgrade() } {
                        popups::native_rect(&item_rc)
                    } else {
                        Default::default()
                    }
                });
            auto topLeft = mapToGlobal(QPoint(static_cast<int>(r.left()), static_cast<int>(r.top())));
            auto bottomRight = mapToGlobal(QPoint(static_cast<int>(r.right()), static_cast<int>(r.bottom())));
            return QRect(topLeft, bottomRight);
        }

        QRect hitRect() const {
            if (!isValid() || !qwidget()->isVisible()) return QRect();
            auto item = rustItem();
            auto rect = rust!(Slint_accessible_hit_rect [item: &ItemWeak as "void*"] -> qttypes::QRectF as "QRectF" {
                item.upgrade().map_or_else(Default::default, |item| popups::hit_rect(&item))
            }).toAlignedRect();
            if (!rect.isEmpty()) rect.moveTopLeft(mapToGlobal(rect.topLeft()));
            return rect;
        }

        QAccessibleInterface *childAt(int x, int y) const override {
            if (!isValid() || !qwidget()->isVisible()) return nullptr;
            for (int i = childCount() - 1; i >= 0; --i)  {
                auto c = child(i);
                if (!c || !c->isValid() || c->state().invisible) continue;
                // An un-clipped invocation button may own an overflow popup.
                // Search descendants before testing the parent's own bounds.
                if (auto nested = c->childAt(x, y)) return nested;
                auto slint = dynamic_cast<Slint_accessible*>(c);
                if (slint && slint->hitRect().contains(x, y)) return c;
            }
            return nullptr;
        }

        void updateAccessibilityTree(bool announceNewSubtree = false) const;

    protected:
        mutable bool has_focus;
        mutable bool has_focus_delegation;

    private:
        QAccessible::Role m_role = QAccessible::NoRole;
        QAccessibleInterface *m_parent = nullptr;
        mutable QList<QObject*> m_children;
        mutable bool m_tree_initialized = false;
        bool m_attached = true;
    };

    // ------------------------------------------------------------------------------
    // Slint_accessible_item:
    // ------------------------------------------------------------------------------

}}

cpp! {{
    class Slint_accessible_item : public Slint_accessible, public QAccessibleValueInterface, public QAccessibleActionInterface, public QAccessibleTextInterface
#if QT_VERSION >= QT_VERSION_CHECK(6, 11, 0)
        , public QAccessibleAttributesInterface
#endif
    {
    public:
        Slint_accessible_item(void *item, QObject *obj, QAccessible::Role role, QAccessibleInterface *parent) :
            Slint_accessible(role, parent), m_object(obj)
        {
            m_data = rust!(Slint_accessible_item_ctor [obj: *mut c_void as "QObject*",
                    item: &ItemWeak as "void*"] ->
                    *mut SlintAccessibleItemData as "void*" {
                        let data = SlintAccessibleItemData::new_pin_box(obj, item);
                        unsafe { Box::into_raw(Pin::into_inner_unchecked(data)) }
            });
            m_last_state = state();
            m_last_label = text(QAccessible::Name);
            if (hasTextInterface()) {
                m_last_text = text(0, -1);
                m_last_cursor = cursorPosition();
                m_last_selection = selectionState();
            }
        }

        QAccessibleActionInterface *actionInterface() { return this; }
        QAccessibleValueInterface *valueInterface() { return this; }


        ~Slint_accessible_item() {
            rust!(Slint_accessible_item_dtor [m_data: *mut SlintAccessibleItemData as "void*"] {
                unsafe { Pin::new_unchecked(Box::from_raw(m_data)) };
            });
        }

        void *rustItem() const override {
            return rust!(Slint_accessible_item_rustItem [m_data: Pin<&SlintAccessibleItemData> as "void*"] -> *const ItemWeak as "void*" {
                &m_data.item
            });
        }

        QObject *object() const override {
            return m_object;
        }

        QWidget *qwidget() const override {
            return dynamic_cast<Slint_accessible *>(parent())->qwidget();
        }

        void *data() const {
            return m_data;
        }

        bool isValid() const override {
            if (!Slint_accessible::isValid() || !m_data) return false;
            return rust!(Slint_accessible_item_valid [m_data: &SlintAccessibleItemData as "void*"] -> bool as "bool" {
                m_data.item.upgrade().is_some()
            });
        }

        QWindow *window() const override {
            return parent()->window();
        }

        void delegateFocus() const override {
            if (!has_focus) { return; }

            auto index = rust!(Slint_accessible_item_delegate_focus [m_data: Pin<&SlintAccessibleItemData> as "void*"] -> i32 as "int" {
                m_data.item.upgrade()
                    .and_then(|i| { i.accessible_string_property(AccessibleStringProperty::DelegateFocus) })
                    .and_then(|s| s.as_str().parse::<i32>().ok()).unwrap_or(-1)
            });

            if (index >= 0 && index < rawChildCount()) {
                static_cast<Slint_accessible_item*>(rawChild(index))->sendFocusChangeEvent();
            } else {
                sendFocusChangeEvent();
            }
        }

        // properties and state
        QString text(QAccessible::Text t) const override {
            return item_string_property(m_data, t);
        }

        QAccessible::State state() const override {
            auto result = slint_item_state(role(), has_focus_delegation,
                item_string_property(m_data, ENABLED) == "true",
                isVisibleToAccessibility(),
                item_string_property(m_data, CHECKABLE) == "true",
                item_string_property(m_data, CHECKED) == "true",
                item_string_property(m_data, EXPANDABLE) == "true",
                item_string_property(m_data, EXPANDED) == "true",
                item_string_property(m_data, READ_ONLY) == "true",
                item_string_property(m_data, ITEM_SELECTABLE) == "true",
                item_string_property(m_data, ITEM_SELECTED) == "true");
            if (hasTextInterface()) {
                result.selectableText = true;
                const auto flags = rust!(Slint_text_flags [m_data: &SlintAccessibleItemData as "void*"] -> u32 as "uint32_t" {
                    text::snapshot(&m_data.item).map_or(0, |value| u32::from(value.read_only) | (u32::from(value.multi_line) << 1))
                });
                result.readOnly = flags & 1;
                result.multiLine = (flags & 2) != 0;
            }
            result.invalid = !isValid();
            return result;
        }

        void sendStateChangeEvent() {
            if (!isValid()) return;
            const auto current = state();
            const auto changed = slint_changed_item_states(m_last_state, current);
            m_last_state = current;
            if (changed == QAccessible::State()) return;
            auto event = QAccessibleStateChangeEvent(m_object, changed);
            QAccessible::updateAccessibility(&event);
        }

        bool isVisibleToAccessibility() const {
            return isValid() && qwidget()->isVisible() && item_string_property(m_data, VISIBLE) == "true";
        }

        void queueInsertedLiveAnnouncement() {
            const auto live = item_string_property(m_data, LIVE_REGION);
            const auto current = text(QAccessible::Name);
            if (slint_announcement_kind(live, isVisibleToAccessibility(),
                    QString(), current) == 0) return;
            // This node was inserted after its parent's initial accessibility
            // tree. Defer until every ancestor's child cache has been committed.
            // A property notification may run first; the shared label cache
            // makes the announcement exactly once in either ordering.
            m_last_label.clear();
            QTimer::singleShot(0, [obj = QPointer(m_object)]() {
                if (!obj) return;
                auto item = live_accessible_item(obj);
                if (!item) return;
                item->sendNameChangeEvent();
            });
        }

        void sendNameChangeEvent() {
            if (!isValid()) return;
            const auto current = text(QAccessible::Name);
            if (current == m_last_label) return;
            const auto announcement = slint_announcement_kind(
                item_string_property(m_data, LIVE_REGION),
                isVisibleToAccessibility(), m_last_label, current);
            m_last_label = current;
            auto event = QAccessibleEvent(m_object, QAccessible::NameChanged);
            QAccessible::updateAccessibility(&event);
            if (announcement != 0) {
                auto event = QAccessibleAnnouncementEvent(m_object, current);
                event.setPoliteness(announcement == 2 ? QAccessible::AnnouncementPoliteness::Assertive
                    : QAccessible::AnnouncementPoliteness::Polite);
                QAccessible::updateAccessibility(&event);
            }
        }

        void *interface_cast(QAccessible::InterfaceType t) override {
            if (!isValid()) return nullptr;
            if (t == QAccessible::TextInterface && hasTextInterface()) {
                return static_cast<QAccessibleTextInterface*>(this);
            } else if (t == QAccessible::ValueInterface && !hasTextInterface() && !item_string_property(m_data, QAccessible::Value).isEmpty()) {
                return static_cast<QAccessibleValueInterface*>(this);
            } else if (t == QAccessible::ActionInterface) {
                return static_cast<QAccessibleActionInterface*>(this);
            }
#if QT_VERSION >= QT_VERSION_CHECK(6, 11, 0)
            else if (t == QAccessible::AttributesInterface) {
                return static_cast<QAccessibleAttributesInterface*>(this);
            }
#endif
            return QAccessibleInterface::interface_cast(t);
        }

#if QT_VERSION >= QT_VERSION_CHECK(6, 11, 0)
        // QAccessibleAttributesInterface
        QList<QAccessible::Attribute> attributeKeys() const override {
            QList<QAccessible::Attribute> keys;
            if (item_qt_orientation(m_data) != 0) {
                keys << QAccessible::Attribute::Orientation;
            }
            return keys;
        }

        QVariant attributeValue(QAccessible::Attribute key) const override {
            if (key == QAccessible::Attribute::Orientation) {
                int o = item_qt_orientation(m_data);
                if (o != 0) {
                    return QVariant::fromValue(static_cast<Qt::Orientation>(o));
                }
            }
            return QVariant();
        }
#endif

        bool hasTextInterface() const {
            if (!isValid() || role() != QAccessible::EditableText) return false;
            return rust!(Slint_has_text [m_data: &SlintAccessibleItemData as "void*"] -> bool as "bool" {
                text::snapshot(&m_data.item).is_some()
            });
        }

        SlintTextSelection selectionState() const {
            return rust!(Slint_text_selection [m_data: &SlintAccessibleItemData as "void*"] -> text::Selection as "SlintTextSelection" {
                text::selection(&m_data.item)
            });
        }

        void selection(int index, int *start, int *end) const override {
            const auto value = selectionState();
            if (index != 0 || value.start < 0 || value.start == value.end) { *start = *end = -1; return; }
            *start = value.start; *end = value.end;
        }
        int selectionCount() const override {
            const auto value = selectionState();
            return value.start >= 0 && value.start != value.end ? 1 : 0;
        }
        void addSelection(int start, int end) override { setSelection(0, start, end); }
        void removeSelection(int index) override {
            if (index == 0 && selectionCount() != 0) setCursorPosition(cursorPosition());
        }
        void setSelection(int index, int start, int end) override {
            if (index != 0) return;
            rust!(Slint_text_set_selection [m_data: &SlintAccessibleItemData as "void*", start: i32 as "int", end: i32 as "int"] {
                text::set_selection(&m_data.item, start, end);
            });
        }
        int cursorPosition() const override {
            return rust!(Slint_text_cursor [m_data: &SlintAccessibleItemData as "void*"] -> i32 as "int" {
                text::snapshot(&m_data.item).map_or(-1, |value| value.cursor)
            });
        }
        void setCursorPosition(int position) override { setSelection(0, position, position); }
        QString text(int start, int end) const override {
            return rust!(Slint_text_range [m_data: &SlintAccessibleItemData as "void*", start: i32 as "int", end: i32 as "int"] -> QString as "QString" {
                text::range(&m_data.item, start, end).map_or_else(QString::default, |value| QString::from(value.as_str()))
            });
        }
        QString textBeforeOffset(int offset, QAccessible::TextBoundaryType boundary, int *start, int *end) const override {
            return QAccessibleTextInterface::textBeforeOffset(offset == -2 ? cursorPosition() : offset, boundary, start, end);
        }
        QString textAfterOffset(int offset, QAccessible::TextBoundaryType boundary, int *start, int *end) const override {
            return QAccessibleTextInterface::textAfterOffset(offset == -2 ? cursorPosition() : offset, boundary, start, end);
        }
        QString textAtOffset(int offset, QAccessible::TextBoundaryType boundary, int *start, int *end) const override {
            return QAccessibleTextInterface::textAtOffset(offset == -2 ? cursorPosition() : offset, boundary, start, end);
        }
        int characterCount() const override { return text(0, -1).size(); }
        QString attributes(int offset, int *start, int *end) const override {
            const auto count = characterCount();
            if (offset == -1) offset = count;
            if (offset == -2) offset = cursorPosition();
            if (offset < 0 || offset > count) { *start = *end = -1; return QString(); }
            *start = 0; *end = count;
            return QString(); // No fabricated uniform font attributes.
        }
        QRect characterRect(int offset) const override {
            const auto value = text(0, -1);
            if (offset < 0 || offset > value.size()) return QRect();
            QTextBoundaryFinder finder(QTextBoundaryFinder::Grapheme, value);
            finder.setPosition(offset);
            int start = finder.isAtBoundary() ? offset : finder.toPreviousBoundary();
            finder.setPosition(start);
            int end = finder.toNextBoundary();
            if (end < 0) end = value.size();
            auto rect = rust!(Slint_text_rect [m_data: &SlintAccessibleItemData as "void*", start: i32 as "int", end: i32 as "int"] -> qttypes::QRectF as "QRectF" {
                text::character_rect(&m_data.item, start, end).unwrap_or_default()
            }).toAlignedRect();
            if (rect.isEmpty()) return QRect();
            rect.moveTopLeft(mapToGlobal(rect.topLeft()));
            return rect;
        }
        int offsetAtPoint(const QPoint &point) const override {
            if (!isVisibleToAccessibility()) return -1;
            const auto local = mapFromGlobal(point);
            if (!qwidget()->rect().contains(local)) return -1;
            float x = local.x(), y = local.y();
            return rust!(Slint_text_hit [m_data: &SlintAccessibleItemData as "void*", x: f32 as "float", y: f32 as "float"] -> i32 as "int" {
                text::offset_at_point(&m_data.item, x, y)
            });
        }
        void scrollToSubstring(int start, int end) override {
            rust!(Slint_text_scroll [m_data: &SlintAccessibleItemData as "void*", start: i32 as "int", end: i32 as "int"] {
                text::scroll_to_range(&m_data.item, start, end);
            });
        }

        void sendTextChangeEvents() {
            if (!hasTextInterface()) return;
            const auto current = text(0, -1);
            const auto cursor = cursorPosition();
            const auto selected = selectionState();
            const auto before = m_last_text;
            const auto beforeCursor = m_last_cursor;
            const auto beforeSelection = m_last_selection;
            m_last_text = current; m_last_cursor = cursor; m_last_selection = selected;
            if (current != before) {
                auto event = QAccessibleTextUpdateEvent(m_object, 0, before, current);
                QAccessible::updateAccessibility(&event);
            }
            if (selected.start != beforeSelection.start || selected.end != beforeSelection.end) {
                auto event = QAccessibleTextSelectionEvent(m_object, selected.start, selected.end);
                event.setCursorPosition(cursor);
                QAccessible::updateAccessibility(&event);
            }
            if (cursor != beforeCursor) {
                auto event = QAccessibleTextCursorEvent(m_object, cursor);
                QAccessible::updateAccessibility(&event);
            }
        }

        // AccessibleValueInterface:
        QVariant currentValue() const override {
            return item_string_property(m_data, QAccessible::Value);
        }

        void setCurrentValue(const QVariant &value) override {
            QString value_string = value.toString();
            rust!(Slint_accessible_setCurrentValue [m_data: Pin<&SlintAccessibleItemData> as "void*", value_string: qttypes::QString as "QString"] {
                let Some(item) = m_data.item.upgrade() else {return};
                item.accessible_action(&AccessibilityAction::SetValue(i_slint_core::format!("{value_string}")));
            });
        }

        QVariant maximumValue() const override {
            return item_string_property(m_data, VALUE_MAXIMUM);
        }

        QVariant minimumValue() const override {
            return item_string_property(m_data, VALUE_MINIMUM);
        }

        QVariant minimumStepSize() const override {
            return item_string_property(m_data, VALUE_STEP);
        }

        QStringList actionNames() const override {
            int supported = rust!(Slint_accessible_item_supported [m_data: Pin<&SlintAccessibleItemData> as "void*"] -> SupportedAccessibilityAction as "uint" {
                m_data.item.upgrade().map(|i| i.supported_accessibility_actions()).unwrap_or_default()
            });
            QStringList actions;
            if (supported & rust!(Slint_accessible_item_an1 [] -> SupportedAccessibilityAction as "uint" { SupportedAccessibilityAction::Default }))
                actions << QAccessibleActionInterface::pressAction();
            if (supported & rust!(Slint_accessible_item_an2 [] -> SupportedAccessibilityAction as "uint" { SupportedAccessibilityAction::Increment }))
                actions << QAccessibleActionInterface::increaseAction();
            if (supported & rust!(Slint_accessible_item_an3 [] -> SupportedAccessibilityAction as "uint" { SupportedAccessibilityAction::Decrement }))
                actions << QAccessibleActionInterface::decreaseAction();
            if (supported & rust!(Slint_accessible_item_an4 [] -> SupportedAccessibilityAction as "uint" { SupportedAccessibilityAction::Expand }))
                actions << QAccessibleActionInterface::pressAction();
            // Existing activation remains the default action. Focusing is an
            // additional capability, not a replacement for Press at index zero.
            const auto current = state();
            if (isValid() && qwidget()->isVisible() && current.focusable && !current.disabled)
                actions << QAccessibleActionInterface::setFocusAction();
            return actions;
        }

        void doAction(const QString &actionName) override {
            if (!isValid() || state().disabled || !qwidget()->isVisible()) return;
            if (actionName == QAccessibleActionInterface::setFocusAction()) {
                if (!state().focusable) return;
                qwidget()->activateWindow();
                qwidget()->setFocus(Qt::OtherFocusReason);
                rust!(Slint_accessible_set_focus [m_data: &SlintAccessibleItemData as "void*"] {
                    if let Some(item) = m_data.item.upgrade() {
                        if let Some(adapter) = item.window_adapter() {
                            WindowInner::from_pub(adapter.window()).set_focus_item(
                                &item, true, i_slint_core::input::FocusReason::PointerClick);
                        }
                    }
                });
            } else if (actionName == QAccessibleActionInterface::pressAction()) {
                rust!(Slint_accessible_item_do_action1 [m_data: Pin<&SlintAccessibleItemData> as "void*"] {
                    let Some(item) = m_data.item.upgrade() else {return};
                    let supported_actions = item.supported_accessibility_actions();
                    if supported_actions.contains(SupportedAccessibilityAction::Expand) {
                        item.accessible_action(&AccessibilityAction::Expand);
                    } else {
                        item.accessible_action(&AccessibilityAction::Default);
                    }
                });
            } else if (actionName == QAccessibleActionInterface::increaseAction()) {
                rust!(Slint_accessible_item_do_action2 [m_data: Pin<&SlintAccessibleItemData> as "void*"] {
                    let Some(item) = m_data.item.upgrade() else {return};
                    item.accessible_action(&AccessibilityAction::Increment);
                });
            } else if (actionName == QAccessibleActionInterface::decreaseAction()) {
                rust!(Slint_accessible_item_do_action3 [m_data: Pin<&SlintAccessibleItemData> as "void*"] {
                    let Some(item) = m_data.item.upgrade() else {return};
                    item.accessible_action(&AccessibilityAction::Decrement);
                });
            }
        }

        QStringList keyBindingsForAction(const QString &) const override {
            return QStringList();
        }

    private:
        QObject *m_object = nullptr;
        mutable void *m_data = nullptr;
        QAccessible::State m_last_state;
        QString m_last_label;
        QString m_last_text;
        int m_last_cursor = -1;
        SlintTextSelection m_last_selection { -1, -1 };
    };

    static Slint_accessible_item *live_accessible_item(QObject *object) {
        auto item = dynamic_cast<Slint_accessible_item*>(QAccessible::queryAccessibleInterface(object));
        return item && item->isValid() ? item : nullptr;
    }

    // ------------------------------------------------------------------------------
}}

cpp! {{
    // Slint_accessible_window:
    // ------------------------------------------------------------------------------

    class Slint_accessible_window : public Slint_accessible {
    public:
        Slint_accessible_window(QWidget *widget, void *rust_window) :
            Slint_accessible(QAccessible::Window, QAccessible::queryAccessibleInterface(qApp)),
            m_widget(widget),
            m_rustWindow(rust_window)
        { }

        ~Slint_accessible_window()
        {
            if (m_rootItem) {
                rust!(Slint_accessible_root_item_dtor [m_rootItem: *mut ItemWeak as "void*"] {
                    drop(unsafe { Box::from_raw(m_rootItem) });
                });
            }
            rust!(Slint_accessible_window_dtor [m_rustWindow: *mut c_void as "void*"] {
                unsafe {
                    alloc::rc::Weak::from_raw(m_rustWindow as *const QtWindow); // Consume the Weak<QtWindow> we hold in our void*!
                }
            });
        }

        bool isUsed() const {
            return is_used;
        }

        void *rustItem() const override {
            m_rootItem = root_item_for_window(m_rustWindow, m_rootItem);
            return m_rootItem;
        }

        QObject *object() const override {
            return m_widget;
        }

        QWidget *qwidget() const override {
            return m_widget;
        }

        QWindow *window() const override {
            return qobject_cast<QWidget *>(object())->windowHandle();
        }

        int childCount() const override {
            if (!is_used) { updateAccessibilityTree(); }
            is_used = true;
            return Slint_accessible::childCount();
        }

        QAccessibleInterface *childAt(int x, int y) const override {
            if (!m_widget->isVisible() || !m_widget->rect().contains(m_widget->mapFromGlobal(QPoint(x, y)))) return nullptr;
            is_used = true;
            // Popups paint above the base scene even when their logical owner
            // appears earlier in the accessibility tree (for example Help).
            updateAccessibilityTree();
            Descendents popups(rustItem(), true);
            for (size_t i = popups.count(); i > 0; --i) {
                auto item = popups.itemAt(i - 1);
                auto popup = findItemInterface(item);
                rust!(Slint_popup_hit_item_dtor [item: *mut ItemWeak as "void*"] {
                    drop(unsafe { Box::from_raw(item) });
                });
                if (!popup || !popup->isValid() || popup->state().invisible) continue;
                if (auto child = popup->childAt(x, y)) return child;
                if (popup->hitRect().contains(x, y)) return popup;
            }
            return Slint_accessible::childAt(x, y);
        }

        // properties and state
        QString text(QAccessible::Text t) const override {
            switch (t) {
                case QAccessible::Name: return qobject_cast<QWidget*>(object())->windowTitle();
                default: return QString();
            }
        }

        QAccessible::State state() const override {
            QAccessible::State state;
            state.active = m_widget->isActiveWindow();
            state.disabled = !m_widget->isEnabled();
            state.invisible = !m_widget->isVisible();
            state.focusable = m_widget->focusPolicy() != Qt::NoFocus;
            state.focused = m_widget->hasFocus();
            return state;
        }

    private:
        QWidget *m_widget;
        void *m_rustWindow; // owned Weak<QtWindow>::into_raw data pointer, not &QtWindow
        mutable void *m_rootItem = nullptr; // owned Box<ItemWeak>, refreshed in place
        mutable bool is_used = false;
    };

    QList<QObject *> deleteStaleItems(QList<QObject *> &&current_children) {
        // Delete no longer valid objects:
        current_children.erase(std::remove_if(current_children.begin(), current_children.end(), [](QObject *o) {
            auto ai = dynamic_cast<Slint_accessible_item *>(QAccessible::queryAccessibleInterface(o));
            if (!ai) { o->deleteLater(); return true; }
            auto data = ai->data();

            if (rust!(Slint_delete_stale_items
                    [data: Pin<&SlintAccessibleItemData> as "void*"] -> bool as "bool" {
                data.item.upgrade().is_none()
            })) {
                ai->detach();
                o->deleteLater();
                return true;
            } else {
                return false;
            }
        }), current_children.end());

        return std::move(current_children);
    }

    int indexOfItem(const QList<QObject *> &existing, void *item) {
        for (int i = 0; i < existing.count(); ++i) {
            auto iface = dynamic_cast<Slint_accessible_item *>(QAccessible::queryAccessibleInterface(existing[i]));
            if (!iface) continue;
            auto data = iface->data();
            if (rust!(Slint_indexOfItems [data: Pin<&SlintAccessibleItemData> as "void*", item: &ItemWeak as "void*"] -> bool as "bool" {
                data.item == *item
            })) {
                return i;
            }
        }
        return -1;
    }

    QList<QObject *> updateItems(QList<QObject *> &&current_children,
                                 Descendents &descendents,
                                 Slint_accessible *parent, bool announceNewChildren) {
        QList<QObject *> children = {};
        children.reserve(descendents.count());

        for (size_t i = 0; i < descendents.count(); ++i) {
            auto item = descendents.itemAt(i);
            auto index = indexOfItem(current_children, item);
            QObject *object = nullptr;
            Slint_accessible_item *ai = nullptr;

            if (index == -1) {
                // Create new item:
                object = new QObject();
                auto role = descendents.roleAt(i);
                ai = new Slint_accessible_item(item, object, role, parent);

                QAccessible::registerAccessibleInterface(ai);
            } else {
                // Reuse existing item:
                object = current_children[index];
                ai = dynamic_cast<Slint_accessible_item *>(QAccessible::queryAccessibleInterface(object));

                current_children.removeAt(index);
            }

            Q_ASSERT(ai);
            Q_ASSERT(object);

            const bool inserted = index == -1;
            ai->updateAccessibilityTree(inserted && announceNewChildren);

            children.append(object);
            if (inserted && announceNewChildren) ai->queueInsertedLiveAnnouncement();
            // itemAt owns a temporary Box<ItemWeak>. The persistent item data
            // cloned that weak reference; neither reused nor new nodes own this box.
            rust!(Slint_descendents_item_dtor [item: *mut ItemWeak as "void*"] {
                drop(unsafe { Box::from_raw(item) });
            });
        }

        // A closed popup's model may remain cached by Slint. Membership in this
        // accessibility tree, rather than model expiry alone, retires its nodes.
        for (auto object : current_children) {
            if (auto item = dynamic_cast<Slint_accessible*>(QAccessible::queryAccessibleInterface(object))) item->detach();
            object->deleteLater();
        }

        return children;
    }

    void Slint_accessible::updateAccessibilityTree(bool announceNewSubtree) const {
        if (!isValid()) return;
        QList<QObject *> valid_objects = deleteStaleItems(std::move(m_children));
        auto descendents = Descendents(rustItem());

        m_children = updateItems(std::move(valid_objects), descendents,
                                 const_cast<Slint_accessible *>(this),
                                 m_tree_initialized || announceNewSubtree);
        m_tree_initialized = true;
    }
}}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_raw_weak_preserves_ownership_and_rejects_expired_data() {
        let window = Weak::<QtWindow>::new().into_raw();
        assert!(unsafe { upgrade_borrowed_weak(window) }.is_none());
        assert!(unsafe { upgrade_borrowed_weak(window) }.is_none());
        let slot = cpp!(unsafe [window as "void*"] -> *mut ItemWeak as "void*" {
            return root_item_for_window(window, nullptr);
        });
        let reused = cpp!(unsafe [window as "void*", slot as "void*"] -> *mut ItemWeak as "void*" {
            return root_item_for_window(window, slot);
        });
        assert_eq!(slot, reused);
        assert!(unsafe { &*slot }.upgrade().is_none());
        drop(unsafe { Box::from_raw(slot) });
        drop(unsafe { Weak::from_raw(window) });
        assert!(unsafe { upgrade_borrowed_weak::<QtWindow>(std::ptr::null()) }.is_none());

        let live = Rc::new(42_u32);
        let raw = Rc::downgrade(&live).into_raw();
        assert_eq!(*unsafe { upgrade_borrowed_weak(raw) }.unwrap(), 42);
        assert_eq!(Rc::strong_count(&live), 1);
        assert_eq!(Rc::weak_count(&live), 1);
        drop(live);
        assert!(unsafe { upgrade_borrowed_weak(raw) }.is_none());
        drop(unsafe { Weak::from_raw(raw) });
    }

    #[test]
    fn missing_model_root_has_an_empty_descendant_snapshot() {
        let item = ItemWeak::default();
        let root = &item;
        let count = cpp!(unsafe [root as "void*"] -> usize as "size_t" {
            Descendents descendants(root);
            return descendants.count();
        });
        assert_eq!(count, 0);
    }

    #[test]
    fn native_state_mapping_preserves_declared_capabilities_and_changes() {
        let correct = cpp!(unsafe [] -> bool as "bool" {
            const auto label = slint_item_state(QAccessible::StaticText, false,
                true, true, false, false, false, false, false, false, false);
            const auto disabled = slint_item_state(QAccessible::Button, false,
                false, true, false, false, false, false, false, false, false);
            const auto selected = slint_item_state(QAccessible::ListItem, true,
                true, true, false, false, false, false, false, true, true);
            const auto unselectable = slint_item_state(QAccessible::ListItem, false,
                true, true, false, false, false, false, false, false, true);
            const auto hidden = slint_item_state(QAccessible::Button, false,
                true, false, true, true, true, false, true, false, false);
            const auto expanded = slint_item_state(QAccessible::Button, false,
                true, true, true, true, true, true, true, false, false);
            const auto changed = slint_changed_item_states(label, disabled);
            return !label.disabled && !label.invisible && !label.focusable
                && disabled.disabled && disabled.focusable
                && selected.selectable && selected.selected && selected.focused
                && !unselectable.selectable && !unselectable.selected
                && hidden.invisible && hidden.checked && hidden.checkable
                && hidden.expandable && hidden.collapsed && !hidden.expanded && hidden.readOnly
                && expanded.expanded && !expanded.collapsed
                && changed.disabled && changed.focusable && !changed.selected
                && slint_changed_item_states(selected, selected) == QAccessible::State();
        });
        assert!(correct, "Qt states must preserve Slint properties, role focusability and real change masks");
    }

    #[test]
    fn announcements_require_changed_visible_nonempty_declared_live_text() {
        let correct = cpp!(unsafe [] -> bool as "bool" {
            return slint_announcement_kind("polite", true, "Checking", "Ready") == 1
                && slint_announcement_kind("assertive", true, "Checking", "Error") == 2
                && slint_announcement_kind("off", true, "Checking", "Ready") == 0
                && slint_announcement_kind("", true, "Checking", "Ready") == 0
                && slint_announcement_kind("polite", false, "Checking", "Ready") == 0
                && slint_announcement_kind("polite", true, "Ready", "Ready") == 0
                && slint_announcement_kind("polite", true, "Ready", "") == 0;
        });
        assert!(correct, "Qt announcements must honor explicit liveness without hidden, duplicate or empty messages");
    }
}
