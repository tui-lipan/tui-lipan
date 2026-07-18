use rustc_hash::{FxHashMap, FxHashSet};
use std::any::TypeId;
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::sync::Arc;

use crate::app::context::SurfaceMode;
use crate::app::input::command_registry::CommandEntry;
use crate::app::input::command_registry::CommandRegistry;
use crate::callback::{CancellationToken, CommandLink, CommandTx, Dispatcher, Link, ScopeId};
use crate::core::context_value::ContextValue;
use crate::core::element::{Element, Key};
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::core::runtime_env::{
    DevToolsRequest, MemoDependency, MemoDependencySnapshot, RuntimeEnv, ScrollDependency,
    ScrollDependencyKind, ScrollIdentity, TranscriptEntry,
};
use crate::runtime::FocusRequest;
use crate::style::{HostTerminalColors, Rect, RichText, Theme, ThemeExtension};

/// Side-effect command returned from `Component::update`.
///
/// A `Command` represents work that should happen *outside* the synchronous `update` call
/// (HTTP/IO/timers), typically executed by the runtime and eventually producing more messages.
#[non_exhaustive]
pub struct Command {
    action: Box<dyn CommandAction>,
}

/// Coalescing behavior for keyed background tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskPolicy {
    /// Always enqueue every task.
    QueueAll,
    /// If a task with the same key is already active, drop the new one.
    DropIfRunning,
    /// Keep only the latest pending task for the key.
    LatestOnly,
}

mod task_policy;

use task_policy::Task;

#[cfg(not(target_arch = "wasm32"))]
mod executor_native;
#[cfg(target_arch = "wasm32")]
mod executor_wasm;

#[cfg(not(target_arch = "wasm32"))]
use executor_native::TaskExecutor;
#[cfg(target_arch = "wasm32")]
use executor_wasm::TaskExecutor;

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Command").finish_non_exhaustive()
    }
}

impl Command {
    /// Run a one-shot action on the UI thread.
    pub fn new(action: impl FnOnce() + 'static) -> Self {
        Self {
            action: Box::new(RunAction(Some(action))),
        }
    }

    /// Spawn a background task that can send messages back.
    pub fn spawn<Msg, F>(f: F) -> Self
    where
        Msg: Send + 'static,
        F: FnOnce(CommandLink<Msg>) + Send + 'static,
    {
        Self {
            action: Box::new(SpawnAction::<Msg, F> {
                f: Some(f),
                _marker: PhantomData,
            }),
        }
    }

    /// Spawn a keyed background task with explicit coalescing policy.
    pub fn spawn_keyed<Msg, F>(key: impl Into<Arc<str>>, policy: TaskPolicy, f: F) -> Self
    where
        Msg: Send + 'static,
        F: FnOnce(CommandLink<Msg>) + Send + 'static,
    {
        Self {
            action: Box::new(SpawnKeyedAction::<Msg, F> {
                key: key.into(),
                policy,
                f: Some(f),
                _marker: PhantomData,
            }),
        }
    }

    pub(crate) fn run(self, runtime: CommandRuntime) {
        self.action.run(runtime);
    }
}

pub(crate) struct CommandRuntime {
    pub(crate) scope: ScopeId,
    pub(crate) tx: CommandTx,
}

trait CommandAction {
    fn run(self: Box<Self>, runtime: CommandRuntime);
}

struct RunAction<F>(Option<F>);

impl<F> CommandAction for RunAction<F>
where
    F: FnOnce() + 'static,
{
    fn run(mut self: Box<Self>, _runtime: CommandRuntime) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

struct SpawnAction<Msg, F> {
    f: Option<F>,
    _marker: PhantomData<fn(Msg)>,
}

struct SpawnKeyedAction<Msg, F> {
    key: Arc<str>,
    policy: TaskPolicy,
    f: Option<F>,
    _marker: PhantomData<fn(Msg)>,
}

impl<Msg, F> CommandAction for SpawnAction<Msg, F>
where
    Msg: Send + 'static,
    F: FnOnce(CommandLink<Msg>) + Send + 'static,
{
    fn run(mut self: Box<Self>, runtime: CommandRuntime) {
        let Some(f) = self.f.take() else {
            return;
        };

        let token = CancellationToken::default();
        let link = CommandLink::new(runtime.scope, runtime.tx, token.clone());
        TaskExecutor::global().execute(Task::with_token(move || f(link), token));
    }
}

impl<Msg, F> CommandAction for SpawnKeyedAction<Msg, F>
where
    Msg: Send + 'static,
    F: FnOnce(CommandLink<Msg>) + Send + 'static,
{
    fn run(mut self: Box<Self>, runtime: CommandRuntime) {
        let Some(f) = self.f.take() else {
            return;
        };

        let key = Arc::clone(&self.key);
        let policy = self.policy;
        let token = CancellationToken::default();
        let link = CommandLink::new(runtime.scope, runtime.tx, token.clone());
        TaskExecutor::global().execute_keyed(key, policy, Task::with_token(move || f(link), token));
    }
}

impl<Msg: 'static> Link<Msg> {
    /// Create a background `Command` that can send messages back to this component.
    pub fn command<F>(&self, f: F) -> Command
    where
        Msg: Send + 'static,
        F: FnOnce(CommandLink<Msg>) + Send + 'static,
    {
        Command::spawn::<Msg, F>(f)
    }

    /// Create a keyed background `Command` with coalescing policy.
    pub fn command_keyed<F>(&self, key: impl Into<Arc<str>>, policy: TaskPolicy, f: F) -> Command
    where
        Msg: Send + 'static,
        F: FnOnce(CommandLink<Msg>) + Send + 'static,
    {
        Command::spawn_keyed::<Msg, F>(key, policy, f)
    }
}

