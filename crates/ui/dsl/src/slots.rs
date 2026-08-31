//! P5 binding-level retained slots.
//!
//! A slot is intentionally separate from [`crate::UiNode`]: immutable retained trees keep
//! structure while the host-owned presentation value is updated through function-pointer
//! bindings. The API is small enough for macro output and is also usable by kit components that
//! need an explicit static table today.

use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use tela_contract::{
    ContentConcern, DirtyFlags, LayoutConcern, SemanticKey, UiNode, VisualConcern,
};
use tela_core::UiTree;

use crate::{Signal, SignalId};

/// One node's mutable candidate presentation state.
///
/// The composition tree remains an immutable shared [`UiNode`] graph. A binding owns this
/// smaller copy of the three presentation dimensions it may update, and the frame coordinator
/// projects it into a path-copied candidate node only after every changed slot has synchronized.
/// Identity, interaction and child structure are deliberately absent: a presentation binding
/// cannot turn into a structural or cross-component write channel.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePresentation {
    layout: Option<LayoutConcern>,
    visual: Option<VisualConcern>,
    content: Option<ContentConcern>,
}

impl NodePresentation {
    /// Captures the bindable presentation dimensions of one assembled node.
    pub fn from_node(node: &UiNode) -> Self {
        Self {
            layout: node.layout.clone(),
            visual: node.visual.clone(),
            content: node.content.clone(),
        }
    }

    /// Returns the layout presentation dimension, if the component assembled one.
    pub fn layout(&self) -> Option<&LayoutConcern> {
        self.layout.as_ref()
    }

    /// Returns the visual presentation dimension, if the component assembled one.
    pub fn visual(&self) -> Option<&VisualConcern> {
        self.visual.as_ref()
    }

    /// Returns the content presentation dimension, if the component assembled one.
    pub fn content(&self) -> Option<&ContentConcern> {
        self.content.as_ref()
    }

    /// Mutably accesses the layout presentation dimension.
    ///
    /// A slot declaring [`BindingSlotKind::Layout`] may use this to update text constraints,
    /// size or another geometry-affecting field.
    pub fn layout_mut(&mut self) -> Option<&mut LayoutConcern> {
        self.layout.as_mut()
    }

    /// Mutably accesses the visual presentation dimension.
    ///
    /// A slot declaring [`BindingSlotKind::Paint`] may use this for fill, opacity, color or
    /// another non-geometric appearance field.
    pub fn visual_mut(&mut self) -> Option<&mut VisualConcern> {
        self.visual.as_mut()
    }

    /// Mutably accesses the content presentation dimension.
    pub fn content_mut(&mut self) -> Option<&mut ContentConcern> {
        self.content.as_mut()
    }

    /// Projects this candidate presentation onto a fresh shell of `node`.
    ///
    /// Children, identity and input metadata remain the original shared values. Callers install
    /// the returned shell through `UiTree::splice_many_shared`; the active `Rc<UiNode>` is never
    /// mutated.
    pub fn apply_to(&self, node: &UiNode) -> UiNode {
        let mut projected = node.clone();
        projected.layout = self.layout.clone();
        projected.visual = self.visual.clone();
        projected.content = self.content.clone();
        projected
    }
}

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
    /// Installs one erased source subscription. The returned token unregisters on drop.
    ///
    /// This is an implementation hook for the candidate presentation runtime. Component authors
    /// normally declare a [`BindingSlot`] in a static table and never call it directly.
    #[doc(hidden)]
    fn subscribe(&self, component: &Component, callback: Rc<dyn Fn()>) -> Box<dyn Any>;
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

