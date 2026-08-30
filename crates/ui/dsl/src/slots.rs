//! P5 binding-level retained slots.
//!
//! A slot is intentionally separate from [`crate::UiNode`]: immutable retained trees keep
//! structure while the host-owned presentation value is updated through function-pointer
//! bindings. The API is small enough for macro output and is also usable by kit components that
//! need an explicit static table today.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Signal, SignalId};

/// The downstream work caused by writing a slot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlotDamage {
    /// A paint-only presentation value changed.
    pub paint: bool,
    /// A geometry-affecting presentation value changed.
    pub layout: bool,
}

impl SlotDamage {
    fn include(&mut self, kind: BindingSlotKind) {
        match kind {
            BindingSlotKind::Paint => self.paint = true,
            BindingSlotKind::Layout => self.layout = true,
        }
    }
}

/// Whether a slot writes only presentation or invalidates layout geometry too.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingSlotKind {
    /// Fill, opacity, transform offset and similar presentation-only values.
    Paint,
    /// Text, size and every value that can change the layout box.
    Layout,
}

/// Object-safe part of a heterogeneous static binding table.
///
/// Applications normally construct this through [`BindingSlot`] and never need to implement it
/// manually. It is public solely because a `static` table may contain different signal value
/// types while sharing one component and presentation state type.
#[doc(hidden)]
pub trait BindingSlotDyn<Component, Presentation>: Sync {
    /// Signal identity used for per-slot version tracking.
    fn signal_id(&self, component: &Component) -> SignalId;
    /// Current source write version.
    fn version(&self, component: &Component) -> u64;
    /// Applies the current source value to host-owned presentation state.
    fn apply(&self, component: &Component, presentation: &mut Presentation);
    /// Downstream invalidation class.
    fn kind(&self) -> BindingSlotKind;
}

/// One statically-wired signal-to-presentation binding.
///
/// Both access and write are plain function pointers. A generated `static BindingSlot` therefore
/// contains no captured component value and requires neither unsafe field offsets nor a runtime
/// lookup of the source field.
pub struct BindingSlot<Component, Value, Presentation> {
    source: fn(&Component) -> &Signal<Value>,
    apply: fn(&Value, &mut Presentation),
    kind: BindingSlotKind,
}

impl<Component, Value, Presentation> BindingSlot<Component, Value, Presentation> {
    /// Creates a paint-only static binding.
    pub const fn paint(
        source: fn(&Component) -> &Signal<Value>,
        apply: fn(&Value, &mut Presentation),
    ) -> Self {
        Self {
            source,
            apply,
            kind: BindingSlotKind::Paint,
        }
    }

    /// Creates a geometry-affecting static binding.
    pub const fn layout(
        source: fn(&Component) -> &Signal<Value>,
        apply: fn(&Value, &mut Presentation),
    ) -> Self {
        Self {
            source,
            apply,
            kind: BindingSlotKind::Layout,
        }
    }
}

impl<Component, Value: Clone, Presentation> BindingSlotDyn<Component, Presentation>
    for BindingSlot<Component, Value, Presentation>
{
    fn signal_id(&self, component: &Component) -> SignalId {
        (self.source)(component).id()
    }

    fn version(&self, component: &Component) -> u64 {
        (self.source)(component).version()
    }

    fn apply(&self, component: &Component, presentation: &mut Presentation) {
        let signal = (self.source)(component);
        let value = signal.get();
        (self.apply)(&value, presentation);
    }

    fn kind(&self) -> BindingSlotKind {
        self.kind
    }
}

/// A read-only static table emitted for one component template.
pub struct StaticBindingTable<Component: 'static, Presentation: 'static> {
    slots: &'static [&'static dyn BindingSlotDyn<Component, Presentation>],
}