/// Result from a component's `update()` method.
///
/// `level` refines the minimum refresh work needed by the runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum UpdateLevel {
    #[default]
    None,
    Paint,
    Layout,
    Full,
}

/// Component update result with optional side-effect command.
pub struct Update {
    /// Whether the component's view needs re-rendering.
    pub dirty: bool,
    /// Granularity of the requested refresh.
    pub(crate) level: UpdateLevel,
    /// An optional async command to execute.
    pub command: Option<Command>,
}

impl Update {
    /// Request a paint-only update.
    pub fn paint() -> Self {
        Self {
            dirty: true,
            level: UpdateLevel::Paint,
            command: None,
        }
    }

    /// Request a layout reconcile update.
    pub fn layout() -> Self {
        Self {
            dirty: true,
            level: UpdateLevel::Layout,
            command: None,
        }
    }

    /// Request a layout reconcile and optionally run a command.
    pub fn layout_with_command(command: impl Into<Option<Command>>) -> Self {
        match command.into() {
            Some(command) => Self {
                dirty: true,
                level: UpdateLevel::Layout,
                command: Some(command),
            },
            None => Self::layout(),
        }
    }

    /// Request a full update.
    pub fn full() -> Self {
        Self {
            dirty: true,
            level: UpdateLevel::Full,
            command: None,
        }
    }

    /// Run a command without marking the component dirty (e.g. background fetch with a follow-up message).
    pub fn command_only(command: Command) -> Self {
        Self {
            dirty: false,
            level: UpdateLevel::None,
            command: Some(command),
        }
    }

    /// Nothing changed, no command.
    pub fn none() -> Self {
        Self {
            dirty: false,
            level: UpdateLevel::None,
            command: None,
        }
    }

    /// Full refresh, and optionally run a command when [`Some`]; when [`None`], same as [`Self::full`].
    pub fn with_command(command: impl Into<Option<Command>>) -> Self {
        match command.into() {
            Some(cmd) => Self {
                dirty: true,
                level: UpdateLevel::Full,
                command: Some(cmd),
            },
            None => Self::full(),
        }
    }

    pub(crate) fn level(&self) -> UpdateLevel {
        self.level
    }
}

/// The return type of `Component::on_key`.
///
/// `handled` is tracked separately from `dirty` and `command`.
pub struct KeyUpdate {
    /// Whether the key event was handled.
    pub handled: bool,
    /// State change and side-effect work, same as `Update`.
    pub update: Update,
}

impl KeyUpdate {
    /// Mark the key event as handled.
    pub fn handled(update: Update) -> Self {
        Self {
            handled: true,
            update,
        }
    }

    /// Mark the key event as unhandled.
    pub fn unhandled(update: Update) -> Self {
        Self {
            handled: false,
            update,
        }
    }
}

/// A stateful, reusable UI component.
///
/// Components own their dependencies via the struct instance (dependency injection),
/// while all UI state mutations happen through `update()` via `Context`.
pub trait Component: Sized + 'static {
    /// Messages (events) that can be sent to this component.
    type Message: 'static;

    /// Properties passed from the parent.
    type Properties: Clone + PartialEq + 'static;

    /// Local state owned by the runtime.
    type State: 'static;

    /// Create the initial state for this component.
    fn create_state(&self, props: &Self::Properties) -> Self::State;

    /// Stable memo key used to retain this component's previously expanded subtree.
    ///
    /// Return `Some(key)` to opt into retained subtree reuse. When the key is unchanged,
    /// the runtime may skip `view()` and reuse the prior expanded subtree until local state,
    /// props, or observed context dependencies require a refresh.
    fn memo_key(&self, _props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
        None
    }

    /// Called once when the component is first mounted.
    ///
    /// This is the right place to kick off background work (HTTP/IO) by returning a `Command`.
    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        None
    }

    /// Declarative UI definition.
    fn view(&self, ctx: &Context<Self>) -> Element;

    /// Handle a keyboard event that was not handled by the focused node.
    ///
    /// This enables global shortcuts (e.g. `Ctrl+S`) without attaching handlers to every widget.
    /// Return `KeyUpdate::handled` to stop bubbling.
    fn on_key(&mut self, _key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
        KeyUpdate::unhandled(Update::none())
    }

    /// Update state in response to a message.
    ///
    /// Returns `(dirty, command)`.
    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update;

    /// Called when properties have changed.
    ///
    /// Returns `(dirty, command)`.
    fn on_props_changed(
        &mut self,
        _old_props: &Self::Properties,
        _ctx: &mut Context<Self>,
    ) -> Update {
        Update::none()
    }

    /// Called once when the component is being unmounted.
    fn unmount(&mut self, _ctx: &mut Context<Self>) {}
}