impl<Component, Value: Clone + 'static, Presentation> BindingSlotDyn<Component, Presentation>
    for BindingSlot<Component, Value, Presentation>
{
    fn signal_id(&self, component: &Component) -> SignalId {
        (self.source)(component).id()
    }

    fn version(&self, component: &Component) -> u64 {
        (self.source)(component).version()
    }

    fn subscribe(&self, component: &Component, callback: Rc<dyn Fn()>) -> Box<dyn Any> {
        (self.source)(component).subscribe_erased(callback)
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

/// Static metadata for conditionally selecting one of two binding tables.
///
/// The condition accessor and both table entries are function-pointer/static data only. A
/// component instantiates this definition with its own cloneable snapshot while assembling a
/// node it owns; the runtime never receives a closure, `NodeId`, writable Signal or another
/// component's presentation target.
pub struct StaticBindingSelector<Component: 'static, Presentation: 'static> {
    condition: fn(&Component) -> &Signal<bool>,
    when_true: &'static StaticBindingTable<Component, Presentation>,
    when_false: &'static StaticBindingTable<Component, Presentation>,
}

impl<Component: 'static, Presentation: 'static> StaticBindingSelector<Component, Presentation> {
    /// Creates one static condition-to-table wiring definition.
    pub const fn new(
        condition: fn(&Component) -> &Signal<bool>,
        when_true: &'static StaticBindingTable<Component, Presentation>,
        when_false: &'static StaticBindingTable<Component, Presentation>,
    ) -> Self {
        Self {
            condition,
            when_true,
            when_false,
        }
    }

    fn instantiate(&'static self, component: &Component) -> SlotSelector<Component, Presentation> {
        SlotSelector::new(
            (self.condition)(component).clone(),
            self.when_true,
            self.when_false,
        )
    }
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

impl<Component: 'static, Presentation: 'static> Clone for SlotGroup<Component, Presentation> {
    fn clone(&self) -> Self {
        Self {
            table: self.table,
            versions: self.versions.clone(),
        }
    }
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

    /// Records the current versions without writing a target.
    ///
    /// Initial component assembly already writes the source values into its newly created
    /// `UiNode`. Priming makes that assembled node and the retained binding group one atomic
    /// starting snapshot; later candidate frames write only sources whose versions changed.
    fn prime(&mut self, component: &Component) {
        self.versions = self
            .table
            .slots()
            .iter()
            .map(|slot| Some(slot.version(component)))
            .collect();
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

/// Type-erased candidate binding used by the frame runtime.
///
/// The concrete implementation is normally [`StaticNodeBinding`]. The interface intentionally
/// exposes only static-source subscription, version observation and writes into
/// [`NodePresentation`]; it has no route to component State, another component or the active UI
/// tree.
pub(crate) trait PresentationBinding: 'static {
    /// Clones the binding occurrence for a candidate transaction.
    #[doc(hidden)]
    fn clone_box(&self) -> Box<dyn PresentationBinding>;

    /// Captures the source versions represented by this occurrence without mutating presentation.
    #[doc(hidden)]
    fn prime(&mut self);

    /// Applies sources changed since the last prime/synchronization to candidate presentation.
    #[doc(hidden)]
    fn synchronize(&mut self, presentation: &mut NodePresentation) -> SlotDamage;

    /// Source identities and current versions, one entry per distinct Signal.
    #[doc(hidden)]
    fn source_versions(&self) -> Vec<(SignalId, u64)>;

    /// Installs one callback per distinct Signal source.
    #[doc(hidden)]
    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Vec<(SignalId, Box<dyn Any>)>;
}

/// A concrete static binding occurrence anchored to one component output root.
///
/// `Component` is an ordinary cloneable snapshot containing read-only [`Signal`] handles. The
/// table and its access/write functions are static function pointers, so no user closure or
/// runtime field-offset lookup is involved.
pub(crate) struct StaticNodeBinding<Component: Clone + 'static> {
    component: Component,
    group: SlotGroup<Component, NodePresentation>,
}

impl<Component: Clone + 'static> StaticNodeBinding<Component> {
    /// Creates one binding occurrence from a component snapshot and a static slot table.
    pub const fn new(
        component: Component,
        table: &'static StaticBindingTable<Component, NodePresentation>,
    ) -> Self {
        Self {
            component,
            group: SlotGroup::new(table),
        }
    }
}

impl<Component: Clone + 'static> Clone for StaticNodeBinding<Component> {
    fn clone(&self) -> Self {
        Self {
            component: self.component.clone(),
            group: self.group.clone(),
        }
    }
}

impl<Component: Clone + 'static> PresentationBinding for StaticNodeBinding<Component> {
    fn clone_box(&self) -> Box<dyn PresentationBinding> {
        Box::new(self.clone())
    }

    fn prime(&mut self) {
        self.group.prime(&self.component);
    }

    fn synchronize(&mut self, presentation: &mut NodePresentation) -> SlotDamage {
        self.group.synchronize(&self.component, presentation)
    }

    fn source_versions(&self) -> Vec<(SignalId, u64)> {
        let mut versions = BTreeMap::new();
        for slot in self.group.table.slots() {
            versions
                .entry(slot.signal_id(&self.component))
                .or_insert_with(|| slot.version(&self.component));
        }
        versions.into_iter().collect()
    }

    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Vec<(SignalId, Box<dyn Any>)> {
        let mut seen = BTreeSet::new();
        self.group
            .table
            .slots()
            .iter()
            .filter_map(|slot| {
                let signal = slot.signal_id(&self.component);
                seen.insert(signal).then(|| {
                    (
                        signal,
                        slot.subscribe(&self.component, Rc::clone(&callback)),
                    )
                })
            })
            .collect()
    }
}