impl<Component: 'static, Presentation: 'static> StaticBindingTable<Component, Presentation> {
    /// Creates a table from static slot references.
    pub const fn new(
        slots: &'static [&'static dyn BindingSlotDyn<Component, Presentation>],
    ) -> Self {
        Self { slots }
    }

    /// Returns the immutable static slot entries.
    pub const fn slots(&self) -> &'static [&'static dyn BindingSlotDyn<Component, Presentation>] {
        self.slots
    }
}

/// Runtime instance of one static binding table.
///
/// The instance owns only source versions. The mutable target belongs to the presentation layer,
/// so synchronizing slots never mutates an `Rc<UiNode>` retained by the composition tree.
pub struct SlotGroup<Component: 'static, Presentation: 'static> {
    table: &'static StaticBindingTable<Component, Presentation>,
    // Version state belongs to a binding occurrence, not just SignalId: one source may drive
    // several independent presentation targets in the same static table.
    versions: Vec<Option<u64>>,
}

impl<Component: 'static, Presentation: 'static> SlotGroup<Component, Presentation> {
    /// Instantiates an initially-unapplied static group.
    pub const fn new(table: &'static StaticBindingTable<Component, Presentation>) -> Self {
        Self {
            table,
            versions: Vec::new(),
        }
    }

    /// Applies only slots whose source version changed since this group was installed.
    pub fn synchronize(
        &mut self,
        component: &Component,
        presentation: &mut Presentation,
    ) -> SlotDamage {
        let mut damage = SlotDamage::default();
        for (index, slot) in self.table.slots().iter().enumerate() {
            let version = slot.version(component);
            if self.versions.get(index).copied().flatten() == Some(version) {
                continue;
            }
            slot.apply(component, presentation);
            if self.versions.len() == index {
                self.versions.push(Some(version));
            } else {
                self.versions[index] = Some(version);
            }
            damage.include(slot.kind());
        }
        damage
    }

    /// Signal identities currently instantiated by this group.
    pub fn signal_ids(&self, component: &Component) -> BTreeSet<SignalId> {
        self.table
            .slots()
            .iter()
            .map(|slot| slot.signal_id(component))
            .collect()
    }
}

/// Result of synchronizing a conditional slot selector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlotSelection {
    /// A condition change unregistered the old group and instantiated the other group.
    pub switched: bool,
    /// Downstream invalidation caused by the active group.
    pub damage: SlotDamage,
}

/// Static `if` slot wiring with transactional group replacement.
///
/// The condition is itself a Signal edge. On a condition version change the old `SlotGroup` is
/// dropped before the selected group is installed; the call is synchronous and no mixed branch
/// subscription state is observable to the single-threaded composition runtime.
pub struct SlotSelector<Component: 'static, Presentation: 'static> {
    condition: Signal<bool>,
    when_true: &'static StaticBindingTable<Component, Presentation>,
    when_false: &'static StaticBindingTable<Component, Presentation>,
    active: Option<bool>,
    group: Option<SlotGroup<Component, Presentation>>,
}

impl<Component: 'static, Presentation: 'static> SlotSelector<Component, Presentation> {
    /// Creates an uninstantiated selector.
    pub fn new(
        condition: Signal<bool>,
        when_true: &'static StaticBindingTable<Component, Presentation>,
        when_false: &'static StaticBindingTable<Component, Presentation>,
    ) -> Self {
        Self {
            condition,
            when_true,
            when_false,
            active: None,
            group: None,
        }
    }

    /// Reconciles the selected group and writes its changed slots.
    pub fn synchronize(
        &mut self,
        component: &Component,
        presentation: &mut Presentation,
    ) -> SlotSelection {
        let selected = self.condition.get();
        let switched = self.active != Some(selected);
        if switched {
            self.group = None;
            let table = if selected {
                self.when_true
            } else {
                self.when_false
            };
            self.group = Some(SlotGroup::new(table));
            self.active = Some(selected);
        }
        let damage = self
            .group
            .as_mut()
            .expect("selector installs a group before synchronizing")
            .synchronize(component, presentation);
        SlotSelection { switched, damage }
    }