/// Simple width breakpoint derived from the current viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breakpoint {
    /// Narrow terminal / compact layout.
    Small,
    /// Medium-width terminal.
    Medium,
    /// Wide terminal.
    Large,
}

/// Resolved scrollbar visibility for a keyed [`TextArea`](crate::widgets::TextArea).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarVisibility {
    /// Whether the vertical scrollbar is visible.
    pub v: bool,
    /// Whether the horizontal scrollbar is visible.
    pub h: bool,
}

/// Shared ancestor-walk state used by both [`FocusContext`] and [`HoverContext`].
#[derive(Default)]
struct NodeChainState {
    current_node: Cell<Option<NodeId>>,
    chain_scopes: RefCell<Vec<ScopeId>>,
    chain_keys: RefCell<Vec<Key>>,
    generation: Cell<u64>,
}

impl NodeChainState {
    fn node_id(&self) -> Option<NodeId> {
        self.current_node.get()
    }

    fn has_within_scope(&self, scope: ScopeId) -> bool {
        self.chain_scopes.borrow().contains(&scope)
    }

    fn has_within_key(&self, key: &Key) -> bool {
        self.chain_keys
            .borrow()
            .iter()
            .any(|candidate| candidate == key)
    }

    fn push_scope_if_missing(scopes: &mut Vec<ScopeId>, scope: ScopeId) {
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    fn push_key_if_missing(keys: &mut Vec<Key>, key: &Key) {
        if !keys.iter().any(|candidate| candidate == key) {
            keys.push(key.clone());
        }
    }

    fn replace_snapshot(&self, node: Option<NodeId>, scopes: Vec<ScopeId>, keys: Vec<Key>) {
        let mut scope_ref = self.chain_scopes.borrow_mut();
        let mut key_ref = self.chain_keys.borrow_mut();
        let changed = self.current_node.get() != node || *scope_ref != scopes || *key_ref != keys;

        self.current_node.set(node);
        *scope_ref = scopes;
        *key_ref = keys;

        if changed {
            self.generation
                .set(self.generation.get().wrapping_add(1).max(1));
        }
    }

    /// Walk the ancestor chain from `node` upward, populating scopes and keys.
    fn update_chain(&self, tree: &NodeTree, node: Option<NodeId>) {
        let mut scopes = Vec::new();
        let mut keys = Vec::new();

        if let Some(mut cur) = node {
            Self::push_scope_if_missing(&mut scopes, ScopeId(1));

            loop {
                if !tree.is_valid(cur) {
                    break;
                }

                let node_ref = tree.node(cur);

                if let Some(k) = &node_ref.key {
                    Self::push_key_if_missing(&mut keys, k);
                }

                if let NodeKind::Group(group) = &node_ref.kind {
                    Self::push_scope_if_missing(&mut scopes, group.scope);
                }

                let Some(parent) = node_ref.parent else {
                    break;
                };
                cur = parent;
            }
        }

        self.replace_snapshot(node, scopes, keys);
    }

    fn generation(&self) -> u64 {
        self.generation.get()
    }
}

/// Shared render-time focus information from the previous frame.
#[derive(Default)]
pub(crate) struct FocusContext {
    inner: NodeChainState,
}

impl FocusContext {
    pub(crate) fn update_from_tree(
        &self,
        tree: &NodeTree,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
    ) {
        let mut cur = focused.filter(|id| tree.is_valid(*id));

        if cur.is_none()
            && let Some(key) = focused_key
        {
            if let Some(id) = tree
                .iter()
                .find(|n| n.key.as_ref() == Some(key))
                .map(|n| n.id)
            {
                cur = Some(id);
            } else {
                self.inner
                    .replace_snapshot(None, Vec::new(), vec![key.clone()]);
                return;
            }
        }

        self.inner.update_chain(tree, cur);
    }

    pub(crate) fn focused_node_id(&self) -> Option<NodeId> {
        self.inner.node_id()
    }

    pub(crate) fn has_focus_within_scope(&self, scope: ScopeId) -> bool {
        self.inner.has_within_scope(scope)
    }

    pub(crate) fn has_focus_within_key(&self, key: &Key) -> bool {
        self.inner.has_within_key(key)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.inner.generation()
    }
}

/// Shared render-time hover information from the previous frame.
#[derive(Default)]
pub(crate) struct HoverContext {
    inner: NodeChainState,
}

impl HoverContext {
    pub(crate) fn update_from_tree(&self, tree: &NodeTree, hovered: Option<NodeId>) {
        let cur = hovered.filter(|id| tree.is_valid(*id));
        self.inner.update_chain(tree, cur);
    }

    pub(crate) fn hovered_node_id(&self) -> Option<NodeId> {
        self.inner.node_id()
    }

    pub(crate) fn has_hover_within_scope(&self, scope: ScopeId) -> bool {
        self.inner.has_within_scope(scope)
    }

