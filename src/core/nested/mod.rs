use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::cell::Cell;
#[cfg(feature = "devtools")]
use std::cell::RefCell;
use std::sync::Arc;

use crate::callback::{CommandTx, Dispatcher, ScopeId};
use crate::core::component::{KeyUpdate, Update};
use crate::core::element::MeasureCacheEntry;
use crate::core::element::{Element, ElementKind, Group, Key};
use crate::core::event::KeyEvent;
use crate::core::runtime_env::RuntimeEnv;
use crate::style::{LayoutConstraints, Rect};
use crate::utils::arena::Arena;
use crate::utils::diff::reuse_plan;

pub mod any_props;
pub mod element;
pub mod erased;
pub mod host;

pub(crate) use element::ComponentElement;
pub(crate) use erased::{ComponentMount, EmptyComponent, ErasedComponent};
pub(crate) use host::{ComponentId, HostState};

use host::{ContainerPath, ContainerTag, MemoCacheEntry, PathSegment, SegmentId, segment_id};

/// Trim a `std::any::type_name` string to a short display form.
///
/// Strips module paths while preserving generic arguments, e.g.
/// `app::Panel<alloc::string::String>` → `Panel<String>`.
pub(crate) fn short_type_name(full: &str) -> String {
    let mut out = String::with_capacity(full.len());
    let mut pending = String::new();
    for ch in full.chars() {
        match ch {
            ':' => {
                pending.clear();
            }
            '<' | '>' | ',' | ' ' | '&' | '(' | ')' | '[' | ']' => {
                out.push_str(&pending);
                pending.clear();
                out.push(ch);
            }
            _ => pending.push(ch),
        }
    }
    out.push_str(&pending);
    out
}

#[cfg(feature = "devtools")]
thread_local! {
    static MEMO_HIT_COUNT: Cell<u32> = const { Cell::new(0) };
    static MEMO_MISS_COUNT: Cell<u32> = const { Cell::new(0) };
    static MEMO_MISS_REASONS: RefCell<Vec<(MemoMissReason, u32)>> = const { RefCell::new(Vec::new()) };
    static VIEW_TIMING_ENABLED: Cell<bool> = const { Cell::new(false) };
    static VIEW_TIMINGS: RefCell<Vec<ViewTimingSample>> = const { RefCell::new(Vec::new()) };
}

/// One exclusive `view()` sample collected during a frame.
#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ViewTimingSample {
    pub scope: ScopeId,
    pub name: Arc<str>,
    pub duration: std::time::Duration,
}

#[cfg(feature = "devtools")]
const VIEW_TIMING_CAP: usize = 512;

#[cfg(feature = "devtools")]
pub(crate) fn set_view_timing_enabled(enabled: bool) {
    VIEW_TIMING_ENABLED.with(|flag| flag.set(enabled));
}

#[cfg(feature = "devtools")]
pub(crate) fn view_timing_enabled() -> bool {
    VIEW_TIMING_ENABLED.with(|flag| flag.get())
}

#[cfg(feature = "devtools")]
pub(crate) fn record_view_timing(scope: ScopeId, name: Arc<str>, duration: std::time::Duration) {
    if !view_timing_enabled() {
        return;
    }
    VIEW_TIMINGS.with(|timings| {
        let mut timings = timings.borrow_mut();
        if timings.len() >= VIEW_TIMING_CAP {
            return;
        }
        timings.push(ViewTimingSample {
            scope,
            name,
            duration,
        });
    });
}

#[cfg(feature = "devtools")]
pub(crate) fn take_view_timings() -> Vec<ViewTimingSample> {
    VIEW_TIMINGS.with(|timings| std::mem::take(&mut *timings.borrow_mut()))
}

/// Aggregated exclusive `view()` time for one component scope in a frame.
#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AggregatedViewTiming {
    pub name: Arc<str>,
    pub scope: ScopeId,
    pub duration: std::time::Duration,
    pub calls: u32,
}

/// Aggregate per-scope view samples into top-N timings by total duration.
#[cfg(feature = "devtools")]
pub(crate) fn aggregate_view_timings(
    samples: Vec<ViewTimingSample>,
    limit: usize,
) -> Vec<AggregatedViewTiming> {
    use rustc_hash::FxHashMap;
    let mut by_scope: FxHashMap<ScopeId, (Arc<str>, std::time::Duration, u32)> =
        FxHashMap::default();
    for sample in samples {
        let entry = by_scope
            .entry(sample.scope)
            .or_insert_with(|| (Arc::clone(&sample.name), std::time::Duration::ZERO, 0));
        entry.1 = entry.1.saturating_add(sample.duration);
        entry.2 = entry.2.saturating_add(1);
    }
    let mut out: Vec<_> = by_scope
        .into_iter()
        .map(|(scope, (name, duration, calls))| AggregatedViewTiming {
            name,
            scope,
            duration,
            calls,
        })
        .collect();
    out.sort_by(|a, b| {
        b.duration
            .cmp(&a.duration)
            .then_with(|| a.name.cmp(&b.name))
    });
    if out.len() > limit {
        out.truncate(limit);
    }
    out
}

/// Why a component or in-view `Memo` failed to retain its cached subtree.
#[cfg(feature = "devtools")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MemoMissReason {
    NotMemoized,
    NoCache,
    KeyChanged,
    SelfDirty,
    DependencyChanged(MemoDependencyKind),
    RetainedChildRefreshFailed,
    ViewMemoNoCache,
    ViewMemoDepsChanged,
    ViewMemoStructureChanged,
}

/// Which memo dependency drifted when a retain check failed.
#[cfg(feature = "devtools")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MemoDependencyKind {
    Theme,
    Focus,
    Hover,
    Scroll,
    MouseCapture,
    Viewport,
    Transition,
    HostTerminalColors,
    Context(&'static str),
}

/// Per-frame memo counters drained into DevTools metrics.
#[cfg(feature = "devtools")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoFrameStats {
    pub hits: u32,
    pub misses: u32,
    pub reasons: Vec<(MemoMissReason, u32)>,
}

#[cfg(feature = "devtools")]
fn increment_memo_hit_counter() {
    MEMO_HIT_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
}

#[cfg(feature = "devtools")]
fn record_memo_miss(reason: MemoMissReason) {
    MEMO_MISS_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
    MEMO_MISS_REASONS.with(|reasons| {
        let mut reasons = reasons.borrow_mut();
        if let Some((_, count)) = reasons.iter_mut().find(|(r, _)| *r == reason) {
            *count = count.saturating_add(1);
        } else {
            reasons.push((reason, 1));
        }
    });
}

#[cfg(feature = "devtools")]
pub(crate) fn take_memo_frame_stats() -> MemoFrameStats {
    let hits = MEMO_HIT_COUNT.with(|count| {
        let current = count.get();
        count.set(0);
        current
    });
    let misses = MEMO_MISS_COUNT.with(|count| {
        let current = count.get();
        count.set(0);
        current
    });
    let mut reasons = MEMO_MISS_REASONS.with(|reasons| std::mem::take(&mut *reasons.borrow_mut()));
    reasons.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| format!("{a:?}").cmp(&format!("{b:?}")))
    });
    if reasons.len() > 4 {
        reasons.truncate(4);
    }
    MemoFrameStats {
        hits,
        misses,
        reasons,
    }
}

#[cfg(feature = "devtools")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemoRetainFacts {
    has_key: bool,
    has_cache: bool,
    self_dirty: bool,
    key_matches: bool,
}

#[cfg(feature = "devtools")]
fn classify_component_miss(
    facts: MemoRetainFacts,
    dep: Option<MemoDependencyKind>,
) -> MemoMissReason {
    if !facts.has_key {
        return MemoMissReason::NotMemoized;
    }
    if !facts.has_cache {
        return MemoMissReason::NoCache;
    }
    if facts.self_dirty {
        return MemoMissReason::SelfDirty;
    }
    if !facts.key_matches {
        return MemoMissReason::KeyChanged;
    }
    MemoMissReason::DependencyChanged(dep.unwrap_or(MemoDependencyKind::Theme))
}

/// Short overlay label for a memo miss reason.
#[cfg(feature = "devtools")]
pub(crate) fn memo_miss_reason_label(reason: MemoMissReason) -> String {
    match reason {
        MemoMissReason::NotMemoized => "no-memo".into(),
        MemoMissReason::NoCache => "no-cache".into(),
        MemoMissReason::KeyChanged => "key".into(),
        MemoMissReason::SelfDirty => "dirty".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::Theme) => "dep:theme".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::Focus) => "dep:focus".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::Hover) => "dep:hover".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::Scroll) => "dep:scroll".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::MouseCapture) => {
            "dep:mouse-capture".into()
        }
        MemoMissReason::DependencyChanged(MemoDependencyKind::Viewport) => "dep:viewport".into(),
        MemoMissReason::DependencyChanged(MemoDependencyKind::Transition) => {
            "dep:transition".into()
        }
        MemoMissReason::DependencyChanged(MemoDependencyKind::HostTerminalColors) => {
            "dep:host-colors".into()
        }
        MemoMissReason::DependencyChanged(MemoDependencyKind::Context(name)) => {
            format!("dep:ctx({})", short_type_name(name))
        }
        MemoMissReason::RetainedChildRefreshFailed => "child-refresh".into(),
        MemoMissReason::ViewMemoNoCache => "view-cache".into(),
        MemoMissReason::ViewMemoDepsChanged => "view-deps".into(),
        MemoMissReason::ViewMemoStructureChanged => "view-structure".into(),
    }
}

#[derive(Clone, Debug)]
struct ComponentSpec {
    key: Option<Key>,
    type_id: TypeId,
}

struct ComponentEntry {
    id: ComponentId,
    scope: ScopeId,
    type_id: TypeId,
    key: Option<Key>,
    state_key: Option<Key>,
    parent: Option<ComponentId>,
    host: HostState,
    epoch: u32,
    component: Box<dyn ErasedComponent>,
    /// Trimmed type name for DevTools overlays (`short_type_name`).
    display_name: Arc<str>,
    /// Full `type_name` for tracing spans.
    full_name: &'static str,
    initialized: bool,
    /// Theme from an ancestor `ThemeProvider`, recorded during expansion
    /// so that `refresh_scope_in_place` can re-apply it.
    active_theme: Option<crate::style::Theme>,
    /// Typed context values visible to this component from ancestor
    /// `ContextProvider` wrappers.
    active_contexts: FxHashMap<TypeId, Arc<dyn Any>>,
    active_context_generations: FxHashMap<TypeId, u64>,
    cached_element: Option<Element>,
    last_memo_key: Option<u64>,
    memo_deps: crate::core::runtime_env::MemoDependencySnapshot,
    self_dirty: bool,
    descendant_dirty: bool,
}

impl ComponentEntry {
    fn empty(id: ComponentId) -> Self {
        Self {
            id,
            scope: ScopeId(0),
            type_id: TypeId::of::<()>(),
            key: None,
            state_key: None,
            parent: None,
            host: HostState::default(),
            epoch: 0,
            component: Box::new(EmptyComponent),
            display_name: Arc::from("<mount-failed>"),
            full_name: "<mount-failed>",
            initialized: false,
            active_theme: None,
            active_contexts: FxHashMap::default(),
            active_context_generations: FxHashMap::default(),
            cached_element: None,
            last_memo_key: None,
            memo_deps: crate::core::runtime_env::MemoDependencySnapshot::default(),
            self_dirty: false,
            descendant_dirty: false,
        }
    }

    fn reset_for_reuse(&mut self, id: ComponentId) {
        self.id = id;
        self.state_key = None;
        self.epoch = 0;
        self.initialized = false;
        self.active_contexts.clear();
        self.active_context_generations.clear();
        self.cached_element = None;
        self.last_memo_key = None;
        self.memo_deps = crate::core::runtime_env::MemoDependencySnapshot::default();
        self.self_dirty = false;
        self.descendant_dirty = false;
        // display_name / full_name are kept across reuse — the next mount that
        // actually replaces the component will rewrite them.
    }