/// Candidate-local instance of a static conditional binding selector.
///
/// It carries only a cloneable component snapshot and a selected static table group. The active
/// group is part of candidate state, which makes branch switches transactional: a rejected frame
/// leaves the old group and its subscriptions active.
pub(crate) struct StaticSelectorBinding<Component: Clone + 'static> {
    component: Component,
    selector: SlotSelector<Component, NodePresentation>,
}

impl<Component: Clone + 'static> StaticSelectorBinding<Component> {
    pub(crate) fn new(
        component: Component,
        definition: &'static StaticBindingSelector<Component, NodePresentation>,
    ) -> Self {
        let selector = definition.instantiate(&component);
        Self {
            component,
            selector,
        }
    }
}

impl<Component: Clone + 'static> Clone for StaticSelectorBinding<Component> {
    fn clone(&self) -> Self {
        Self {
            component: self.component.clone(),
            selector: self.selector.clone(),
        }
    }
}

impl<Component: Clone + 'static> PresentationBinding for StaticSelectorBinding<Component> {
    fn clone_box(&self) -> Box<dyn PresentationBinding> {
        Box::new(self.clone())
    }

    fn prime(&mut self) {
        self.selector.prime(&self.component);
    }

    fn synchronize(&mut self, presentation: &mut NodePresentation) -> SlotDamage {
        self.selector
            .synchronize(&self.component, presentation)
            .damage
    }

    fn source_versions(&self) -> Vec<(SignalId, u64)> {
        self.selector.source_versions(&self.component)
    }

    fn subscribe(&self, callback: Rc<dyn Fn()>) -> Vec<(SignalId, Box<dyn Any>)> {
        self.selector.subscribe(&self.component, callback)
    }
}

/// Candidate-local collection of static presentation bindings.
///
/// It owns both the last written presentation values and the per-occurrence source versions. The
/// active runtime keeps a separate instance; cloning this value is therefore the presentation
/// equivalent of cloning component owner State into a candidate transaction.
#[derive(Default)]
pub(crate) struct PresentationState {
    targets: BTreeMap<SemanticKey, PresentationTarget>,
}

struct PresentationTarget {
    presentation: NodePresentation,
    bindings: Vec<Box<dyn PresentationBinding>>,
}

impl Clone for PresentationTarget {
    fn clone(&self) -> Self {
        Self {
            presentation: self.presentation.clone(),
            bindings: self
                .bindings
                .iter()
                .map(|binding| binding.clone_box())
                .collect(),
        }
    }
}

impl Clone for PresentationState {
    fn clone(&self) -> Self {
        Self {
            targets: self
                .targets
                .iter()
                .map(|(key, target)| (key.clone(), target.clone()))
                .collect(),
        }
    }
}

/// Changed candidate presentations and their downstream invalidation class.
///
/// The frame coordinator applies these projections in deepest-first tree order. That order keeps
/// an already updated descendant when a later ancestor shell is path-copied, so two explicit
/// bindings may legally target nested nodes without changing structure or ownership.
pub(crate) struct PresentationUpdate {
    pub(crate) projections: Vec<(SemanticKey, NodePresentation)>,
    pub(crate) flags: DirtyFlags,
}