    pub(crate) fn has_hover_within_key(&self, key: &Key) -> bool {
        self.inner.has_within_key(key)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.inner.generation()
    }
}

/// Shared render-time scrollable information from the previous frame.
#[derive(Default)]
pub(crate) struct ScrollContext {
    by_key: RefCell<FxHashMap<ScrollIdentity, ScrollbarVisibility>>,
    text_area_metrics_by_key: RefCell<FxHashMap<ScrollIdentity, crate::widgets::TextAreaMetrics>>,
    metrics_generations: RefCell<FxHashMap<ScrollIdentity, u64>>,
    scrollbar_generations: RefCell<FxHashMap<ScrollIdentity, u64>>,
    metrics_view_dependencies: RefCell<FxHashSet<ScrollIdentity>>,
    scrollbar_view_dependencies: RefCell<FxHashSet<ScrollIdentity>>,
}

/// Snapshot of [`ScrollContext`] generations taken before a reconcile pass,
/// used to decide whether cached views became stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScrollGenerations {
    metrics: FxHashMap<ScrollIdentity, u64>,
    scrollbars: FxHashMap<ScrollIdentity, u64>,
}

impl ScrollContext {
    pub(crate) fn update_from_tree(&self, tree: &NodeTree) {
        let mut map = self.by_key.borrow_mut();
        let prev = std::mem::take(&mut *map);
        let mut metrics_map = self.text_area_metrics_by_key.borrow_mut();
        let prev_metrics = std::mem::take(&mut *metrics_map);

        for node in tree.iter_with_overlays() {
            if let (Some(key), NodeKind::TextArea(text_area)) = (&node.key, &node.kind) {
                let identity = ScrollIdentity {
                    scope: node_scope(tree, node.id),
                    key: key.clone(),
                };
                let metrics = text_area.metrics(node.rect);
                map.insert(identity.clone(), metrics.scrollbars);
                metrics_map.insert(identity, metrics);
            }
        }

        advance_changed_generations(&prev, &map, &self.scrollbar_generations);
        advance_changed_generations(&prev_metrics, &metrics_map, &self.metrics_generations);
    }

    pub(crate) fn get(&self, identity: &ScrollIdentity) -> Option<ScrollbarVisibility> {
        self.by_key.borrow().get(identity).copied()
    }

    pub(crate) fn text_area_metrics(
        &self,
        identity: &ScrollIdentity,
    ) -> Option<crate::widgets::TextAreaMetrics> {
        self.text_area_metrics_by_key
            .borrow()
            .get(identity)
            .cloned()
    }

    pub(crate) fn begin_view(&self, scope: ScopeId) {
        self.metrics_view_dependencies
            .borrow_mut()
            .retain(|identity| identity.scope != scope);
        self.scrollbar_view_dependencies
            .borrow_mut()
            .retain(|identity| identity.scope != scope);
    }

    pub(crate) fn remove_scope(&self, scope: ScopeId) {
        self.begin_view(scope);
        self.by_key
            .borrow_mut()
            .retain(|identity, _| identity.scope != scope);
        self.text_area_metrics_by_key
            .borrow_mut()
            .retain(|identity, _| identity.scope != scope);
        self.metrics_generations
            .borrow_mut()
            .retain(|identity, _| identity.scope != scope);
        self.scrollbar_generations
            .borrow_mut()
            .retain(|identity, _| identity.scope != scope);
    }

    pub(crate) fn mark_view_dependency(&self, dependency: &ScrollDependency) {
        match dependency.kind {
            ScrollDependencyKind::Metrics => {
                self.metrics_view_dependencies
                    .borrow_mut()
                    .insert(dependency.identity.clone());
            }
            ScrollDependencyKind::Scrollbars => {
                self.scrollbar_view_dependencies
                    .borrow_mut()
                    .insert(dependency.identity.clone());
            }
        }
    }

    pub(crate) fn dependency_generation(&self, dependency: &ScrollDependency) -> u64 {
        let generations = match dependency.kind {
            ScrollDependencyKind::Metrics => &self.metrics_generations,
            ScrollDependencyKind::Scrollbars => &self.scrollbar_generations,
        };
        generations
            .borrow()
            .get(&dependency.identity)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn view_generations(&self) -> ScrollGenerations {
        ScrollGenerations {
            metrics: self.metrics_generations.borrow().clone(),
            scrollbars: self.scrollbar_generations.borrow().clone(),
        }
    }

    /// Whether a change since `prev` can affect the output of any cached
    /// `view()`, given which accessors views have actually used.
    pub(crate) fn view_dependencies_stale(&self, prev: &ScrollGenerations) -> bool {
        self.metrics_view_dependencies
            .borrow()
            .iter()
            .any(|identity| {
                self.metrics_generations
                    .borrow()
                    .get(identity)
                    .copied()
                    .unwrap_or(0)
                    != prev.metrics.get(identity).copied().unwrap_or(0)
            })
            || self
                .scrollbar_view_dependencies
                .borrow()
                .iter()
                .any(|identity| {
                    self.scrollbar_generations
                        .borrow()
                        .get(identity)
                        .copied()
                        .unwrap_or(0)
                        != prev.scrollbars.get(identity).copied().unwrap_or(0)
                })
    }
}

fn node_scope(tree: &NodeTree, mut id: NodeId) -> ScopeId {
    loop {
        let node = tree.node(id);
        if let NodeKind::Group(group) = &node.kind {
            return group.scope;
        }
        let Some(parent) = node.parent.filter(|parent| tree.is_valid(*parent)) else {
            return ScopeId(1);
        };
        id = parent;
    }
}

fn advance_changed_generations<T: PartialEq>(
    previous: &FxHashMap<ScrollIdentity, T>,
    current: &FxHashMap<ScrollIdentity, T>,
    generations: &RefCell<FxHashMap<ScrollIdentity, u64>>,
) {
    let identities: FxHashSet<_> = previous.keys().chain(current.keys()).cloned().collect();
    let mut generations = generations.borrow_mut();
    for identity in identities {
        if previous.get(&identity) != current.get(&identity) {
            let generation = generations.entry(identity).or_default();
            *generation = generation.wrapping_add(1).max(1);
        }
    }
    generations.retain(|identity, _| current.contains_key(identity));
}

/// Per-component runtime context.
pub struct Context<C: Component> {
    /// Component-local state.
    pub state: C::State,