    /// Current branch, if this selector has been synchronized at least once.
    pub fn active_branch(&self) -> Option<bool> {
        self.active
    }

    /// Stable graph identity of the condition edge.
    pub fn condition_id(&self) -> SignalId {
        self.condition.id()
    }
}

/// A key collision encountered while reconciling one list pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DuplicateListKey(pub u64);

impl std::fmt::Display for DuplicateListKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "duplicate ListFactory key {}", self.0)
    }
}

impl std::error::Error for DuplicateListKey {}

/// Reconciliation result for a keyed static row template.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListReconcile {
    /// Rows instantiated for keys not present in the previous pass.
    pub created: Vec<u64>,
    /// Rows removed because their keys disappeared.
    pub removed: Vec<u64>,
    /// Existing rows retained by exact `u64` key identity.
    pub retained: Vec<u64>,
    /// New visual row order.
    pub order: Vec<u64>,
}

/// Keyed runtime factory for a static row template.
///
/// `key` and `template` are function pointers, so a macro can put the row wiring in static code.
/// The runtime performs no value comparison: it retains rows solely by `u64` identity and drops
/// removed rows, which is the explicit slot-group unregister operation.
pub struct ListFactory<Item, Row> {
    key: fn(&Item) -> u64,
    template: fn(&Item) -> Row,
    rows: BTreeMap<u64, Row>,
    order: Vec<u64>,
}