impl PresentationState {
    /// Installs bindings produced by a freshly assembled root and records their initial versions.
    ///
    /// Assembly itself is responsible for writing those initial source values into `tree`.
    /// Priming intentionally does not write a second time: it establishes one coherent starting
    /// snapshot without cloning the just-built tree or changing identity allocation.
    pub(crate) fn from_root(
        bindings: Vec<crate::view::ResolvedPresentationBinding>,
        tree: &UiTree,
    ) -> Self {
        let mut state = Self::default();
        state.install_resolved(bindings, tree);
        state
    }

    /// Clones bindings outside independently re-entered retained roots.
    ///
    /// Re-entry path-copies only the selected roots. Bindings below those roots carry component
    /// snapshots from the old assembly and must be replaced by the new output's resolved plans;
    /// bindings in untouched shared subtrees retain both their presentation values and source
    /// versions. Tree paths are only candidate-local traversal aids here: callers still select
    /// roots by stable [`SemanticKey`].
    pub(crate) fn retain_outside_roots(
        &self,
        active_tree: &UiTree,
        roots: &[SemanticKey],
    ) -> Option<Self> {
        let root_paths = roots
            .iter()
            .map(|root| active_tree.path_for_key(root))
            .collect::<Option<Vec<_>>>()?;
        let mut targets = BTreeMap::new();
        for (key, target) in &self.targets {
            let path = active_tree.path_for_key(key)?;
            if root_paths.iter().any(|root| path.starts_with(root)) {
                continue;
            }
            targets.insert(key.clone(), target.clone());
        }
        Some(Self { targets })
    }

    /// Installs bindings produced by one newly assembled/re-entered output root.
    ///
    /// The caller has already formed `tree`, so every anchor has a stable coordinate. Initial
    /// assembly wrote the corresponding source values into that candidate node; priming records
    /// only the source versions and avoids an active-tree write.
    pub(crate) fn install_resolved(
        &mut self,
        bindings: Vec<crate::view::ResolvedPresentationBinding>,
        tree: &UiTree,
    ) {
        self.install_resolved_with_dirty(bindings, tree, &BTreeSet::new());
    }

    /// Installs bindings from a retained re-entry while forcing selected targets to synchronize.
    ///
    /// A parent retained entry may restore one of its materialized child nodes instead of
    /// reconstructing it. If a static binding below that restored child is dirty in the same
    /// candidate, priming it would record the new source version against the old presentation
    /// value. Keeping that binding unprimed makes the ordinary candidate synchronization write
    /// the current value before publication. A missing selected key is still rejected by
    /// [`Self::synchronize_dirty`], which sends a structural change through rooted assembly.
    pub(crate) fn install_resolved_with_dirty(
        &mut self,
        bindings: Vec<crate::view::ResolvedPresentationBinding>,
        tree: &UiTree,
        dirty: &BTreeSet<SemanticKey>,
    ) {
        for mut binding in bindings {
            let node = tree
                .shared_node_for_key(&binding.key)
                .expect("resolved presentation binding key belongs to its candidate tree");
            if !dirty.contains(&binding.key) {
                binding.binding.prime();
            }
            self.targets
                .entry(binding.key)
                .or_insert_with(|| PresentationTarget {
                    presentation: NodePresentation::from_node(&node),
                    bindings: Vec::new(),
                })
                .bindings
                .push(binding.binding);
        }
    }

    /// Returns whether this state can fully own a dirty set without rerunning component assembly.
    ///
    /// A coordinate shared with an ordinary `#[watch]` is intentionally not eligible: that watch
    /// may affect arbitrary component logic, so the rooted/retained assembly path remains the
    /// sound owner of the update.
    pub(crate) fn owns_dirty(
        &self,
        dirty: &BTreeSet<SemanticKey>,
        ordinary_watch_keys: &BTreeSet<SemanticKey>,
    ) -> bool {
        !dirty.is_empty()
            && dirty
                .iter()
                .all(|key| self.targets.contains_key(key) && !ordinary_watch_keys.contains(key))
    }