    /// Component properties.
    pub props: C::Properties,

    viewport: Rect,
    link: Link<C::Message>,
    env: RuntimeEnv,
    scope: ScopeId,
}

impl<C: Component> Context<C> {
    pub(crate) fn new(
        component: &C,
        scope: ScopeId,
        dispatcher: Dispatcher,
        props: C::Properties,
        env: RuntimeEnv,
        viewport: Rect,
    ) -> Self {
        let state = component.create_state(&props);
        Self {
            state,
            props,
            viewport,
            link: Link::new(scope, dispatcher),
            env,
            scope,
        }
    }

    /// Link used to create callbacks.
    pub fn link(&self) -> &Link<C::Message> {
        &self.link
    }

    /// Access the toast notification API.
    pub fn toast(&self) -> crate::overlay::ToastHandle {
        crate::overlay::ToastHandle::new(self.env.overlay_manager.clone())
    }

    /// Access the clipboard API.
    pub fn clipboard(&self) -> crate::clipboard::ClipboardHandle {
        crate::clipboard::ClipboardHandle::new(
            self.env.clipboard.clone(),
            self.env.clipboard_config.clone(),
        )
    }

    /// Access the command registry API.
    pub fn command_registry(&self) -> CommandRegistry {
        self.env.command_registry.clone()
    }

    /// Check if an app command chord is currently pending.
    pub fn command_chord_pending(&self) -> bool {
        self.env.command_chord_pending.get()
    }

    /// Register a command scoped to this component instance.
    pub fn register_command(&self, entry: CommandEntry) {
        self.env
            .command_registry
            .register_for_scope(self.scope, entry);
    }

    /// Current viewport bounds (content area) for this render.
    pub fn viewport(&self) -> Rect {
        self.env.note_memo_dependency(MemoDependency::Viewport);
        self.viewport
    }

    /// Returns the active theme for this component's subtree.
    pub fn theme(&self) -> Theme {
        self.env.note_memo_dependency(MemoDependency::Theme);
        self.env.active_theme.borrow().clone()
    }

    /// Returns a cloned typed theme extension from the active theme.
    pub fn theme_extension<T>(&self) -> Option<T>
    where
        T: ThemeExtension,
    {
        self.env.note_memo_dependency(MemoDependency::Theme);
        self.env.active_theme.borrow().extension_cloned::<T>()
    }

    /// Returns a cloned typed value from the nearest active `ContextProvider<T>`.
    pub fn use_context<T>(&self) -> Option<T>
    where
        T: ContextValue,
    {
        self.env
            .note_memo_dependency(MemoDependency::Context(TypeId::of::<T>()));
        self.env
            .contexts
            .borrow()
            .get(&TypeId::of::<T>())
            .and_then(|value| value.as_ref().downcast_ref::<T>())
            .cloned()
    }

    /// Returns a cloned typed value from the nearest active `ContextProvider<T>`.
    pub fn context<T>(&self) -> Option<T>
    where
        T: ContextValue,
    {
        self.use_context::<T>()
    }

    /// Returns `true` when the app runs in inline viewport mode.
    pub fn is_inline(&self) -> bool {
        self.env.surface_mode.is_inline()
    }

    /// Returns the app surface mode.
    pub fn surface_mode(&self) -> SurfaceMode {
        self.env.surface_mode
    }

    /// Returns the current renderer animation phase used by built-in visual effects.
    ///
    /// Capture this value when starting a one-shot phase-based effect such as
    /// [`VisualEffect::centered_burst_ripple`](crate::style::VisualEffect::centered_burst_ripple).
    pub fn effect_phase(&self) -> u64 {
        self.env.effect_phase.get()
    }

    /// Return the latest host terminal colors known to the runner.
    ///
    /// Values are available after `App::system_theme()` or
    /// `App::live_host_terminal_colors(true)` is enabled and the runner completes
    /// at least one successful OSC 4/10/11 probe. The cache is updated on the UI
    /// thread, not by app background tasks.
    pub fn host_terminal_colors(&self) -> Option<HostTerminalColors> {
        self.env.host_terminal_colors()
    }