    fn reset_for_free(&mut self) {
        self.component.unmount();
        self.scope = ScopeId(0);
        self.type_id = TypeId::of::<()>();
        self.key = None;
        self.state_key = None;
        self.parent = None;
        self.host = HostState::default();
        self.epoch = 0;
        self.component = Box::new(EmptyComponent);
        self.display_name = Arc::from("<mount-failed>");
        self.full_name = "<mount-failed>";
        self.initialized = false;
        self.active_theme = None;
        self.active_contexts.clear();
        self.active_context_generations.clear();
        self.cached_element = None;
        self.last_memo_key = None;
        self.memo_deps = crate::core::runtime_env::MemoDependencySnapshot::default();
        self.self_dirty = false;
        self.descendant_dirty = false;
    }
}

/// Runtime registry for nested component instances.
pub(crate) struct ComponentRegistry {
    dispatcher: Dispatcher,
    command_tx: CommandTx,
    env: RuntimeEnv,
    arena: Arena<ComponentEntry, ComponentId>,
    scope_to_id: FxHashMap<ScopeId, ComponentId>,
    /// Path-independent component identity index. Populated by components
    /// declared with [`Element::component_state_key`]; consulted in
    /// [`Self::expand_children`] before the usual path-based reuse plan.
    state_key_index: FxHashMap<Key, ComponentId>,
    epoch: u32,
    next_scope: u32,
    /// Theme stack used during expansion. When a `ThemeProvider` is being
    /// expanded, its theme is pushed here so that child `Component` entries
    /// can record it for later use in `refresh_scope_in_place`.
    theme_stack: Vec<crate::style::Theme>,
    /// Typed context stack used during expansion.
    context_stack: Vec<crate::core::element::ContextProviderElement>,
    context_generation: u64,
}

pub(crate) struct ComponentRegistryConfig {
    pub(crate) dispatcher: Dispatcher,
    pub(crate) command_tx: CommandTx,
    pub(crate) env: RuntimeEnv,
}

struct ExpandElementParams {
    parent: Option<ComponentId>,
    key: Option<Key>,
    layout: LayoutConstraints,
    kind: ElementKind,
    index_in_parent: usize,
    epoch: u32,
    viewport: Rect,
}

struct ExpandVecContainerParams {
    parent: Option<ComponentId>,
    key: Option<Key>,
    tag: ContainerTag,
    index_in_parent: usize,
    epoch: u32,
    viewport: Rect,
}

pub(crate) struct ScopeRefreshResult {
    pub(crate) scope: ScopeId,
    pub(crate) theme: crate::style::Theme,
    pub(crate) expanded: Element,
}

impl ComponentRegistry {
    fn current_contexts(&self) -> (FxHashMap<TypeId, Arc<dyn Any>>, FxHashMap<TypeId, u64>) {
        let mut out: FxHashMap<TypeId, Arc<dyn Any>> = FxHashMap::default();
        let mut generations: FxHashMap<TypeId, u64> = FxHashMap::default();
        for provider in &self.context_stack {
            out.insert(provider.type_id, Arc::clone(&provider.value));
            generations.insert(provider.type_id, provider.generation);
        }
        (out, generations)
    }

    pub(crate) fn new(config: ComponentRegistryConfig) -> Self {
        let ComponentRegistryConfig {
            dispatcher,
            command_tx,
            env,
        } = config;

        Self {
            dispatcher,
            command_tx,
            env,
            arena: Arena::new(),
            scope_to_id: FxHashMap::default(),
            state_key_index: FxHashMap::default(),
            epoch: 0,
            next_scope: 2,
            theme_stack: Vec::new(),
            context_stack: Vec::new(),
            context_generation: 1,
        }
    }

    pub(crate) fn begin_epoch(&mut self) -> u32 {
        self.epoch = self.epoch.wrapping_add(1).max(1);
        self.epoch
    }

    pub(crate) fn is_valid(&self, id: ComponentId) -> bool {
        self.arena.is_valid(id)
    }

    /// Trimmed component display name for a mounted scope, if still valid.
    #[cfg_attr(not(feature = "devtools"), allow(dead_code))]
    pub(crate) fn display_name_for_scope(&self, scope: ScopeId) -> Option<Arc<str>> {
        let id = self.scope_to_id.get(&scope).copied()?;
        if !self.is_valid(id) {
            return None;
        }
        Some(Arc::clone(&self.arena.get(id).display_name))
    }