    /// Synchronizes selected bindings into this candidate state.
    ///
    /// This intentionally returns presentation snapshots rather than pre-built node replacements:
    /// the coordinator is the only layer that owns candidate-tree path copying, and can preserve
    /// a dirty descendant while it subsequently projects a dirty ancestor shell.
    pub(crate) fn synchronize_dirty(
        &mut self,
        dirty: &BTreeSet<SemanticKey>,
    ) -> Option<PresentationUpdate> {
        let mut projections = Vec::with_capacity(dirty.len());
        let mut flags = DirtyFlags::EMPTY;
        for key in dirty {
            let target = self.targets.get_mut(key)?;
            let mut damage = SlotDamage::default();
            for binding in &mut target.bindings {
                let slot_damage = binding.synchronize(&mut target.presentation);
                damage.paint |= slot_damage.paint;
                damage.layout |= slot_damage.layout;
            }
            if damage.paint {
                flags |= DirtyFlags::VISUAL;
            }
            if damage.layout {
                flags |= DirtyFlags::GEOMETRY;
            }
            if !damage.paint && !damage.layout {
                continue;
            }
            projections.push((key.clone(), target.presentation.clone()));
        }
        Some(PresentationUpdate { projections, flags })
    }

    /// Current source versions indexed by stable Signal identity.
    pub(crate) fn source_versions(&self) -> BTreeMap<SignalId, u64> {
        let mut versions = BTreeMap::new();
        for target in self.targets.values() {
            for binding in &target.bindings {
                for (source, version) in binding.source_versions() {
                    versions.entry(source).or_insert(version);
                }
            }
        }
        versions
    }

    /// Target coordinates affected by each source identity.
    pub(crate) fn source_keys(&self) -> BTreeMap<SignalId, BTreeSet<SemanticKey>> {
        let mut keys = BTreeMap::<SignalId, BTreeSet<SemanticKey>>::new();
        for (key, target) in &self.targets {
            for binding in &target.bindings {
                for (source, _) in binding.source_versions() {
                    keys.entry(source).or_default().insert(key.clone());
                }
            }
        }
        keys
    }
}

/// Active presentation binding runtime.
///
/// Subscriptions belong here rather than in `UiNode` or the host. They are replaced only after a
/// candidate has successfully presented, so an abandoned `Show` branch or rejected frame cannot
/// leave a binding capable of waking the application.
#[derive(Default)]
pub(crate) struct PresentationRuntime {
    active: PresentationState,
    subscriptions: BTreeMap<SemanticKey, BTreeMap<SignalId, Box<dyn Any>>>,
}

impl PresentationRuntime {
    /// Starts a candidate that reuses every currently committed presentation binding.
    ///
    /// Host-only projection changes (for example a scroll offset or focus ring) still need a
    /// normal candidate/present transaction, but they do not give the host permission to mutate
    /// the active tree or rebuild application structure. The coordinator uses this snapshot when
    /// it reuses the active shared tree for such a candidate.
    pub(crate) fn candidate_active(&self) -> PresentationState {
        self.active.clone()
    }

    /// Returns whether a committed static presentation binding owns this exact coordinate.
    ///
    /// The caller still has to establish that no ordinary watch owns the same coordinate and
    /// that any selected retained replacement is structurally disjoint. A binding occurrence
    /// alone is never permission to skip arbitrary component assembly.
    pub(crate) fn owns_key(&self, key: &SemanticKey) -> bool {
        self.active.targets.contains_key(key)
    }

    /// Starts a retained re-entry candidate from bindings outside the selected roots.
    ///
    /// The runtime keeps active presentation values and subscription tokens private; callers get
    /// only a cloneable candidate state that still must pass the normal `presented(token)` commit
    /// boundary before it can replace those active records.
    pub(crate) fn candidate_outside_roots(
        &self,
        active_tree: &UiTree,
        roots: &[SemanticKey],
    ) -> Option<PresentationState> {
        self.active.retain_outside_roots(active_tree, roots)
    }

    pub(crate) fn candidate_for_dirty(
        &self,
        dirty: &BTreeSet<SemanticKey>,
        ordinary_watch_keys: &BTreeSet<SemanticKey>,
    ) -> Option<PresentationState> {
        self.active
            .owns_dirty(dirty, ordinary_watch_keys)
            .then(|| self.active.clone())
    }