    /// Return the generation for the cached host terminal colors.
    ///
    /// Starts at `0` and increments whenever the runner observes a different
    /// host terminal palette. Components can compare this value to decide when
    /// to rebuild app-specific theme tokens.
    pub fn host_terminal_color_generation(&self) -> u64 {
        self.env.host_terminal_color_generation()
    }

    /// Ask the runner to refresh host terminal colors on the UI thread.
    ///
    /// This is a no-op unless `App::system_theme()` or
    /// `App::live_host_terminal_colors(true)` was enabled. Those app settings
    /// also refresh on focus gained. The actual OSC query is performed later by
    /// the runner while coordinating with its input reader, so apps should call
    /// this instead of polling
    /// [`query_host_colors`](crate::style::query_host_colors) from background
    /// threads.
    pub fn request_host_terminal_color_refresh(&self) {
        self.env.request_host_terminal_color_refresh();
    }

    /// Returns whether terminal mouse capture is currently enabled.
    pub fn mouse_capture_enabled(&self) -> bool {
        self.env.note_memo_dependency(MemoDependency::MouseCapture);
        self.env.mouse_capture.get()
    }

    /// Enable or disable terminal mouse capture at runtime.
    pub fn set_mouse_capture(&self, enabled: bool) {
        if self.env.mouse_capture.get() != enabled {
            self.env.mouse_capture.set(enabled);
            self.env.mouse_capture_generation.set(
                self.env
                    .mouse_capture_generation
                    .get()
                    .wrapping_add(1)
                    .max(1),
            );
        }
    }

    /// Toggle terminal mouse capture at runtime and return the new state.
    pub fn toggle_mouse_capture(&self) -> bool {
        let next = !self.env.mouse_capture.get();
        self.set_mouse_capture(next);
        next
    }

    /// Append plain rich-text lines to transcript history above the inline viewport.
    ///
    /// This is a no-op outside inline transcript mode.
    pub fn append_transcript_lines<I, L>(&mut self, lines: I)
    where
        I: IntoIterator<Item = L>,
        L: Into<RichText>,
    {
        if !matches!(self.env.surface_mode, SurfaceMode::InlineTranscript { .. }) {
            return;
        }

        let lines: Vec<RichText> = lines.into_iter().map(Into::into).collect();
        if lines.is_empty() {
            return;
        }

        self.env
            .transcript_history
            .borrow_mut()
            .push(TranscriptEntry::Lines(lines.clone()));
        self.env
            .pending_transcript_entries
            .borrow_mut()
            .push_back(TranscriptEntry::Lines(lines));
    }

    /// Append a rendered element to transcript history above the inline viewport.
    ///
    /// This is a no-op outside inline transcript mode. The appended subtree must
    /// already be expanded: this API accepts widget trees, not `Component` elements.
    pub fn append_transcript_element(&mut self, element: impl Into<Element>) {
        if !matches!(self.env.surface_mode, SurfaceMode::InlineTranscript { .. }) {
            return;
        }

        let element = element.into();
        if element.contains_unexpanded_component() {
            crate::debug::internal_log!(
                "[tui-lipan] append_transcript_element ignored an element containing Component nodes"
            );
            return;
        }

        self.env
            .transcript_history
            .borrow_mut()
            .push(TranscriptEntry::Element(Box::new(element.clone())));
        self.env
            .pending_transcript_entries
            .borrow_mut()
            .push_back(TranscriptEntry::Element(Box::new(element)));
    }

    /// Returns `true` if the currently focused node (from the previous frame) is inside this
    /// component's subtree.
    pub fn has_focus_within(&self) -> bool {
        self.env.note_memo_dependency(MemoDependency::Focus);
        self.env.focus.has_focus_within_scope(self.scope)
    }

    /// Returns `true` if the currently focused node (from the previous frame) is inside the
    /// subtree of the element identified by `key`.
    pub fn has_focus_within_key(&self, key: impl Into<Key>) -> bool {
        let key = key.into();
        self.env.note_memo_dependency(MemoDependency::Focus);
        self.env.focus.has_focus_within_key(&key)
    }

    /// Returns resolved scrollbar visibility for the keyed `TextArea` from the previous frame.
    ///
    /// The `TextArea` element must have an `Element::key`; missing or first-frame entries return
    /// `ScrollbarVisibility::default()`.
    pub fn text_area_scrollbars(&self, key: impl Into<Key>) -> ScrollbarVisibility {
        let dependency = ScrollDependency {
            identity: ScrollIdentity {
                scope: self.scope,
                key: key.into(),
            },
            kind: ScrollDependencyKind::Scrollbars,
        };
        self.env
            .note_memo_dependency(MemoDependency::Scroll(dependency.clone()));
        self.env.scroll.mark_view_dependency(&dependency);
        self.env
            .scroll
            .text_area_metrics(&dependency.identity)
            .map(|metrics| metrics.scrollbars)
            .or_else(|| self.env.scroll.get(&dependency.identity))
            .unwrap_or_default()
    }