    /// Full `type_name` for a mounted component id, if still valid.
    #[allow(dead_code)] // consumed by DevTools / profiling spans
    pub(crate) fn full_name_for_id(&self, id: ComponentId) -> Option<&'static str> {
        if !self.is_valid(id) {
            return None;
        }
        Some(self.arena.get(id).full_name)
    }

    pub(crate) fn update_by_scope(
        &mut self,
        scope: ScopeId,
        msg: Box<dyn Any>,
    ) -> crate::Result<Update> {
        let Some(id) = self.scope_to_id.get(&scope).copied() else {
            return Ok(Update::none());
        };
        if !self.is_valid(id) {
            return Ok(Update::none());
        }
        let entry = self.arena.get_mut(id);
        if let Some(theme) = entry.active_theme.clone() {
            entry.component.set_active_theme(theme);
        }
        entry.component.set_contexts(
            entry.active_contexts.clone(),
            entry.active_context_generations.clone(),
        );
        let update = entry.component.update(msg)?;
        if update.dirty {
            self.mark_dirty_path(id);
        }
        Ok(update)
    }

    pub(crate) fn on_key_by_scope(&mut self, scope: ScopeId, key: KeyEvent) -> KeyUpdate {
        let Some(id) = self.scope_to_id.get(&scope).copied() else {
            return KeyUpdate::unhandled(Update::none());
        };
        if !self.is_valid(id) {
            return KeyUpdate::unhandled(Update::none());
        }
        let entry = self.arena.get_mut(id);
        if let Some(theme) = entry.active_theme.clone() {
            entry.component.set_active_theme(theme);
        }
        entry.component.set_contexts(
            entry.active_contexts.clone(),
            entry.active_context_generations.clone(),
        );
        let update = entry.component.on_key(key);
        if update.update.dirty {
            self.mark_dirty_path(id);
        }
        update
    }

    pub(crate) fn parent_scope(&self, scope: ScopeId) -> Option<ScopeId> {
        let id = self.scope_to_id.get(&scope).copied()?;
        if !self.is_valid(id) {
            return None;
        }
        let entry = self.arena.get(id);
        match entry.parent {
            None => Some(ScopeId(1)),
            Some(parent) => {
                if self.is_valid(parent) {
                    Some(self.arena.get(parent).scope)
                } else {
                    Some(ScopeId(1))
                }
            }
        }
    }

    pub(crate) fn mount(
        &mut self,
        element: &ComponentElement,
        key: Option<Key>,
        parent: Option<ComponentId>,
        reuse: Option<ComponentId>,
        epoch: u32,
    ) -> ComponentId {
        let current_theme = self
            .theme_stack
            .last()
            .cloned()
            .unwrap_or_else(|| self.env.active_theme.borrow().clone());
        let (current_contexts, current_context_generations) = self.current_contexts();

        if let Some(id) = reuse
            && self.is_valid(id)
        {
            // Reuse is valid when types match. For state-keyed components we
            // intentionally tolerate the stable user-level `key` changing,
            // since the state key is the authoritative identity.
            let (type_matches, key_matches) = {
                let entry = self.arena.get(id);
                (
                    entry.type_id == element.type_id,
                    entry.key == key || element.state_key.is_some(),
                )
            };
            if type_matches && key_matches {
                {
                    let entry = self.arena.get_mut(id);
                    entry.epoch = epoch;
                    entry.parent = parent;
                    entry.key = key;
                }
                self.update_state_key_on_entry(id, element.state_key.clone());
                let entry = self.arena.get_mut(id);
                entry.active_theme = Some(current_theme.clone());
                entry.active_contexts = current_contexts.clone();
                entry.active_context_generations = current_context_generations.clone();
                entry.component.set_active_theme(current_theme);
                entry
                    .component
                    .set_contexts(current_contexts, current_context_generations);
                if !entry.component.props_equal(&element.props) {
                    let update = entry.component.set_props(element.props.clone());
                    entry.self_dirty = true;
                    if let Some(cmd) = update.command {
                        cmd.run(crate::core::component::CommandRuntime {
                            scope: entry.scope,
                            tx: self.command_tx.clone(),
                        });
                    }
                }
                return id;
            }
        }

        let scope = ScopeId(self.next_scope);
        self.next_scope = self.next_scope.wrapping_add(1).max(2);

        let mut env = self.env.clone();
        env.active_theme = std::rc::Rc::new(std::cell::RefCell::new(current_theme.clone()));
        env.active_theme_generation = std::rc::Rc::new(std::cell::Cell::new(1));
        env.contexts = std::rc::Rc::new(std::cell::RefCell::new(current_contexts.clone()));
        env.context_generations =
            std::rc::Rc::new(std::cell::RefCell::new(current_context_generations.clone()));

        let component: Box<dyn ErasedComponent> = match (element.factory)(ComponentMount {
            scope,
            dispatcher: self.dispatcher.clone(),
            env,
            props: element.props.clone(),
        }) {
            Ok(c) => c,
            Err(e) => {
                #[cfg(feature = "profiling-tracing")]
                tracing::error!("component mount failed: {e}");
                let _ = e;
                Box::new(EmptyComponent)
            }
        };

        let full_name = component.component_name();
        let display_name: Arc<str> = Arc::from(short_type_name(full_name));

        let id = self.alloc();
        let entry = ComponentEntry {
            id,
            scope,
            type_id: element.type_id,
            key,
            state_key: None,
            parent,
            host: HostState::default(),
            epoch,
            component,
            display_name,
            full_name,
            initialized: false,
            active_theme: Some(current_theme),
            active_contexts: current_contexts,
            active_context_generations: current_context_generations,
            cached_element: None,
            last_memo_key: None,
            memo_deps: crate::core::runtime_env::MemoDependencySnapshot::default(),
            self_dirty: false,
            descendant_dirty: false,
        };

        self.scope_to_id.insert(scope, id);
        *self.arena.get_mut(id) = entry;
        self.update_state_key_on_entry(id, element.state_key.clone());

        id
    }

    /// Keep `state_key_index` in sync with the entry's current state key.
    ///
    /// Removes any prior index entry pointing at `id`, removes any index
    /// entry under the new key that points at a *different* (possibly stale)
    /// id, and finally installs `id` under `new_key` if `Some`.
    fn update_state_key_on_entry(&mut self, id: ComponentId, new_key: Option<Key>) {
        let old_key = self.arena.get(id).state_key.clone();
        if old_key == new_key {
            if let Some(key) = &new_key {
                // Ensure the index still points at us (in case another
                // instance transiently overwrote it).
                self.state_key_index.insert(key.clone(), id);
            }
            return;
        }
        if let Some(old) = &old_key
            && self.state_key_index.get(old) == Some(&id)
        {
            self.state_key_index.remove(old);
        }
        if let Some(new) = &new_key {
            self.state_key_index.insert(new.clone(), id);
        }
        self.arena.get_mut(id).state_key = new_key;
    }

    fn mark_dirty_path(&mut self, id: ComponentId) {
        let mut current = Some(id);
        let mut is_origin = true;

        while let Some(component_id) = current {
            if !self.is_valid(component_id) {
                break;
            }

            let parent = {
                let entry = self.arena.get_mut(component_id);
                if is_origin {
                    entry.self_dirty = true;
                } else {
                    entry.descendant_dirty = true;
                }
                entry.parent
            };

            current = parent;
            is_origin = false;
        }
    }

    pub(crate) fn expand_in_host(
        &mut self,
        host: &mut HostState,
        parent: Option<ComponentId>,
        root: Element,
        epoch: u32,
        viewport: Rect,
    ) -> Element {
        host.begin_render();

        let mut path = vec![PathSegment {
            tag: ContainerTag::Root,
            id: SegmentId::Index(0),
        }];

        let mut out = self.expand_children(host, parent, &mut path, vec![root], epoch, viewport);
        let root = out
            .pop()
            .unwrap_or_else(|| crate::widgets::Text::new("").into());

        host.finish_render();
        root
    }

    fn expand_component_instance(
        &mut self,
        id: ComponentId,
        epoch: u32,
        viewport: Rect,
    ) -> Element {
        if !self.is_valid(id) {
            return crate::widgets::Text::new("").into();
        }

        #[cfg(feature = "devtools")]
        let mut retain_facts = MemoRetainFacts {
            has_key: false,
            has_cache: false,
            self_dirty: false,
            key_matches: false,
        };
        #[cfg(feature = "devtools")]
        let mut dep_mismatch = None;

        let can_retain = {
            let fallback_theme = self.env.active_theme.borrow().clone();
            let entry = self.arena.get_mut(id);
            let active_theme = entry.active_theme.clone().unwrap_or(fallback_theme);
            entry.component.set_active_theme(active_theme);
            entry.component.set_contexts(
                entry.active_contexts.clone(),
                entry.active_context_generations.clone(),
            );
            entry.component.set_viewport(viewport);

            if !entry.initialized {
                if let Some(cmd) = entry.component.init() {
                    cmd.run(crate::core::component::CommandRuntime {
                        scope: entry.scope,
                        tx: self.command_tx.clone(),
                    });
                }
                entry.initialized = true;
            }

            let memo_key = entry.component.memo_key();
            #[cfg(feature = "devtools")]
            {
                retain_facts.has_key = memo_key.is_some();
                retain_facts.has_cache = entry.cached_element.is_some();
                retain_facts.self_dirty = entry.self_dirty;
                retain_facts.key_matches = entry.last_memo_key == memo_key;
                if retain_facts.has_key
                    && retain_facts.has_cache
                    && !retain_facts.self_dirty
                    && retain_facts.key_matches
                    && !entry.component.memo_dependencies_match(&entry.memo_deps)
                {
                    dep_mismatch = entry.component.memo_dependency_mismatch(&entry.memo_deps);
                }
            }
            let can_retain = memo_key.is_some()
                && entry.cached_element.is_some()
                && !entry.self_dirty
                && entry.last_memo_key == memo_key
                && entry.component.memo_dependencies_match(&entry.memo_deps);

            if can_retain {
                entry.last_memo_key = memo_key;
            }

            can_retain
        };

        if can_retain {
            #[cfg(feature = "devtools")]
            increment_memo_hit_counter();

            let needs_descendant_refresh = self
                .direct_child_ids(id)
                .into_iter()
                .any(|child_id| self.component_subtree_needs_refresh(child_id));

            let mut cached = self
                .arena
                .get(id)
                .cached_element
                .clone()
                .unwrap_or_else(|| crate::widgets::Text::new("").into());

            if needs_descendant_refresh
                && !self.refresh_retained_children(id, &mut cached, epoch, viewport)
            {
                #[cfg(feature = "devtools")]
                record_memo_miss(MemoMissReason::RetainedChildRefreshFailed);
                return self.render_component_instance(id, epoch, viewport);
            }

            if self.is_valid(id) {
                let entry = self.arena.get_mut(id);
                entry.cached_element = Some(cached.clone());
                entry.self_dirty = false;
                entry.descendant_dirty = false;
            }

            return cached;
        }

        #[cfg(feature = "devtools")]
        record_memo_miss(classify_component_miss(retain_facts, dep_mismatch));

        self.render_component_instance(id, epoch, viewport)
    }

    pub(crate) fn refresh_scope_in_place(
        &mut self,
        scope: ScopeId,
        viewport: Rect,
    ) -> Option<ScopeRefreshResult> {
        if scope == ScopeId(1) {
            return None;
        }

        let id = self.scope_to_id.get(&scope).copied()?;
        if !self.is_valid(id) {
            return None;
        }

        let theme = self
            .arena
            .get(id)
            .active_theme
            .clone()
            .unwrap_or_else(|| self.env.active_theme.borrow().clone());
        #[cfg(feature = "profiling-tracing")]
        let _refresh_span = {
            let full_name = self.arena.get(id).full_name;
            let scope = self.arena.get(id).scope.0;
            tracing::trace_span!("component.refresh", component = full_name, scope = scope)
                .entered()
        };
        let expanded = self.expand_component_instance(id, self.epoch, viewport);
        let (splice_scope, expanded_for_splice) =
            self.propagate_cached_replacement_to_ancestors(id, &expanded);
        self.refresh_pending_dirty_flags(self.arena.get(id).parent);

        Some(ScopeRefreshResult {
            scope: splice_scope,
            theme,
            expanded: expanded_for_splice,
        })
    }

    fn render_component_instance(
        &mut self,
        id: ComponentId,
        epoch: u32,
        viewport: Rect,
    ) -> Element {
        let (element, mut host, memo_key, memo_deps) = {
            let entry = self.arena.get_mut(id);
            // Start dependency capture before memo_key() so that any context
            // reads inside memo_key() itself are recorded.
            entry.component.begin_memo_dependency_capture();
            let memo_key = entry.component.memo_key();
            let (element, memo_deps) = if memo_key.is_some() {
                #[cfg(feature = "devtools")]
                let view_start = web_time::Instant::now();
                #[cfg(feature = "profiling-tracing")]
                let _view_span = tracing::trace_span!(
                    "component.view",
                    component = entry.full_name,
                    scope = entry.scope.0
                )
                .entered();
                let element = entry.component.view();
                #[cfg(feature = "devtools")]
                record_view_timing(
                    entry.scope,
                    Arc::clone(&entry.display_name),
                    view_start.elapsed(),
                );
                let memo_deps = entry.component.finish_memo_dependency_capture();
                (element, memo_deps)
            } else {
                entry.component.finish_memo_dependency_capture();
                #[cfg(feature = "devtools")]
                let view_start = web_time::Instant::now();
                #[cfg(feature = "profiling-tracing")]
                let _view_span = tracing::trace_span!(
                    "component.view",
                    component = entry.full_name,
                    scope = entry.scope.0
                )
                .entered();
                let element = entry.component.view();
                #[cfg(feature = "devtools")]
                record_view_timing(
                    entry.scope,
                    Arc::clone(&entry.display_name),
                    view_start.elapsed(),
                );
                (
                    element,
                    crate::core::runtime_env::MemoDependencySnapshot::default(),
                )
            };
            let host = std::mem::take(&mut entry.host);
            (element, host, memo_key, memo_deps)
        };

        let expanded = self.expand_in_host(&mut host, Some(id), element, epoch, viewport);

        if self.is_valid(id) {
            let entry = self.arena.get_mut(id);
            entry.host = host;
            entry.cached_element = memo_key.map(|_| expanded.clone());
            entry.last_memo_key = memo_key;
            entry.memo_deps = memo_deps;
            entry.self_dirty = false;
            entry.descendant_dirty = false;
        }

        expanded
    }

    fn direct_child_ids(&self, id: ComponentId) -> Vec<ComponentId> {
        if !self.is_valid(id) {
            return Vec::new();
        }

        let mut out = Vec::new();
        for child_id in self
            .arena
            .get(id)
            .host
            .slots_prev
            .values()
            .flatten()
            .copied()
        {
            if !out.contains(&child_id) {
                out.push(child_id);
            }
        }
        out
    }

    fn component_subtree_needs_refresh(&self, id: ComponentId) -> bool {
        if !self.is_valid(id) {
            return false;
        }

        let entry = self.arena.get(id);
        if entry.self_dirty || entry.descendant_dirty {
            return true;
        }
        if entry.last_memo_key.is_none()
            || entry.cached_element.is_none()
            || !entry.component.memo_dependencies_match(&entry.memo_deps)
        {
            return true;
        }

        self.direct_child_ids(id)
            .into_iter()
            .any(|child_id| self.component_subtree_needs_refresh(child_id))
    }

    fn refresh_retained_children(
        &mut self,
        id: ComponentId,
        element: &mut Element,
        epoch: u32,
        viewport: Rect,
    ) -> bool {
        for child_id in self.direct_child_ids(id) {
            if !self.component_subtree_needs_refresh(child_id) {
                continue;
            }

            let replacement = self.expand_component_instance(child_id, epoch, viewport);
            let scope = self.arena.get(child_id).scope;
            let mut replacement = Some(replacement);
            if element.replace_group_child_by_scope(scope, &mut replacement) {
                continue;
            }
            return false;
        }

        true
    }

    fn propagate_cached_replacement_to_ancestors(
        &mut self,
        id: ComponentId,
        replacement: &Element,
    ) -> (ScopeId, Element) {
        let mut current_id = id;
        let mut current_scope = self.arena.get(id).scope;
        let mut current_replacement = replacement.clone();

        while let Some(parent_id) = self.arena.get(current_id).parent {
            if !self.is_valid(parent_id) {
                break;
            }

            let Some(mut parent_cached) = self.arena.get(parent_id).cached_element.clone() else {
                // Non-memoized intermediate parent - skip it but keep walking
                // so memoized ancestors further up get their caches updated.
                current_id = parent_id;
                continue;
            };

            let mut replacement = Some(current_replacement.clone());
            if parent_cached.replace_group_child_by_scope(current_scope, &mut replacement) {
                current_scope = self.arena.get(parent_id).scope;
                current_id = parent_id;
                current_replacement = parent_cached.clone();
                self.arena.get_mut(parent_id).cached_element = Some(parent_cached);
            } else {
                // Scope not found in this cached snapshot; keep walking upward.
                current_id = parent_id;
                continue;
            }
        }

        (current_scope, current_replacement)
    }

    fn refresh_pending_dirty_flags(&mut self, mut current: Option<ComponentId>) {
        while let Some(id) = current {
            if !self.is_valid(id) {
                break;
            }

            let descendant_dirty = self.direct_child_ids(id).into_iter().any(|child_id| {
                if !self.is_valid(child_id) {
                    return false;
                }
                let entry = self.arena.get(child_id);
                entry.self_dirty || entry.descendant_dirty
            });

            current = {
                let entry = self.arena.get_mut(id);
                entry.descendant_dirty = descendant_dirty;
                entry.parent
            };
        }
    }

    fn expand_children(
        &mut self,
        host: &mut HostState,
        parent: Option<ComponentId>,
        path: &mut ContainerPath,
        children: Vec<Element>,
        epoch: u32,
        viewport: Rect,
    ) -> Vec<Element> {
        let mut specs = Vec::new();
        for child in &children {
            if let ElementKind::Component(component) = &child.kind {
                specs.push(ComponentSpec {
                    key: child.key.clone(),
                    type_id: component.type_id,
                });
            }
        }

        let old_ids = host.prev_ids(path);
        let plan = reuse_plan(
            old_ids,
            &specs,
            |id| self.arena.get(*id).key.clone(),
            |spec| spec.key.clone(),
            |id| self.is_valid(*id),
            |id, spec| self.arena.get(*id).type_id == spec.type_id,
        );
        let mut plan_cursor = 0usize;

        let mut next_ids: Vec<ComponentId> = Vec::with_capacity(specs.len());
        let mut out: Vec<Element> = Vec::with_capacity(children.len());

        for (idx, child) in children.into_iter().enumerate() {
            let Element {
                key, kind, layout, ..
            } = child;
            match kind {
                ElementKind::Component(component) => {
                    let plan_reuse = plan.get(plan_cursor).copied().unwrap_or(None);
                    plan_cursor += 1;

                    // State-keyed components consult a registry-global index so
                    // their instance (and state) survives ancestor reshaping.
                    // Only override when the index points at a valid entry of
                    // the matching type; otherwise fall through to the usual
                    // path-based reuse plan.
                    #[cfg(debug_assertions)]
                    if let Some(state_key) = component.state_key.as_ref()
                        && let Some(&existing_id) = self.state_key_index.get(state_key)
                        && self.is_valid(existing_id)
                        && self.arena.get(existing_id).parent == parent
                    {
                        crate::debug::internal_log!(
                            "Duplicate component_state_key {:?} detected; last-writer-wins",
                            state_key
                        );
                    }

                    let reuse = if let Some(state_key) = component.state_key.as_ref() {
                        self.state_key_index
                            .get(state_key)
                            .copied()
                            .filter(|id| {
                                self.is_valid(*id)
                                    && self.arena.get(*id).type_id == component.type_id
                            })
                            .or(plan_reuse)
                    } else {
                        plan_reuse
                    };

                    let id = self.mount(&component, key.clone(), parent, reuse, epoch);
                    next_ids.push(id);

                    // Record the active theme from the nearest ancestor ThemeProvider
                    // so that `refresh_scope_in_place` can re-apply it later.
                    let active_theme = self
                        .theme_stack
                        .last()
                        .cloned()
                        .unwrap_or_else(|| self.env.active_theme.borrow().clone());
                    let (active_contexts, active_context_generations) = self.current_contexts();
                    self.arena.get_mut(id).active_theme = Some(active_theme.clone());
                    self.arena.get_mut(id).active_contexts = active_contexts.clone();
                    self.arena.get_mut(id).active_context_generations =
                        active_context_generations.clone();
                    self.arena
                        .get_mut(id)
                        .component
                        .set_active_theme(active_theme);
                    self.arena
                        .get_mut(id)
                        .component
                        .set_contexts(active_contexts, active_context_generations);

                    // set_viewport and init are handled by expand_component_instance below.
                    // We don't handle the update from set_props here because expand_component_instance will call view()
                    // and we are already in a render cycle.

                    let expanded = self.expand_component_instance(id, epoch, viewport);
                    let scope = self.arena.get(id).scope;

                    out.push(Element {
                        key,
                        kind: ElementKind::Group(Group {
                            scope,
                            child: Box::new(expanded),
                        }),
                        layout,
                        layout_hash_cache: Cell::new(None),
                        measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                        split_wrap_probe_cache: Cell::new(None),
                    });
                }
                other => {
                    out.push(self.expand_element(
                        host,
                        path,
                        ExpandElementParams {
                            parent,
                            key,
                            layout,
                            kind: other,
                            index_in_parent: idx,
                            epoch,
                            viewport,
                        },
                    ));
                }
            }
        }

        host.set_next_ids(path, next_ids);
        out
    }

    fn expand_element(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        params: ExpandElementParams,
    ) -> Element {
        let ExpandElementParams {
            parent,
            key,
            layout,
            kind,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        match kind {
            ElementKind::VStack(mut vs) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut vs.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::VStack,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::VStack(vs),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::HStack(mut hs) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut hs.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::HStack,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::HStack(hs),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Flow(mut flow) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut flow.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::Flow,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::Flow(flow),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::ScrollView(mut sv) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut sv.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::ScrollView,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::ScrollView(sv),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Splitter(mut splitter) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut splitter.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::Splitter,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::Splitter(splitter),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Grid(mut grid) => {
                let placements: Vec<_> = grid.items.iter().map(|i| (i.placement, i.span)).collect();
                let mut children: Vec<Element> = std::mem::take(&mut grid.items)
                    .into_iter()
                    .map(|i| i.element)
                    .collect();
                self.expand_vec_container(
                    host,
                    path,
                    &mut children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::Grid,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                grid.items = children
                    .into_iter()
                    .zip(placements)
                    .map(|(element, (placement, span))| crate::widgets::GridItem {
                        element,
                        placement,
                        span,
                    })
                    .collect();
                Element {
                    key,
                    kind: ElementKind::Grid(grid),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Canvas(mut canvas) => {
                let rects: Vec<_> = canvas.items.iter().map(|item| item.rect).collect();
                let mut children: Vec<Element> = std::mem::take(&mut canvas.items)
                    .into_iter()
                    .map(|item| item.element)
                    .collect();
                self.expand_vec_container(
                    host,
                    path,
                    &mut children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::Canvas,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                canvas.items = children
                    .into_iter()
                    .zip(rects)
                    .map(|(element, rect)| crate::widgets::CanvasItem { rect, element })
                    .collect();
                Element {
                    key,
                    kind: ElementKind::Canvas(canvas),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Frame(mut frame) => {
                let seg = PathSegment {
                    tag: ContainerTag::Frame,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);

                let header = frame.header.take().map(|c| *c);
                let child = frame.child.take().map(|c| *c);
                let has_header = header.is_some();
                let has_child = child.is_some();

                if has_header || has_child {
                    let mut children = Vec::new();
                    if let Some(header) = header {
                        children.push(header);
                    }
                    if let Some(child) = child {
                        children.push(child);
                    }

                    let expanded =
                        self.expand_children(host, parent, path, children, epoch, viewport);
                    let mut iter = expanded.into_iter();
                    if has_header {
                        frame.header = iter.next().map(Box::new);
                    }
                    if has_child {
                        frame.child = iter.next().map(Box::new);
                    }
                }

                path.pop();
                Element {
                    key,
                    kind: ElementKind::Frame(frame),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::ZStack(mut zs) => {
                self.expand_vec_container(
                    host,
                    path,
                    &mut zs.children,
                    ExpandVecContainerParams {
                        parent,
                        key: key.clone(),
                        tag: ContainerTag::ZStack,
                        index_in_parent,
                        epoch,
                        viewport,
                    },
                );
                Element {
                    key,
                    kind: ElementKind::ZStack(zs),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Center(mut center) => {
                let seg = PathSegment {
                    tag: ContainerTag::Center,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let child = center.child.take().map(|c| *c);
                if let Some(child) = child {
                    let mut children =
                        self.expand_children(host, parent, path, vec![child], epoch, viewport);
                    center.child = children.pop().map(Box::new);
                }
                path.pop();
                Element {
                    key,
                    kind: ElementKind::Center(center),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::CenterPin(mut cp) => {
                let seg = PathSegment {
                    tag: ContainerTag::CenterPin,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let slots: Vec<Option<Box<crate::core::element::Element>>> =
                    vec![cp.top.take(), cp.center.take(), cp.bottom.take()];
                let expanded: Vec<Option<crate::core::element::Element>> = slots
                    .into_iter()
                    .enumerate()
                    .map(|(slot_idx, opt)| {
                        opt.map(|boxed| {
                            path.push(PathSegment {
                                tag: ContainerTag::CenterPinSlot,
                                id: SegmentId::Index(slot_idx),
                            });
                            let mut children = self.expand_children(
                                host,
                                parent,
                                path,
                                vec![*boxed],
                                epoch,
                                viewport,
                            );
                            path.pop();
                            children.pop().unwrap()
                        })
                    })
                    .collect();
                let mut iter = expanded.into_iter();
                cp.top = iter.next().flatten().map(Box::new);
                cp.center = iter.next().flatten().map(Box::new);
                cp.bottom = iter.next().flatten().map(Box::new);
                path.pop();
                Element {
                    key,
                    kind: ElementKind::CenterPin(cp),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::StatusBarLayout(mut status_layout) => {
                let seg = PathSegment {
                    tag: ContainerTag::StatusBarLayout,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let slots = vec![
                    status_layout.left,
                    status_layout.center,
                    status_layout.right,
                ];
                let mut expanded = slots
                    .into_iter()
                    .enumerate()
                    .map(|(slot_idx, boxed)| {
                        path.push(PathSegment {
                            tag: ContainerTag::StatusBarLayoutSlot,
                            id: SegmentId::Index(slot_idx),
                        });
                        let mut children =
                            self.expand_children(host, parent, path, vec![*boxed], epoch, viewport);
                        path.pop();
                        Box::new(children.pop().unwrap())
                    })
                    .collect::<Vec<_>>()
                    .into_iter();
                status_layout.left = expanded.next().unwrap();
                status_layout.center = expanded.next().unwrap();
                status_layout.right = expanded.next().unwrap();
                path.pop();
                Element {
                    key,
                    kind: ElementKind::StatusBarLayout(status_layout),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Popover(mut popover) => {
                let seg = PathSegment {
                    tag: ContainerTag::Popover,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let trigger = *popover.trigger;
                let content = *popover.content;
                let expanded = self.expand_children(
                    host,
                    parent,
                    path,
                    vec![trigger, content],
                    epoch,
                    viewport,
                );
                let mut iter = expanded.into_iter();
                popover.trigger = Box::new(
                    iter.next()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into()),
                );
                popover.content = Box::new(
                    iter.next()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into()),
                );
                path.pop();
                Element {
                    key,
                    kind: ElementKind::Popover(popover),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Portal(mut portal) => {
                let seg = PathSegment {
                    tag: ContainerTag::Portal,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let child = *portal.content;
                let mut children =
                    self.expand_children(host, parent, path, vec![child], epoch, viewport);
                portal.content = Box::new(
                    children
                        .pop()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into()),
                );
                path.pop();
                Element {
                    key,
                    kind: ElementKind::Portal(portal),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Group(mut group) => {
                let seg = PathSegment {
                    tag: ContainerTag::Group,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);

                let child = *group.child;
                let mut children =
                    self.expand_children(host, parent, path, vec![child], epoch, viewport);
                group.child = Box::new(
                    children
                        .pop()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into()),
                );

                path.pop();
                Element {
                    key,
                    kind: ElementKind::Group(group),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::ThemeProvider(tp) => {
                let tp = *tp;
                let seg = PathSegment {
                    tag: ContainerTag::ThemeProvider,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                self.theme_stack.push(tp.theme.clone());
                let mut expanded =
                    self.expand_children(host, parent, path, vec![tp.child], epoch, viewport);
                self.theme_stack.pop();
                path.pop();
                let child = expanded
                    .pop()
                    .unwrap_or_else(|| crate::widgets::Text::new("").into());
                #[cfg(feature = "profiling-tracing")]
                let theme_start = web_time::Instant::now();
                let child = crate::style::apply_document_theme_carve_out(&tp.theme, child);
                #[cfg(feature = "profiling-tracing")]
                tracing::trace!(
                    target: "tui_lipan::perf",
                    apply_theme_ms = theme_start.elapsed().as_secs_f64() * 1000.0,
                );
                Element {
                    key,
                    kind: ElementKind::ThemeProvider(Box::new(
                        crate::core::element::ThemeProviderElement {
                            theme: tp.theme,
                            child,
                        },
                    )),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::ContextProvider(cp) => {
                let cp = *cp;
                let seg = PathSegment {
                    tag: ContainerTag::ContextProvider,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);

                let prev = host.prev_context(path).cloned();
                let generation = if let Some(prev) = &prev {
                    if prev.type_id == cp.type_id
                        && (cp.equals)(prev.value.as_ref(), cp.value.as_ref())
                    {
                        prev.generation
                    } else {
                        self.context_generation = self.context_generation.wrapping_add(1).max(1);
                        self.context_generation
                    }
                } else {
                    self.context_generation = self.context_generation.wrapping_add(1).max(1);
                    self.context_generation
                };

                let mut provider = cp.clone();
                provider.generation = generation;
                host.set_next_context(
                    path,
                    host::ContextProviderRecord {
                        type_id: provider.type_id,
                        value: Arc::clone(&provider.value),
                        generation,
                    },
                );

                self.context_stack.push(provider.clone());
                let mut expanded =
                    self.expand_children(host, parent, path, vec![provider.child], epoch, viewport);
                self.context_stack.pop();
                path.pop();

                expanded
                    .pop()
                    .unwrap_or_else(|| crate::widgets::Text::new("").into())
            }
            ElementKind::Memo(memo) => {
                let seg = PathSegment {
                    tag: ContainerTag::Memo,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);

                // Probe deps_hash without cloning; only deep-clone the cached
                // subtree on a true hit (deps match).
                let hit = host
                    .prev_memo(path, memo.call_site)
                    .is_some_and(|cached| cached.deps_hash == memo.deps_hash);
                let child = if hit {
                    let (mut expanded_child, descendant_ids) = host
                        .prev_memo(path, memo.call_site)
                        .map(|cached| {
                            (cached.expanded_child.clone(), cached.descendant_ids.clone())
                        })
                        .expect("hit implies a previous memo entry");
                    let needs_refresh = descendant_ids
                        .iter()
                        .copied()
                        .any(|id| self.component_subtree_needs_refresh(id));
                    if needs_refresh {
                        let descendant_ids =
                            collect_component_ids_in_element(&expanded_child, &self.scope_to_id);
                        for child_id in descendant_ids {
                            if !self.component_subtree_needs_refresh(child_id) {
                                continue;
                            }
                            let replacement =
                                self.expand_component_instance(child_id, epoch, viewport);
                            let scope = self.arena.get(child_id).scope;
                            let mut replacement = Some(replacement);
                            if expanded_child.replace_group_child_by_scope(scope, &mut replacement)
                            {
                                continue;
                            } else {
                                #[cfg(feature = "devtools")]
                                record_memo_miss(MemoMissReason::ViewMemoStructureChanged);
                                let rebuilt = (memo.builder)();
                                let mut expanded = self.expand_children(
                                    host,
                                    parent,
                                    path,
                                    vec![rebuilt],
                                    epoch,
                                    viewport,
                                );
                                let child = expanded
                                    .pop()
                                    .unwrap_or_else(|| crate::widgets::Text::new("").into());
                                let descendant_ids =
                                    collect_component_ids_in_element(&child, &self.scope_to_id);
                                host.set_next_memo(
                                    path,
                                    memo.call_site,
                                    MemoCacheEntry {
                                        deps_hash: memo.deps_hash,
                                        expanded_child: child.clone(),
                                        descendant_ids,
                                    },
                                );
                                path.pop();
                                return child;
                            }
                        }
                    }
                    #[cfg(feature = "devtools")]
                    increment_memo_hit_counter();
                    expanded_child
                } else if host.prev_memo(path, memo.call_site).is_some() {
                    #[cfg(feature = "devtools")]
                    record_memo_miss(MemoMissReason::ViewMemoDepsChanged);
                    let rebuilt = (memo.builder)();
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![rebuilt], epoch, viewport);
                    expanded
                        .pop()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into())
                } else {
                    #[cfg(feature = "devtools")]
                    record_memo_miss(MemoMissReason::ViewMemoNoCache);
                    let rebuilt = (memo.builder)();
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![rebuilt], epoch, viewport);
                    expanded
                        .pop()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into())
                };

                let descendant_ids = collect_component_ids_in_element(&child, &self.scope_to_id);
                host.set_next_memo(
                    path,
                    memo.call_site,
                    MemoCacheEntry {
                        deps_hash: memo.deps_hash,
                        expanded_child: child.clone(),
                        descendant_ids,
                    },
                );
                path.pop();
                child
            }
            ElementKind::EffectScope(mut scope) => {
                let seg = PathSegment {
                    tag: ContainerTag::EffectScope,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                if let Some(child) = scope.child.take() {
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![*child], epoch, viewport);
                    scope.child = expanded.pop().map(Box::new);
                }
                path.pop();
                Element {
                    key,
                    kind: ElementKind::EffectScope(scope),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::MouseRegion(mut region) => {
                let seg = PathSegment {
                    tag: ContainerTag::MouseRegion,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                if let Some(child) = region.child.take() {
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![*child], epoch, viewport);
                    region.child = expanded.pop().map(Box::new);
                }
                path.pop();
                Element {
                    key,
                    kind: ElementKind::MouseRegion(region),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::DragSource(mut source) => {
                let seg = PathSegment {
                    tag: ContainerTag::DragSource,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                if let Some(child) = source.child.take() {
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![*child], epoch, viewport);
                    source.child = expanded.pop().map(Box::new);
                }
                path.pop();
                Element {
                    key,
                    kind: ElementKind::DragSource(source),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::DropTarget(mut target) => {
                let seg = PathSegment {
                    tag: ContainerTag::DropTarget,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                if let Some(child) = target.child.take() {
                    let mut expanded =
                        self.expand_children(host, parent, path, vec![*child], epoch, viewport);
                    target.child = expanded.pop().map(Box::new);
                }
                path.pop();
                Element {
                    key,
                    kind: ElementKind::DropTarget(target),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Animated(mut animated) => {
                let seg = PathSegment {
                    tag: ContainerTag::Animated,
                    id: segment_id(&key, index_in_parent),
                };
                path.push(seg);
                let child = *animated.child;
                let mut expanded =
                    self.expand_children(host, parent, path, vec![child], epoch, viewport);
                animated.child = Box::new(
                    expanded
                        .pop()
                        .unwrap_or_else(|| crate::widgets::Text::new("").into()),
                );
                path.pop();
                Element {
                    key,
                    kind: ElementKind::Animated(animated),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            ElementKind::Component(component) => {
                debug_assert!(
                    false,
                    "component elements must be expanded in parent context"
                );
                Element {
                    key,
                    kind: ElementKind::Component(component),
                    layout,
                    layout_hash_cache: Cell::new(None),
                    measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                    split_wrap_probe_cache: Cell::new(None),
                }
            }
            other => Element {
                key,
                kind: other,
                layout,
                layout_hash_cache: Cell::new(None),
                measure_cache: Cell::new([None::<MeasureCacheEntry>, None]),
                split_wrap_probe_cache: Cell::new(None),
            },
        }
    }

    /// Shared logic for container elements whose children live in a `Vec<Element>`
    /// (VStack, HStack, ScrollView, Canvas, ZStack).
    fn expand_vec_container(
        &mut self,
        host: &mut HostState,
        path: &mut ContainerPath,
        children: &mut Vec<Element>,
        params: ExpandVecContainerParams,
    ) {
        let ExpandVecContainerParams {
            parent,
            key,
            tag,
            index_in_parent,
            epoch,
            viewport,
        } = params;
        let seg = PathSegment {
            tag,
            id: segment_id(&key, index_in_parent),
        };
        path.push(seg);
        let taken = std::mem::take(children);
        *children = self.expand_children(host, parent, path, taken, epoch, viewport);
        path.pop();
    }

    fn alloc(&mut self) -> ComponentId {
        self.arena
            .alloc_with(ComponentEntry::empty, ComponentEntry::reset_for_reuse)
    }

    pub(crate) fn sweep(&mut self, current_epoch: u32) {
        let scope_to_id = &mut self.scope_to_id;
        let state_key_index = &mut self.state_key_index;
        let command_registry = self.env.command_registry.clone();
        let scroll = self.env.scroll.clone();
        self.arena.sweep(
            |entry| entry.epoch != current_epoch,
            |entry| {
                command_registry.unregister_scope(entry.scope);
                scroll.remove_scope(entry.scope);
                scope_to_id.remove(&entry.scope);
                if let Some(key) = &entry.state_key
                    && state_key_index.get(key) == Some(&entry.id)
                {
                    state_key_index.remove(key);
                }
                entry.reset_for_free();
            },
        );
    }
}

fn collect_component_ids_in_element(
    element: &Element,
    scope_to_id: &FxHashMap<ScopeId, ComponentId>,
) -> Vec<ComponentId> {
    fn walk(
        element: &Element,
        scope_to_id: &FxHashMap<ScopeId, ComponentId>,
        out: &mut Vec<ComponentId>,
    ) {
        match &element.kind {
            ElementKind::Group(group) => {
                if let Some(id) = scope_to_id.get(&group.scope).copied()
                    && !out.contains(&id)
                {
                    out.push(id);
                }
                walk(group.child.as_ref(), scope_to_id, out);
            }
            _ => {
                for child in element.kind.children() {
                    walk(child, scope_to_id, out);
                }
            }
        }
    }

    let mut out = Vec::new();
    walk(element, scope_to_id, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::hash::{Hash, Hasher};
    use std::rc::Rc;

    use super::{
        ComponentId, ComponentRegistry, ComponentRegistryConfig, HostState, short_type_name,
    };
    use crate::app::context::SurfaceMode;
    use crate::app::input::command_registry::CommandRegistry;
    use crate::callback::{Dispatcher, ScopeId};
    use crate::core::component::{
        Component, Context, FocusContext, HoverContext, ScrollContext, Update,
    };
    use crate::core::runtime_env::RuntimeEnv;

    use crate::core::element::{Element, ElementKind, Key};
    use crate::style::{Color, Rect, Style, Theme};

    use crate::Memo;
    use crate::widgets::{ContextProvider, Text, ThemeProvider, VStack};

    #[test]
    fn short_type_name_strips_module_paths() {
        assert_eq!(short_type_name("Panel"), "Panel");
        assert_eq!(short_type_name("app::Panel"), "Panel");
        assert_eq!(
            short_type_name("app::Panel<alloc::string::String>"),
            "Panel<String>"
        );
        assert_eq!(
            short_type_name("a::b::Outer<a::c::Inner<alloc::vec::Vec<u8>>>"),
            "Outer<Inner<Vec<u8>>>"
        );
        assert_eq!(short_type_name("(app::A, core::B)"), "(A, B)");
        // Closures keep their opaque suffix; we only strip `::` segments.
        let closure = short_type_name("app::foo::{{closure}}");
        assert_eq!(closure, "{{closure}}");
    }

    fn new_registry() -> ComponentRegistry {
        let dispatcher = Dispatcher::new(|_, _| {});
        let (command_tx, _command_rx) = std::sync::mpsc::channel();
        let quit = Rc::new(Cell::new(false));
        let overlay_manager = Rc::new(RefCell::new(crate::overlay::OverlayManager::new()));

        ComponentRegistry::new(ComponentRegistryConfig {
            dispatcher,
            command_tx,
            env: RuntimeEnv {
                command_registry: CommandRegistry::default(),
                quit,
                focus: Rc::new(FocusContext::default()),
                hover: Rc::new(HoverContext::default()),
                scroll: Rc::new(ScrollContext::default()),
                animations: Rc::new(crate::animation::AnimationRegistry::default()),
                overlay_manager,
                focus_request: Rc::new(RefCell::new(None)),
                mouse_capture: Rc::new(Cell::new(true)),
                surface_mode: SurfaceMode::Fullscreen,
                transcript_history: Rc::new(RefCell::new(Vec::new())),
                pending_transcript_entries: Rc::new(RefCell::new(VecDeque::new())),
                clipboard: crate::clipboard::test_clipboard(),
                clipboard_config: crate::clipboard::ClipboardConfig::default(),
                active_theme: Rc::new(RefCell::new(Theme::default())),
                active_theme_generation: Rc::new(Cell::new(1)),
                effect_phase: Rc::new(Cell::new(0)),
                contexts: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                context_generations: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                host_terminal_colors: Rc::new(Cell::new(None)),
                host_terminal_color_generation: Rc::new(Cell::new(0)),
                host_terminal_color_refresh_requested: Rc::new(Cell::new(false)),
                host_terminal_color_refresh_enabled: false,
                mouse_capture_generation: Rc::new(Cell::new(1)),
                memo_dependency_recorder: Rc::new(RefCell::new(None)),
                full_repaint: Rc::new(Cell::new(false)),
                devtools_request: Rc::new(RefCell::new(None)),
                ui_snapshot_request: Rc::new(RefCell::new(None)),
                command_chord_pending: Rc::new(Cell::new(false)),
            },
        })
    }

    fn hash_value<T: Hash>(value: &T) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    struct Counter;

    enum Msg {
        Inc,
    }

    impl Component for Counter {
        type Message = Msg;
        type Properties = u32;
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("{}:{}", ctx.props, ctx.state)).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Inc => {
                    ctx.state += 1;
                    Update::full()
                }
            }
        }
    }

    fn collect_texts(el: &Element, out: &mut Vec<String>) {
        match &el.kind {
            ElementKind::Text(t) => out.push(t.plain_content()),
            ElementKind::VStack(vs) => {
                for child in &vs.children {
                    collect_texts(child, out);
                }
            }
            ElementKind::Flow(flow) => {
                for child in &flow.children {
                    collect_texts(child, out);
                }
            }
            ElementKind::Group(group) => collect_texts(group.child.as_ref(), out),
            ElementKind::EffectScope(scope) => {
                if let Some(child) = scope.child.as_ref() {
                    collect_texts(child.as_ref(), out);
                }
            }
            _ => {}
        }
    }

    fn texts_in_order(el: &Element) -> Vec<String> {
        let mut out = Vec::new();
        collect_texts(el, &mut out);
        out
    }

    fn find_component_by_key(
        registry: &ComponentRegistry,
        key: &Key,
    ) -> Option<(ComponentId, ScopeId)> {
        registry
            .arena
            .iter_active()
            .find_map(|entry| (entry.key.as_ref() == Some(key)).then_some((entry.id, entry.scope)))
    }

    #[test]
    fn display_name_for_scope_returns_trimmed_name_after_mount() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let epoch = registry.begin_epoch();
        let root: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).key("named"))
            .into();
        let _ = registry.expand_in_host(&mut host, None, root, epoch, Rect::default());

        let key: Key = "named".into();
        let (id, scope) = find_component_by_key(&registry, &key).expect("mounted Counter");
        let display = registry
            .display_name_for_scope(scope)
            .expect("display name");
        assert_eq!(display.as_ref(), "Counter");
        assert!(
            registry
                .full_name_for_id(id)
                .expect("full name")
                .ends_with("Counter")
        );
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn classify_component_miss_orders_reasons() {
        use super::{MemoDependencyKind, MemoMissReason, MemoRetainFacts, classify_component_miss};

        let base = MemoRetainFacts {
            has_key: true,
            has_cache: true,
            self_dirty: false,
            key_matches: true,
        };
        assert_eq!(
            classify_component_miss(
                MemoRetainFacts {
                    has_key: false,
                    ..base
                },
                None
            ),
            MemoMissReason::NotMemoized
        );
        assert_eq!(
            classify_component_miss(
                MemoRetainFacts {
                    has_cache: false,
                    ..base
                },
                None
            ),
            MemoMissReason::NoCache
        );
        assert_eq!(
            classify_component_miss(
                MemoRetainFacts {
                    self_dirty: true,
                    ..base
                },
                None
            ),
            MemoMissReason::SelfDirty
        );
        assert_eq!(
            classify_component_miss(
                MemoRetainFacts {
                    key_matches: false,
                    ..base
                },
                None
            ),
            MemoMissReason::KeyChanged
        );
        assert_eq!(
            classify_component_miss(base, Some(MemoDependencyKind::Focus)),
            MemoMissReason::DependencyChanged(MemoDependencyKind::Focus)
        );
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn aggregate_view_timings_sums_per_scope_and_keeps_top_n() {
        use super::{ViewTimingSample, aggregate_view_timings};
        use std::sync::Arc;
        use std::time::Duration;

        let samples = vec![
            ViewTimingSample {
                scope: ScopeId(2),
                name: Arc::from("A"),
                duration: Duration::from_micros(100),
            },
            ViewTimingSample {
                scope: ScopeId(2),
                name: Arc::from("A"),
                duration: Duration::from_micros(50),
            },
            ViewTimingSample {
                scope: ScopeId(3),
                name: Arc::from("B"),
                duration: Duration::from_micros(80),
            },
        ];
        let top = aggregate_view_timings(samples, 1);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].name.as_ref(), "A");
        assert_eq!(top[0].calls, 2);
        assert_eq!(top[0].duration, Duration::from_micros(150));
    }

    #[test]
    fn keyed_children_keep_state_when_reordered() {
        let dispatcher = Dispatcher::new(|_, _| {});
        let (command_tx, _command_rx) = std::sync::mpsc::channel();
        let quit = Rc::new(Cell::new(false));
        let overlay_manager = Rc::new(RefCell::new(crate::overlay::OverlayManager::new()));

        let mut registry = ComponentRegistry::new(ComponentRegistryConfig {
            dispatcher,
            command_tx,
            env: RuntimeEnv {
                command_registry: CommandRegistry::default(),
                quit,
                focus: Rc::new(FocusContext::default()),
                hover: Rc::new(HoverContext::default()),
                scroll: Rc::new(ScrollContext::default()),
                animations: Rc::new(crate::animation::AnimationRegistry::default()),
                overlay_manager,
                focus_request: Rc::new(RefCell::new(None)),
                mouse_capture: Rc::new(Cell::new(true)),
                surface_mode: SurfaceMode::Fullscreen,
                transcript_history: Rc::new(RefCell::new(Vec::new())),
                pending_transcript_entries: Rc::new(RefCell::new(VecDeque::new())),
                clipboard: crate::clipboard::test_clipboard(),
                clipboard_config: crate::clipboard::ClipboardConfig::default(),
                active_theme: Rc::new(RefCell::new(crate::style::Theme::default())),
                active_theme_generation: Rc::new(Cell::new(1)),
                effect_phase: Rc::new(Cell::new(0)),
                contexts: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                context_generations: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                host_terminal_colors: Rc::new(Cell::new(None)),
                host_terminal_color_generation: Rc::new(Cell::new(0)),
                host_terminal_color_refresh_requested: Rc::new(Cell::new(false)),
                host_terminal_color_refresh_enabled: false,
                mouse_capture_generation: Rc::new(Cell::new(1)),
                memo_dependency_recorder: Rc::new(RefCell::new(None)),
                full_repaint: Rc::new(Cell::new(false)),
                devtools_request: Rc::new(RefCell::new(None)),
                ui_snapshot_request: Rc::new(RefCell::new(None)),
                command_chord_pending: Rc::new(Cell::new(false)),
            },
        });
        let mut host = HostState::default();

        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).key("a"))
            .child(crate::child::<Counter, _>(|| Counter, 2).key("b"))
            .into();

        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        assert_eq!(texts_in_order(&expanded1), vec!["1:0", "2:0"]);

        let key_a: Key = "a".into();
        let (id_a1, scope_a) = find_component_by_key(&registry, &key_a)
            .expect("expected a mounted component with key 'a'");

        assert!(
            registry
                .update_by_scope(scope_a, Box::new(Msg::Inc))
                .expect("update should succeed")
                .dirty
        );

        let epoch2 = registry.begin_epoch();
        let root2: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 2).key("b"))
            .child(crate::child::<Counter, _>(|| Counter, 1).key("a"))
            .into();

        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        let (id_a2, _) = find_component_by_key(&registry, &key_a)
            .expect("expected a mounted component with key 'a' after reorder");
        assert_eq!(id_a1, id_a2);

        assert_eq!(texts_in_order(&expanded2), vec!["2:0", "1:1"]);
    }

    #[test]
    fn state_keyed_component_survives_ancestor_reshaping() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        // Render 1: state-keyed Counter sits directly inside a VStack.
        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).component_state_key("modal"))
            .into();
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);
        assert_eq!(texts_in_order(&expanded1), vec!["1:0"]);

        let state_key: Key = "modal".into();
        let id1 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("state-keyed component should be registered");
        let scope1 = registry.arena.get(id1).scope;

        // Bump the component's state so we can observe whether it survives.
        assert!(
            registry
                .update_by_scope(scope1, Box::new(Msg::Inc))
                .expect("update should succeed")
                .dirty
        );

        // Render 2: wrap the Counter in an extra VStack. This changes its
        // resolved ContainerPath (an extra VStack segment appears above it),
        // which under pure path-based reuse would force a remount and reset
        // the Counter's state back to 0. The state key should prevent that.
        let epoch2 = registry.begin_epoch();
        let root2: Element = VStack::new()
            .child(
                VStack::new()
                    .child(crate::child::<Counter, _>(|| Counter, 1).component_state_key("modal")),
            )
            .into();
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(
            texts_in_order(&expanded2),
            vec!["1:1"],
            "state-keyed component should preserve its state across ancestor reshaping",
        );

        let id2 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("state-keyed component should still be registered");
        assert_eq!(
            id1, id2,
            "state-keyed component should reuse the same ComponentId",
        );
    }

    #[test]
    fn state_keyed_component_unregistered_when_removed() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).component_state_key("gone"))
            .into();
        let _ = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let state_key: Key = "gone".into();
        assert!(registry.state_key_index.contains_key(&state_key));

        // Render 2: component is no longer in the tree. After sweep, the
        // index entry should be dropped.
        let epoch2 = registry.begin_epoch();
        let root2: Element = VStack::new().into();
        let _ = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert!(
            !registry.state_key_index.contains_key(&state_key),
            "state key should be pruned once the component is unmounted",
        );
    }

    struct Parent;

    impl Component for Parent {
        type Message = ();
        type Properties = u32;
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            crate::child::<Counter, _>(|| Counter, ctx.props).key("leaf")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    fn find_component_by_parent_and_key(
        registry: &ComponentRegistry,
        parent: ComponentId,
        key: &Key,
    ) -> Option<(ComponentId, ScopeId)> {
        registry.arena.iter_active().find_map(|entry| {
            (entry.parent == Some(parent) && entry.key.as_ref() == Some(key))
                .then_some((entry.id, entry.scope))
        })
    }

    #[test]
    fn nested_components_are_scoped_per_parent() {
        let dispatcher = Dispatcher::new(|_, _| {});
        let (command_tx, _command_rx) = std::sync::mpsc::channel();
        let quit = Rc::new(Cell::new(false));
        let overlay_manager = Rc::new(RefCell::new(crate::overlay::OverlayManager::new()));

        let mut registry = ComponentRegistry::new(ComponentRegistryConfig {
            dispatcher,
            command_tx,
            env: RuntimeEnv {
                command_registry: CommandRegistry::default(),
                quit,
                focus: Rc::new(FocusContext::default()),
                hover: Rc::new(HoverContext::default()),
                scroll: Rc::new(ScrollContext::default()),
                animations: Rc::new(crate::animation::AnimationRegistry::default()),
                overlay_manager,
                focus_request: Rc::new(RefCell::new(None)),
                mouse_capture: Rc::new(Cell::new(true)),
                surface_mode: SurfaceMode::Fullscreen,
                transcript_history: Rc::new(RefCell::new(Vec::new())),
                pending_transcript_entries: Rc::new(RefCell::new(VecDeque::new())),
                clipboard: crate::clipboard::test_clipboard(),
                clipboard_config: crate::clipboard::ClipboardConfig::default(),
                active_theme: Rc::new(RefCell::new(crate::style::Theme::default())),
                active_theme_generation: Rc::new(Cell::new(1)),
                effect_phase: Rc::new(Cell::new(0)),
                contexts: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                context_generations: Rc::new(RefCell::new(rustc_hash::FxHashMap::default())),
                host_terminal_colors: Rc::new(Cell::new(None)),
                host_terminal_color_generation: Rc::new(Cell::new(0)),
                host_terminal_color_refresh_requested: Rc::new(Cell::new(false)),
                host_terminal_color_refresh_enabled: false,
                mouse_capture_generation: Rc::new(Cell::new(1)),
                memo_dependency_recorder: Rc::new(RefCell::new(None)),
                full_repaint: Rc::new(Cell::new(false)),
                devtools_request: Rc::new(RefCell::new(None)),
                ui_snapshot_request: Rc::new(RefCell::new(None)),
                command_chord_pending: Rc::new(Cell::new(false)),
            },
        });
        let mut host = HostState::default();

        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Parent, _>(|| Parent, 1).key("p1"))
            .child(crate::child::<Parent, _>(|| Parent, 2).key("p2"))
            .into();

        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        assert_eq!(texts_in_order(&expanded1), vec!["1:0", "2:0"]);

        let key_p1: Key = "p1".into();
        let key_leaf: Key = "leaf".into();

        let (p1_id1, _) = find_component_by_key(&registry, &key_p1)
            .expect("expected a mounted parent component with key 'p1'");
        let (leaf1_id1, leaf1_scope) =
            find_component_by_parent_and_key(&registry, p1_id1, &key_leaf)
                .expect("expected a mounted leaf component under 'p1'");

        assert!(
            registry
                .update_by_scope(leaf1_scope, Box::new(Msg::Inc))
                .expect("leaf update should succeed")
                .dirty
        );

        let epoch2 = registry.begin_epoch();
        let root2: Element = VStack::new()
            .child(crate::child::<Parent, _>(|| Parent, 2).key("p2"))
            .child(crate::child::<Parent, _>(|| Parent, 1).key("p1"))
            .into();

        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(texts_in_order(&expanded2), vec!["2:0", "1:1"]);

        let (p1_id2, _) = find_component_by_key(&registry, &key_p1)
            .expect("expected a mounted parent component with key 'p1' after reorder");
        assert_eq!(p1_id1, p1_id2);

        let (leaf1_id2, _) = find_component_by_parent_and_key(&registry, p1_id2, &key_leaf)
            .expect("expected a mounted leaf component under 'p1' after reorder");
        assert_eq!(leaf1_id1, leaf1_id2);
    }

    fn first_text_fg(element: &Element) -> Option<Color> {
        match &element.kind {
            ElementKind::Text(text) => text.style.fg.map(crate::style::Paint::color),
            ElementKind::Group(group) => first_text_fg(group.child.as_ref()),
            ElementKind::VStack(stack) => stack.children.iter().find_map(first_text_fg),
            ElementKind::Flow(flow) => flow.children.iter().find_map(first_text_fg),
            ElementKind::ThemeProvider(tp) => first_text_fg(&tp.child),
            ElementKind::ContextProvider(cp) => first_text_fg(&cp.child),
            _ => None,
        }
    }

    #[derive(Clone)]
    struct MemoLabel {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for MemoLabel {
        type Message = ();
        type Properties = &'static str;
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(hash_value(props))
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            Text::new(ctx.props).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[derive(Clone)]
    struct MemoCounterChild {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for MemoCounterChild {
        type Message = Msg;
        type Properties = ();
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn memo_key(&self, _props: &Self::Properties, ctx: &Context<Self>) -> Option<u64> {
            Some(ctx.state as u64)
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            Text::new(format!("child:{}", ctx.state)).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Inc => {
                    ctx.state += 1;
                    Update::full()
                }
            }
        }
    }

    #[derive(Clone)]
    struct MemoParentWithChild {
        parent_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
    }

    impl Component for MemoParentWithChild {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, _props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(1)
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.parent_view_count.set(self.parent_view_count.get() + 1);
            crate::child(
                {
                    let child_view_count = Rc::clone(&self.child_view_count);
                    move || MemoCounterChild {
                        view_count: Rc::clone(&child_view_count),
                    }
                },
                (),
            )
            .key("memo-child")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[derive(Clone)]
    struct ThemeAwareMemoLabel {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for ThemeAwareMemoLabel {
        type Message = ();
        type Properties = &'static str;
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(hash_value(props))
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            let style = Style::new().fg(ctx.theme().primary.fg.unwrap_or(Color::White.into()));
            Text::new(ctx.props).style(style).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct U32ContextLabel;

    impl Component for U32ContextLabel {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            let value = ctx.use_context::<u32>().unwrap_or(0);
            Text::new(format!("u32:{value}")).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct MultiContextLabel;

    impl Component for MultiContextLabel {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            let count = ctx.use_context::<u32>().unwrap_or_default();
            let compact = ctx.use_context::<bool>().unwrap_or(false);
            let title = ctx
                .use_context::<String>()
                .unwrap_or_else(|| "untitled".to_string());
            Text::new(format!("multi:{title}:{count}:{compact}")).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[derive(Clone)]
    struct ContextAwareMemo {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for ContextAwareMemo {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, _props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(1)
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            let value = ctx.use_context::<u32>().unwrap_or_default();
            Text::new(format!("ctx-memo:{value}")).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[derive(Clone)]
    struct ContextBlindMemo {
        view_count: Rc<Cell<usize>>,
    }

    impl Component for ContextBlindMemo {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, _props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(1)
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            Text::new("ctx-blind").into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn context_provider_makes_typed_value_visible_to_descendants() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch = registry.begin_epoch();
        let root: Element = ContextProvider::new(42u32)
            .child(crate::child::<U32ContextLabel, _>(|| U32ContextLabel, ()))
            .into();
        let expanded = registry.expand_in_host(&mut host, None, root, epoch, Rect::default());
        registry.sweep(epoch);

        assert_eq!(texts_in_order(&expanded), vec!["u32:42"]);
    }

    #[test]
    fn nearest_context_provider_shadows_outer_provider() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch = registry.begin_epoch();
        let root: Element = ContextProvider::new(1u32)
            .child(
                VStack::new()
                    .child(crate::child::<U32ContextLabel, _>(|| U32ContextLabel, ()))
                    .child(
                        ContextProvider::new(9u32)
                            .child(crate::child::<U32ContextLabel, _>(|| U32ContextLabel, ())),
                    ),
            )
            .into();
        let expanded = registry.expand_in_host(&mut host, None, root, epoch, Rect::default());
        registry.sweep(epoch);

        assert_eq!(texts_in_order(&expanded), vec!["u32:1", "u32:9"]);
    }

    #[test]
    fn context_dependencies_invalidate_memoized_component_when_value_changes() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));

        let epoch1 = registry.begin_epoch();
        let root1: Element = ContextProvider::new(3u32)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ContextAwareMemo {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    (),
                )
                .key("ctx-aware"),
            )
            .into();
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = ContextProvider::new(7u32)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ContextAwareMemo {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    (),
                )
                .key("ctx-aware"),
            )
            .into();
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(view_count.get(), 2);
        assert_eq!(texts_in_order(&expanded1), vec!["ctx-memo:3"]);
        assert_eq!(texts_in_order(&expanded2), vec!["ctx-memo:7"]);
    }

    #[test]
    fn memoized_component_without_context_dependency_is_retained() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));

        let epoch1 = registry.begin_epoch();
        let root1: Element = ContextProvider::new(10u32)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ContextBlindMemo {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    (),
                )
                .key("ctx-blind"),
            )
            .into();
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = ContextProvider::new(99u32)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ContextBlindMemo {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    (),
                )
                .key("ctx-blind"),
            )
            .into();
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(view_count.get(), 1);
        assert_eq!(texts_in_order(&expanded2), vec!["ctx-blind"]);
    }

    #[test]
    fn multiple_context_types_are_available_simultaneously() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch = registry.begin_epoch();
        let root: Element =
            ContextProvider::new("workspace".to_string())
                .child(
                    ContextProvider::new(8u32).child(ContextProvider::new(true).child(
                        crate::child::<MultiContextLabel, _>(|| MultiContextLabel, ()),
                    )),
                )
                .into();
        let expanded = registry.expand_in_host(&mut host, None, root, epoch, Rect::default());
        registry.sweep(epoch);

        assert_eq!(texts_in_order(&expanded), vec!["multi:workspace:8:true"]);
    }

    #[test]
    fn memoized_component_reuses_cached_view_when_inputs_match() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));

        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo",
        )
        .key("memo-root");
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo",
        )
        .key("memo-root");
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(view_count.get(), 1);
        assert_eq!(texts_in_order(&expanded1), vec!["memo"]);
        assert_eq!(texts_in_order(&expanded2), vec!["memo"]);
    }

    #[test]
    fn memo_element_hit_skips_builder_execution() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let build_count = Rc::new(Cell::new(0));

        let build_root = |build_count: Rc<Cell<usize>>| {
            Memo::new(11).build(move || {
                build_count.set(build_count.get() + 1);
                Text::new("memo-hit")
            })
        };

        let epoch1 = registry.begin_epoch();
        let root1: Element = build_root(Rc::clone(&build_count));
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = build_root(Rc::clone(&build_count));
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(build_count.get(), 1);
        assert_eq!(texts_in_order(&expanded1), vec!["memo-hit"]);
        assert_eq!(texts_in_order(&expanded2), vec!["memo-hit"]);
    }

    #[test]
    fn memo_element_miss_rebuilds_subtree() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let build_count = Rc::new(Cell::new(0));

        let build_root = |deps_hash: u64, build_count: Rc<Cell<usize>>| {
            Memo::new(deps_hash).build(move || {
                build_count.set(build_count.get() + 1);
                Text::new("memo-miss")
            })
        };

        let epoch1 = registry.begin_epoch();
        let root1: Element = build_root(11, Rc::clone(&build_count));
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = build_root(22, Rc::clone(&build_count));
        registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(build_count.get(), 2);
    }

    #[test]
    fn memo_element_refreshes_dirty_descendant_component_without_rebuild() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let build_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));

        let build_root = |build_count: Rc<Cell<usize>>, child_view_count: Rc<Cell<usize>>| {
            Memo::new(77).build(move || {
                build_count.set(build_count.get() + 1);
                crate::child(
                    {
                        let child_view_count = Rc::clone(&child_view_count);
                        move || MemoCounterChild {
                            view_count: Rc::clone(&child_view_count),
                        }
                    },
                    (),
                )
                .key("memo-node-child")
            })
        };

        let epoch1 = registry.begin_epoch();
        let root1: Element = build_root(Rc::clone(&build_count), Rc::clone(&child_view_count));
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let child_scope = find_component_by_key(&registry, &Key::from("memo-node-child"))
            .expect("expected mounted memo child")
            .1;
        assert!(
            registry
                .update_by_scope(child_scope, Box::new(Msg::Inc))
                .expect("child update should succeed")
                .dirty
        );

        let epoch2 = registry.begin_epoch();
        let root2: Element = build_root(Rc::clone(&build_count), Rc::clone(&child_view_count));
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(build_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);
        assert_eq!(texts_in_order(&expanded1), vec!["child:0"]);
        assert_eq!(texts_in_order(&expanded2), vec!["child:1"]);
    }

    #[test]
    fn memoized_parent_refreshes_dirty_child_without_rerendering_parent() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let parent_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));

        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let parent_view_count = Rc::clone(&parent_view_count);
                let child_view_count = Rc::clone(&child_view_count);
                move || MemoParentWithChild {
                    parent_view_count: Rc::clone(&parent_view_count),
                    child_view_count: Rc::clone(&child_view_count),
                }
            },
            (),
        )
        .key("memo-parent");
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let child_scope = find_component_by_key(&registry, &Key::from("memo-child"))
            .expect("expected mounted child")
            .1;
        assert!(
            registry
                .update_by_scope(child_scope, Box::new(Msg::Inc))
                .expect("child update should succeed")
                .dirty
        );

        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let parent_view_count = Rc::clone(&parent_view_count);
                let child_view_count = Rc::clone(&child_view_count);
                move || MemoParentWithChild {
                    parent_view_count: Rc::clone(&parent_view_count),
                    child_view_count: Rc::clone(&child_view_count),
                }
            },
            (),
        )
        .key("memo-parent");
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(parent_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);
        assert_eq!(texts_in_order(&expanded1), vec!["child:0"]);
        assert_eq!(texts_in_order(&expanded2), vec!["child:1"]);
    }

    #[test]
    fn refresh_scope_in_place_updates_memoized_ancestor_cache() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let parent_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));

        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let parent_view_count = Rc::clone(&parent_view_count);
                let child_view_count = Rc::clone(&child_view_count);
                move || MemoParentWithChild {
                    parent_view_count: Rc::clone(&parent_view_count),
                    child_view_count: Rc::clone(&child_view_count),
                }
            },
            (),
        )
        .key("memo-parent");
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let child_scope = find_component_by_key(&registry, &Key::from("memo-child"))
            .expect("expected mounted child")
            .1;
        assert!(
            registry
                .update_by_scope(child_scope, Box::new(Msg::Inc))
                .expect("child update should succeed")
                .dirty
        );
        let replacement = registry
            .refresh_scope_in_place(child_scope, Rect::default())
            .expect("expected scope refresh to succeed");
        let memo_parent_scope = find_component_by_key(&registry, &Key::from("memo-parent"))
            .expect("expected mounted memo parent")
            .1;
        assert_eq!(replacement.scope, memo_parent_scope);
        assert_eq!(texts_in_order(&replacement.expanded), vec!["child:1"]);

        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let parent_view_count = Rc::clone(&parent_view_count);
                let child_view_count = Rc::clone(&child_view_count);
                move || MemoParentWithChild {
                    parent_view_count: Rc::clone(&parent_view_count),
                    child_view_count: Rc::clone(&child_view_count),
                }
            },
            (),
        )
        .key("memo-parent");
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(parent_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);
        assert_eq!(texts_in_order(&expanded2), vec!["child:1"]);
    }

    #[test]
    fn theme_change_rethemes_memoized_child_without_rerendering_view() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));
        let theme_one = Theme::default().primary(Style::new().fg(Color::Red));
        let theme_two = Theme::default().primary(Style::new().fg(Color::Blue));

        let epoch1 = registry.begin_epoch();
        let root1: Element = ThemeProvider::new(theme_one.clone())
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || MemoLabel {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    "memo",
                )
                .key("memo-theme-child"),
            )
            .into();
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = ThemeProvider::new(theme_two.clone())
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || MemoLabel {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    "memo",
                )
                .key("memo-theme-child"),
            )
            .into();
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(view_count.get(), 1);
        // Theme defaults are no longer baked into the memoized element; the
        // active theme is reattached to realized nodes during reconciliation.
        assert_eq!(first_text_fg(&expanded1), None);
        assert_eq!(first_text_fg(&expanded2), None);
    }

    #[test]
    fn theme_dependency_invalidates_memoized_component() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));
        let theme_one = Theme::default().primary(Style::new().fg(Color::Red));
        let theme_two = Theme::default().primary(Style::new().fg(Color::Blue));

        let epoch1 = registry.begin_epoch();
        let root1: Element = ThemeProvider::new(theme_one)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ThemeAwareMemoLabel {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    "memo",
                )
                .key("theme-aware-child"),
            )
            .into();
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = ThemeProvider::new(theme_two)
            .child(
                crate::child(
                    {
                        let view_count = Rc::clone(&view_count);
                        move || ThemeAwareMemoLabel {
                            view_count: Rc::clone(&view_count),
                        }
                    },
                    "memo",
                )
                .key("theme-aware-child"),
            )
            .into();
        registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        assert_eq!(view_count.get(), 2);
    }

    /// Non-memoized wrapper that hosts a memoized child.
    #[derive(Clone)]
    struct NonMemoWrapper {
        wrapper_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
    }

    impl Component for NonMemoWrapper {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        // No memo_key - this component is NOT memoized.

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.wrapper_view_count
                .set(self.wrapper_view_count.get() + 1);
            crate::child(
                {
                    let child_view_count = Rc::clone(&self.child_view_count);
                    move || MemoCounterChild {
                        view_count: Rc::clone(&child_view_count),
                    }
                },
                (),
            )
            .key("inner-child")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    /// Memoized grandparent that hosts a non-memoized wrapper.
    #[derive(Clone)]
    struct MemoGrandparent {
        grandparent_view_count: Rc<Cell<usize>>,
        wrapper_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
    }

    impl Component for MemoGrandparent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn memo_key(&self, _props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
            Some(1)
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.grandparent_view_count
                .set(self.grandparent_view_count.get() + 1);
            crate::child(
                {
                    let wrapper_view_count = Rc::clone(&self.wrapper_view_count);
                    let child_view_count = Rc::clone(&self.child_view_count);
                    move || NonMemoWrapper {
                        wrapper_view_count: Rc::clone(&wrapper_view_count),
                        child_view_count: Rc::clone(&child_view_count),
                    }
                },
                (),
            )
            .key("wrapper")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn refresh_propagates_through_nonmemoized_intermediate_parent() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let grandparent_view_count = Rc::new(Cell::new(0));
        let wrapper_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));

        // First render: grandparent (memoized) → wrapper (not memoized) → child (memoized).
        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let gp = Rc::clone(&grandparent_view_count);
                let w = Rc::clone(&wrapper_view_count);
                let c = Rc::clone(&child_view_count);
                move || MemoGrandparent {
                    grandparent_view_count: Rc::clone(&gp),
                    wrapper_view_count: Rc::clone(&w),
                    child_view_count: Rc::clone(&c),
                }
            },
            (),
        )
        .key("grandparent");
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        assert_eq!(grandparent_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 1);

        // Mutate the inner memoized child.
        let child_scope = find_component_by_key(&registry, &Key::from("inner-child"))
            .expect("expected mounted inner child")
            .1;
        assert!(
            registry
                .update_by_scope(child_scope, Box::new(Msg::Inc))
                .expect("child update should succeed")
                .dirty
        );

        // In-place refresh of the child.
        let replacement = registry
            .refresh_scope_in_place(child_scope, Rect::default())
            .expect("expected scope refresh to succeed");
        let grandparent_scope = find_component_by_key(&registry, &Key::from("grandparent"))
            .expect("expected mounted grandparent")
            .1;
        assert_eq!(replacement.scope, grandparent_scope);
        assert_eq!(texts_in_order(&replacement.expanded), vec!["child:1"]);

        // Second render: grandparent should reuse its cache (which should now
        // contain the updated child text), without re-running grandparent's view.
        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let gp = Rc::clone(&grandparent_view_count);
                let w = Rc::clone(&wrapper_view_count);
                let c = Rc::clone(&child_view_count);
                move || MemoGrandparent {
                    grandparent_view_count: Rc::clone(&gp),
                    wrapper_view_count: Rc::clone(&w),
                    child_view_count: Rc::clone(&c),
                }
            },
            (),
        )
        .key("grandparent");
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        // Grandparent view should NOT have been called again.
        assert_eq!(grandparent_view_count.get(), 1);
        // Child was refreshed exactly once more (in-place).
        assert_eq!(child_view_count.get(), 2);
        // The final output must show the updated child text.
        assert_eq!(texts_in_order(&expanded2), vec!["child:1"]);
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn devtools_memo_counters_track_hits_and_misses() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));

        let _ = super::take_memo_frame_stats();

        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo",
        )
        .key("memo-root");
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo",
        )
        .key("memo-root");
        registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        let stats = super::take_memo_frame_stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        let cleared = super::take_memo_frame_stats();
        assert_eq!((cleared.hits, cleared.misses), (0, 0));
    }

    #[cfg(feature = "devtools")]
    #[test]
    fn devtools_memo_counters_count_prop_change_as_miss() {
        let mut registry = new_registry();
        let mut host = HostState::default();
        let view_count = Rc::new(Cell::new(0));

        let _ = super::take_memo_frame_stats();

        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo-a",
        )
        .key("memo-root");
        registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child(
            {
                let view_count = Rc::clone(&view_count);
                move || MemoLabel {
                    view_count: Rc::clone(&view_count),
                }
            },
            "memo-b",
        )
        .key("memo-root");
        registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        let stats = super::take_memo_frame_stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 2);
    }

    #[test]
    fn duplicate_state_key_siblings_last_writer_wins() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).component_state_key("dup"))
            .child(crate::child::<Counter, _>(|| Counter, 2).component_state_key("dup"))
            .into();
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);

        // Both render; the second sibling reuses (and overwrites props on) the
        // same state-keyed instance, so the tree captures both expanded results.
        assert_eq!(texts_in_order(&expanded1), vec!["1:0", "2:0"]);

        let state_key: Key = "dup".into();
        let id = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("state key should be registered");
        assert!(registry.is_valid(id));
    }

    struct BranchA;

    impl Component for BranchA {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::child::<Counter, _>(|| Counter, 1).component_state_key("shared")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct BranchB;

    impl Component for BranchB {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::child::<Counter, _>(|| Counter, 2).component_state_key("shared")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn duplicate_state_key_separate_branches_survive() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        // Epoch 1: render BranchA, which contains Counter with state_key "shared"
        let epoch1 = registry.begin_epoch();
        let root1: Element = crate::child::<BranchA, _>(|| BranchA, ());
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);
        assert_eq!(texts_in_order(&expanded1), vec!["1:0"]);

        let state_key: Key = "shared".into();
        let id1 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("registered");
        let scope1 = registry.arena.get(id1).scope;

        // Increment the counter so we can observe state survival.
        assert!(
            registry
                .update_by_scope(scope1, Box::new(Msg::Inc))
                .expect("ok")
                .dirty
        );

        // Epoch 2: render BranchB instead; Counter should reuse the same
        // instance because the state key is global.
        let epoch2 = registry.begin_epoch();
        let root2: Element = crate::child::<BranchB, _>(|| BranchB, ());
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);

        // Props changed from 1 to 2, but state (1) survived the branch switch.
        assert_eq!(texts_in_order(&expanded2), vec!["2:1"]);

        let id2 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("registered");
        assert_eq!(
            id1, id2,
            "same instance should be reused across unrelated branches"
        );
    }

    struct OtherCounter;

    impl Component for OtherCounter {
        type Message = ();
        type Properties = ();
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("other:{}", ctx.state)).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn duplicate_state_key_different_types_panic_or_warn() {
        let mut registry = new_registry();
        let mut host = HostState::default();

        let epoch1 = registry.begin_epoch();
        let root1: Element = VStack::new()
            .child(crate::child::<Counter, _>(|| Counter, 1).component_state_key("mixed"))
            .into();
        let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
        registry.sweep(epoch1);
        assert_eq!(texts_in_order(&expanded1), vec!["1:0"]);

        let state_key: Key = "mixed".into();
        let id1 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("registered");

        // Epoch 2: different component type with the same state key.
        // The type mismatch prevents reuse, so a fresh instance is created.
        let epoch2 = registry.begin_epoch();
        let root2: Element = VStack::new()
            .child(
                crate::child::<OtherCounter, _>(|| OtherCounter, ()).component_state_key("mixed"),
            )
            .into();
        let expanded2 = registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
        registry.sweep(epoch2);
        assert_eq!(texts_in_order(&expanded2), vec!["other:0"]);

        let id2 = registry
            .state_key_index
            .get(&state_key)
            .copied()
            .expect("registered");
        assert_ne!(
            id1, id2,
            "type mismatch should create a new instance rather than reuse"
        );
    }
}