    pub(crate) fn commit(
        &mut self,
        candidate: PresentationState,
        runtime: &crate::runtime::ComponentRuntime,
    ) {
        let mut subscriptions = BTreeMap::<SemanticKey, BTreeMap<SignalId, Box<dyn Any>>>::new();
        for (key, target) in &candidate.targets {
            let callback = runtime.dirty_callback(key.clone());
            let target_subscriptions = subscriptions.entry(key.clone()).or_default();
            for binding in &target.bindings {
                for (source, subscription) in binding.subscribe(Rc::clone(&callback)) {
                    target_subscriptions.entry(source).or_insert(subscription);
                }
            }
        }
        // Drop the old tokens only after new subscriptions have been built. A synchronous source
        // write during this swap can therefore at worst mark the same stable key dirty; it cannot
        // be lost between an unsubscribe and subscribe gap.
        self.subscriptions = subscriptions;
        self.active = candidate;
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

impl<Component: 'static, Presentation: 'static> Clone for SlotSelector<Component, Presentation> {
    fn clone(&self) -> Self {
        Self {
            condition: self.condition.clone(),
            when_true: self.when_true,
            when_false: self.when_false,
            active: self.active,
            group: self.group.clone(),
        }
    }
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

    /// Records the currently selected branch versions without writing a presentation target.
    ///
    /// Initial assembly already produced the matching node values. Priming preserves that node
    /// allocation and gives the selector the same atomic starting point as [`SlotGroup::prime`].
    pub fn prime(&mut self, component: &Component) {
        let selected = self.condition.get();
        if self.active != Some(selected) {
            self.group = None;
            let table = if selected {
                self.when_true
            } else {
                self.when_false
            };
            self.group = Some(SlotGroup::new(table));
            self.active = Some(selected);
        }
        self.group
            .as_mut()
            .expect("selector installs a group before priming")
            .prime(component);
    }

    /// The condition plus sources in the selected branch, for candidate version validation.
    pub fn source_versions(&self, component: &Component) -> Vec<(SignalId, u64)> {
        let mut versions = BTreeMap::new();
        versions.insert(self.condition.id(), self.condition.version());
        let selected = self.active.unwrap_or_else(|| self.condition.get());
        let table = if selected {
            self.when_true
        } else {
            self.when_false
        };
        for slot in table.slots() {
            versions
                .entry(slot.signal_id(component))
                .or_insert_with(|| slot.version(component));
        }
        versions.into_iter().collect()
    }

    /// Installs subscriptions for the condition and sources in the selected branch only.
    pub fn subscribe(
        &self,
        component: &Component,
        callback: Rc<dyn Fn()>,
    ) -> Vec<(SignalId, Box<dyn Any>)> {
        let mut subscriptions = Vec::new();
        let mut seen = BTreeSet::new();
        let condition = self.condition.id();
        seen.insert(condition);
        subscriptions.push((
            condition,
            self.condition.subscribe_erased(Rc::clone(&callback)),
        ));
        let selected = self.active.unwrap_or_else(|| self.condition.get());
        let table = if selected {
            self.when_true
        } else {
            self.when_false
        };
        for slot in table.slots() {
            let source = slot.signal_id(component);
            if seen.insert(source) {
                subscriptions.push((source, slot.subscribe(component, Rc::clone(&callback))));
            }
        }
        subscriptions
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
    use crate::signal;

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
        let (fill_writer, fill) = signal(2);
        let (_text_writer, text) = signal("one".to_owned());
        let component = Component { fill, text };
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

        fill_writer.set(4);
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
        let (_fill_writer, fill) = signal(7);
        let (_text_writer, text) = signal("visible".to_owned());
        let (condition_writer, condition) = signal(false);
        let component = Component { fill, text };
        let mut presentation = Presentation::default();
        let mut selector = SlotSelector::new(condition.clone(), &ALL, &EMPTY);

        let first = selector.synchronize(&component, &mut presentation);
        assert!(first.switched);
        assert!(!first.damage.paint && !first.damage.layout);
        condition_writer.set(true);
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