    /// Returns previous-frame metrics for a keyed `TextArea`.
    pub fn text_area_metrics(
        &self,
        key: impl Into<Key>,
    ) -> Option<crate::widgets::TextAreaMetrics> {
        let dependency = ScrollDependency {
            identity: ScrollIdentity {
                scope: self.scope,
                key: key.into(),
            },
            kind: ScrollDependencyKind::Metrics,
        };
        self.env
            .note_memo_dependency(MemoDependency::Scroll(dependency.clone()));
        self.env.scroll.mark_view_dependency(&dependency);
        self.env.scroll.text_area_metrics(&dependency.identity)
    }

    /// Returns `true` if the hovered node (from the previous frame) is inside this
    /// component's subtree.
    pub fn has_hover_within(&self) -> bool {
        self.env.note_memo_dependency(MemoDependency::Hover);
        self.env.hover.has_hover_within_scope(self.scope)
    }

    /// Returns `true` if the hovered node (from the previous frame) is inside the
    /// subtree of the element identified by `key`.
    pub fn has_hover_within_key(&self, key: impl Into<Key>) -> bool {
        let key = key.into();
        self.env.note_memo_dependency(MemoDependency::Hover);
        self.env.hover.has_hover_within_key(&key)
    }

    /// Returns the focused node id from the previous frame, if any.
    pub fn focused_node_id(&self) -> Option<NodeId> {
        self.env.note_memo_dependency(MemoDependency::Focus);
        self.env.focus.focused_node_id()
    }

    /// Returns the hovered node id from the previous frame, if any.
    pub fn hovered_node_id(&self) -> Option<NodeId> {
        self.env.note_memo_dependency(MemoDependency::Hover);
        self.env.hover.hovered_node_id()
    }

    /// Property-scoped transition for a single style value.
    ///
    /// Pass the desired *final* `target` each frame. The first call for a given
    /// `key` records the target as the resting value. When `target` differs
    /// from the previously stored target, a transition starts from the current
    /// value to the new target using `config`. Returns the current interpolated
    /// value for this frame — embed it directly in a `Style` slot.
    ///
    /// Animations are driven by the runtime: while a transition is in flight,
    /// the component re-renders each animation tick (~16 ms) so the new value
    /// flows into the style. Keys not read during a frame are dropped, so
    /// transitions for hidden elements are automatically cleaned up.
    ///
    /// ```ignore
    /// let edge_fg = ctx.transition(
    ///     "prompt-edge",
    ///     if focused { theme.primary } else { theme.muted },
    ///     TransitionConfig::default(),
    /// );
    /// EdgeDecoration::new(Edge::Left).style(Style::new().fg(edge_fg))
    /// ```
    ///
    /// # Panics
    /// Panics if the same `key` is used with two different value types.
    pub fn transition<T>(
        &self,
        key: impl Into<Key>,
        target: T,
        config: crate::animation::TransitionConfig,
    ) -> T
    where
        T: crate::animation::Lerp + PartialEq + 'static,
    {
        let key = key.into();
        self.env.note_memo_dependency(MemoDependency::Transition);
        self.env.animations.transition(key, target, config)
    }

    /// Convenience helper for responsive layouts based on viewport width.
    ///
    /// Returns:
    /// - `Breakpoint::Small` if `viewport().w < medium`,
    /// - `Breakpoint::Medium` if `viewport().w < large`,
    /// - `Breakpoint::Large` otherwise.
    pub fn breakpoint(&self, medium: u16, large: u16) -> Breakpoint {
        let (medium, large) = if medium <= large {
            (medium, large)
        } else {
            (large, medium)
        };

        self.env.note_memo_dependency(MemoDependency::Viewport);
        let w = self.viewport.w;
        if w < medium {
            Breakpoint::Small
        } else if w < large {
            Breakpoint::Medium
        } else {
            Breakpoint::Large
        }
    }

    pub(crate) fn env(&self) -> &RuntimeEnv {
        &self.env
    }

    pub(crate) fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
    }

    pub(crate) fn set_active_theme(&mut self, theme: Theme) {
        let mut active_theme = self.env.active_theme.borrow_mut();
        if *active_theme != theme {
            *active_theme = theme;
            self.env.active_theme_generation.set(
                self.env
                    .active_theme_generation
                    .get()
                    .wrapping_add(1)
                    .max(1),
            );
        }
    }

    pub(crate) fn set_contexts(
        &mut self,
        contexts: rustc_hash::FxHashMap<TypeId, std::sync::Arc<dyn std::any::Any>>,
        generations: rustc_hash::FxHashMap<TypeId, u64>,
    ) {
        *self.env.contexts.borrow_mut() = contexts;
        *self.env.context_generations.borrow_mut() = generations;
    }

    pub(crate) fn memo_key(&self, component: &C) -> Option<u64> {
        component.memo_key(&self.props, self)
    }

    pub(crate) fn begin_memo_dependency_capture(&self) {
        self.env.scroll.begin_view(self.scope);
        self.env.begin_memo_dependency_capture();
    }

    pub(crate) fn finish_memo_dependency_capture(&self) -> MemoDependencySnapshot {
        self.env.finish_memo_dependency_capture(self.viewport)
    }