impl<Item, Row> ListFactory<Item, Row> {
    /// Creates an empty keyed factory from static key and row-template functions.
    pub const fn new(key: fn(&Item) -> u64, template: fn(&Item) -> Row) -> Self {
        Self {
            key,
            template,
            rows: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    /// Reconciles rows by exact key identity, preserving retained row values in place.
    pub fn reconcile<'item>(
        &mut self,
        items: impl IntoIterator<Item = &'item Item>,
    ) -> Result<ListReconcile, DuplicateListKey>
    where
        Item: 'item,
    {
        let previous = std::mem::take(&mut self.rows);
        let mut previous = previous;
        let mut next = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut result = ListReconcile::default();

        for item in items {
            let key = (self.key)(item);
            if !seen.insert(key) {
                previous.extend(next);
                self.rows = previous;
                return Err(DuplicateListKey(key));
            }
            if let Some(row) = previous.remove(&key) {
                result.retained.push(key);
                next.insert(key, row);
            } else {
                result.created.push(key);
                next.insert(key, (self.template)(item));
            }
            result.order.push(key);
        }

        result.removed = previous.into_keys().collect();
        self.rows = next;
        self.order = result.order.clone();
        Ok(result)
    }

    /// Looks up one live row by its stable key.
    pub fn row(&self, key: u64) -> Option<&Row> {
        self.rows.get(&key)
    }

    /// Current retained keys in visual order.
    pub fn order(&self) -> &[u64] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Component {
        fill: Signal<u32>,
        text: Signal<String>,
    }

    #[derive(Default)]
    struct Presentation {
        fill: u32,
        outline: u32,
        text: String,
    }

    fn fill(component: &Component) -> &Signal<u32> {
        &component.fill
    }

    fn text(component: &Component) -> &Signal<String> {
        &component.text
    }

    fn write_fill(value: &u32, presentation: &mut Presentation) {
        presentation.fill = *value;
    }

    fn write_outline(value: &u32, presentation: &mut Presentation) {
        presentation.outline = *value;
    }

    #[allow(clippy::ptr_arg)] // BindingSlot's source value is Signal<String>, so the fn pointer is exact.
    fn write_text(value: &String, presentation: &mut Presentation) {
        presentation.text = value.clone();
    }

    static FILL: BindingSlot<Component, u32, Presentation> = BindingSlot::paint(fill, write_fill);
    static OUTLINE: BindingSlot<Component, u32, Presentation> =
        BindingSlot::paint(fill, write_outline);
    static TEXT: BindingSlot<Component, String, Presentation> =
        BindingSlot::layout(text, write_text);
    static ALL_SLOTS: [&dyn BindingSlotDyn<Component, Presentation>; 3] = [&FILL, &OUTLINE, &TEXT];
    static ALL: StaticBindingTable<Component, Presentation> = StaticBindingTable::new(&ALL_SLOTS);
    static EMPTY_SLOTS: [&dyn BindingSlotDyn<Component, Presentation>; 0] = [];
    static EMPTY: StaticBindingTable<Component, Presentation> =
        StaticBindingTable::new(&EMPTY_SLOTS);

    #[test]
    fn static_slots_apply_only_changed_versions_and_classify_damage() {
        let component = Component {
            fill: Signal::new(2),
            text: Signal::new("one".to_owned()),
        };
        let mut presentation = Presentation::default();
        let mut group = SlotGroup::new(&ALL);

        assert_eq!(
            group.synchronize(&component, &mut presentation),
            SlotDamage {
                paint: true,
                layout: true
            }
        );
        assert_eq!(presentation.fill, 2);
        assert_eq!(presentation.outline, 2, "one source can drive two slots");
        assert_eq!(presentation.text, "one");
        assert_eq!(
            group.synchronize(&component, &mut presentation),
            SlotDamage::default()
        );

        component.fill.set(4);
        assert_eq!(
            group.synchronize(&component, &mut presentation),
            SlotDamage {
                paint: true,
                layout: false
            }
        );
        assert_eq!(presentation.fill, 4);
        assert_eq!(presentation.outline, 4);
    }

    #[test]
    fn selector_replaces_slot_group_when_condition_changes() {
        let component = Component {
            fill: Signal::new(7),
            text: Signal::new("visible".to_owned()),
        };
        let condition = Signal::new(false);
        let mut presentation = Presentation::default();
        let mut selector = SlotSelector::new(condition.clone(), &ALL, &EMPTY);

        let first = selector.synchronize(&component, &mut presentation);
        assert!(first.switched);
        assert!(!first.damage.paint && !first.damage.layout);
        condition.set(true);
        let second = selector.synchronize(&component, &mut presentation);
        assert!(second.switched);
        assert_eq!(
            second.damage,
            SlotDamage {
                paint: true,
                layout: true
            }
        );
        assert_eq!(presentation.text, "visible");
    }

    #[test]
    fn list_factory_reconciles_by_key_without_value_comparisons() {
        #[derive(Clone)]
        struct Item {
            id: u64,
            label: &'static str,
        }
        fn key(item: &Item) -> u64 {
            item.id
        }
        fn row(item: &Item) -> String {
            item.label.to_owned()
        }

        let first = vec![
            Item {
                id: 2,
                label: "two",
            },
            Item {
                id: 1,
                label: "one",
            },
        ];
        let second = vec![
            Item {
                id: 1,
                label: "changed",
            },
            Item {
                id: 3,
                label: "three",
            },
        ];
        let mut factory = ListFactory::new(key, row);
        assert_eq!(
            factory.reconcile(&first).expect("first list").created,
            vec![2, 1]
        );
        let pass = factory.reconcile(&second).expect("second list");
        assert_eq!(pass.retained, vec![1]);
        assert_eq!(pass.created, vec![3]);
        assert_eq!(pass.removed, vec![2]);
        assert_eq!(factory.order(), &[1, 3]);
        assert_eq!(factory.row(1), Some(&"one".to_owned()));

        let duplicate = vec![
            Item {
                id: 1,
                label: "one",
            },
            Item {
                id: 1,
                label: "again",
            },
        ];
        assert_eq!(factory.reconcile(&duplicate), Err(DuplicateListKey(1)));
        assert_eq!(
            factory.order(),
            &[1, 3],
            "failed pass keeps the active rows intact"
        );
        assert_eq!(factory.row(1), Some(&"one".to_owned()));
    }
}