    pub(crate) fn memo_dependencies_match(&self, snapshot: &MemoDependencySnapshot) -> bool {
        snapshot.matches(&self.env, self.viewport)
    }

    /// Request application shutdown.
    pub fn quit(&mut self) {
        self.env.quit.set(true);
    }

    /// Request focus to move to the focusable node with `key`.
    ///
    /// This takes effect on the next event loop tick / render.
    pub fn request_focus(&mut self, key: impl Into<Key>) {
        *self.env.focus_request.borrow_mut() = Some(FocusRequest::Key(key.into()));
    }

    /// Clear the current focus.
    ///
    /// Under [`crate::FocusPolicy::Auto`], the next render restores the default focus target.
    pub fn blur(&mut self) {
        *self.env.focus_request.borrow_mut() = Some(FocusRequest::Clear);
    }

    /// Request focus to move to the next tab stop.
    pub fn focus_next(&mut self) {
        *self.env.focus_request.borrow_mut() = Some(FocusRequest::Next);
    }

    /// Request focus to move to the previous tab stop.
    pub fn focus_prev(&mut self) {
        *self.env.focus_request.borrow_mut() = Some(FocusRequest::Prev);
    }

    /// Request a full layout and paint pass on the next frame.
    ///
    /// Use after the host terminal was repainted by another process (external editor,
    /// pager, etc.) so nested components are not stuck on a layout-only update path.
    pub fn request_full_repaint(&self) {
        self.env.full_repaint.set(true);
    }

    /// Request that the built-in devtools panel becomes visible.
    pub fn show_devtools(&self) {
        *self.env.devtools_request.borrow_mut() = Some(DevToolsRequest::Show);
    }

    /// Request that the built-in devtools panel becomes hidden.
    pub fn hide_devtools(&self) {
        *self.env.devtools_request.borrow_mut() = Some(DevToolsRequest::Hide);
    }

    /// Request that the built-in devtools panel toggles visibility.
    pub fn toggle_devtools(&self) {
        *self.env.devtools_request.borrow_mut() = Some(DevToolsRequest::Toggle);
    }

    pub(crate) fn take_focus_request(&self) -> Option<FocusRequest> {
        self.env.focus_request.borrow_mut().take()
    }

    pub(crate) fn take_full_repaint_request(&self) -> bool {
        self.env.full_repaint.replace(false)
    }

    pub(crate) fn take_devtools_request(&self) -> Option<DevToolsRequest> {
        self.env.devtools_request.borrow_mut().take()
    }

    /// Queue a UI snapshot write to `path` after the next render.
    ///
    /// Uses JSON when `path` ends with `.json` and the `ui-snapshot-json` feature is
    /// enabled, or PNG when `path` ends with `.png` and the `ui-snapshot-png` feature
    /// is enabled; otherwise writes markdown.
    ///
    /// A pending request replaces any earlier one (last writer wins). Triggers a full
    /// repaint so idle apps still deliver the snapshot.
    pub fn request_ui_snapshot_to(&self, path: impl AsRef<std::path::Path>) {
        let path = path.as_ref().to_path_buf();
        let extension = path.extension();
        let format = if extension.is_some_and(|ext| ext == "json" || ext == "JSON") {
            #[cfg(feature = "ui-snapshot-json")]
            {
                crate::ui_snapshot::UiSnapshotFileFormat::Json
            }
            #[cfg(not(feature = "ui-snapshot-json"))]
            {
                crate::ui_snapshot::UiSnapshotFileFormat::Markdown
            }
        } else if extension.is_some_and(|ext| ext == "png" || ext == "PNG") {
            #[cfg(feature = "ui-snapshot-png")]
            {
                crate::ui_snapshot::UiSnapshotFileFormat::Png
            }
            #[cfg(not(feature = "ui-snapshot-png"))]
            {
                crate::ui_snapshot::UiSnapshotFileFormat::Markdown
            }
        } else {
            crate::ui_snapshot::UiSnapshotFileFormat::Markdown
        };
        *self.env.ui_snapshot_request.borrow_mut() =
            Some(crate::ui_snapshot::UiSnapshotRequest::Write { path, format });
        self.request_full_repaint();
    }

    /// Queue delivery of a UI snapshot into `slot` after the next render.
    ///
    /// A pending request replaces any earlier one (last writer wins). Triggers a full
    /// repaint so idle apps still deliver the snapshot.
    pub fn request_ui_snapshot_to_slot(&self, slot: &crate::ui_snapshot::UiSnapshotSlot) {
        *self.env.ui_snapshot_request.borrow_mut() = Some(
            crate::ui_snapshot::UiSnapshotRequest::Deliver(slot.shared()),
        );
        self.request_full_repaint();
    }

    pub(crate) fn take_ui_snapshot_request(&self) -> Option<crate::ui_snapshot::UiSnapshotRequest> {
        self.env.ui_snapshot_request.borrow_mut().take()
    }

    pub(crate) fn should_quit(&self) -> bool {
        self.env.quit.get()
    }
}

#[cfg(test)]
mod tests;
