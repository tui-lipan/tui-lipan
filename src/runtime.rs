use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::Result;
use crate::app::context::SurfaceMode;
use crate::app::input::command_registry::CommandRegistry;
use crate::app::input::focus;
use crate::callback::{CommandRx, CommandTx, Dispatcher, ScopeId};
use crate::core::component::{
    CommandRuntime, Component, Context, FocusContext, HoverContext, KeyUpdate, ScrollContext,
    UpdateLevel,
};
use crate::core::element::{Element, ElementKind, Key};
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeTree};
use crate::core::runtime_env::{RuntimeEnv, TranscriptEntry};
use crate::layout::LayoutEngine;
use crate::overlay::{OverlayEntry, OverlayManager};
use crate::style::{Rect, Theme};
use crate::widgets::ZStack;

pub(crate) type MsgQueue = Rc<RefCell<VecDeque<(ScopeId, Box<dyn Any>)>>>;
pub(crate) type PendingTranscriptQueue = Rc<RefCell<VecDeque<TranscriptEntry>>>;
pub(crate) type TranscriptHistory = Rc<RefCell<Vec<TranscriptEntry>>>;
pub(crate) const EXTRA_ROOT_WRAPPER_KEY: &str = "__tui_lipan_extra_root_wrapper";

pub(crate) enum FocusRequest {
    Key(Key),
    Clear,
    Next,
    Prev,
}

enum ReconcileRoot<'a> {
    Borrowed(&'a Element),
    Owned(Box<Element>),
}

impl ReconcileRoot<'_> {
    fn as_element(&self) -> &Element {
        match self {
            Self::Borrowed(element) => element,
            Self::Owned(element) => element.as_ref(),
        }
    }
}

fn root_element_for_reconcile<'a>(
    base: &'a Element,
    extra_root_enabled: bool,
    extra: Option<&'a Element>,
) -> Option<ReconcileRoot<'a>> {
    if extra_root_enabled {
        let extra = extra?;
        Some(ReconcileRoot::Owned(Box::new(
            Element::from(
                ZStack::new()
                    .passthrough(true)
                    .child(base.clone())
                    .child(extra.clone()),
            )
            .key(EXTRA_ROOT_WRAPPER_KEY),
        )))
    } else {
        Some(ReconcileRoot::Borrowed(base))
    }
}

/// Resolve the theme active at the app content root, so the DevTools extra root
/// (and its wrapper `ThemeProvider`) match what `App::fill_background()` reads
/// from `app_content_root_node().active_theme()`.
///
/// Reconcile treats `ThemeProvider`/`ContextProvider` as transparent wrappers
/// that create no node of their own: it pushes the provider theme and descends,
/// so the first real (node-creating) element bakes in the innermost provider
/// theme in scope. Mirror that here by walking down through those wrappers
/// instead of only inspecting the top-level element, otherwise an app whose root
/// nests its `ThemeProvider` under a context provider (or a second provider)
/// would leave DevTools stuck on the stale startup theme while the app content
/// tracked the live one.
fn root_active_theme_for_extra_root(default_theme: &Theme, root: &Element) -> Theme {
    let mut theme = default_theme;
    let mut el = root;
    loop {
        match &el.kind {
            ElementKind::ThemeProvider(provider) => {
                theme = &provider.theme;
                el = &provider.child;
            }
            ElementKind::ContextProvider(provider) => {
                el = &provider.child;
            }
            _ => return theme.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BubbleKeyResult {
    pub(crate) handled: bool,
    pub(crate) dirty: bool,
}

pub(crate) struct RuntimeCore<C: Component> {
    pub(crate) component: C,
    pub(crate) surface_mode: SurfaceMode,
    pub(crate) theme: Theme,
    pub(crate) ctx: Context<C>,
    pub(crate) queue: MsgQueue,
    pub(crate) pending_transcript_entries: PendingTranscriptQueue,
    pub(crate) transcript_history: TranscriptHistory,
    pub(crate) command_tx: CommandTx,
    pub(crate) command_rx: CommandRx,
    pub(crate) focus: Rc<FocusContext>,
    pub(crate) hover: Rc<HoverContext>,
    pub(crate) scroll: Rc<ScrollContext>,
    pub(crate) overlay_manager: Rc<RefCell<crate::overlay::OverlayManager>>,
    pub(crate) components: crate::core::nested::ComponentRegistry,
    pub(crate) root_host: crate::core::nested::HostState,
    pub(crate) extra_root_host: crate::core::nested::HostState,
    pub(crate) tree: NodeTree,
    pub(crate) cached_expanded_element: Option<Element>,
    pub(crate) cached_extra_expanded_element: Option<Element>,
    pub(crate) extra_root_element: Option<Element>,
    /// Cached overlay entries with the generation they were cloned at.
    /// Avoids deep-cloning every frame when overlays haven't changed.
    cached_overlays: Option<(u64, Rc<[OverlayEntry]>)>,
    #[cfg(debug_assertions)]
    pub(crate) debug_last_root_view_before_expand: Option<Element>,
}

pub(crate) struct RuntimeCoreConfig {
    pub viewport: Rect,
    pub theme: Theme,
    pub surface_mode: SurfaceMode,
    pub mouse_capture: Rc<Cell<bool>>,
    pub clipboard: Rc<crate::clipboard::ClipboardService>,
    pub clipboard_config: crate::clipboard::ClipboardConfig,
    pub host_terminal_color_refresh_enabled: bool,
}

impl<C> RuntimeCore<C>
where
    C: Component,
{
    /// Full root component type name for diagnostics and tracing.
    #[allow(dead_code)] // consumed by DevTools attribution and render spans
    pub(crate) fn root_component_name(&self) -> &'static str {
        std::any::type_name::<C>()
    }

    pub(crate) fn new(component: C, props: C::Properties, config: RuntimeCoreConfig) -> Self {
        let RuntimeCoreConfig {
            viewport,
            theme,
            surface_mode,
            mouse_capture,
            clipboard,
            clipboard_config,
            host_terminal_color_refresh_enabled,
        } = config;
        let queue: MsgQueue = Rc::new(RefCell::new(VecDeque::new()));
        let dispatcher = {
            let queue = queue.clone();
            Dispatcher::new(move |scope, msg| queue.borrow_mut().push_back((scope, msg)))
        };

        let (command_tx, command_rx): (CommandTx, CommandRx) = std::sync::mpsc::channel();
        let quit = Rc::new(Cell::new(false));

        let focus = Rc::new(FocusContext::default());
        let hover = Rc::new(HoverContext::default());
        let scroll = Rc::new(ScrollContext::default());
        let animations = Rc::new(crate::animation::AnimationRegistry::default());
        let focus_request = Rc::new(RefCell::new(None));
        let overlay_manager = Rc::new(RefCell::new(OverlayManager::new()));
        overlay_manager
            .borrow_mut()
            .set_inline_mode(surface_mode.is_inline());
        let pending_transcript_entries: PendingTranscriptQueue =
            Rc::new(RefCell::new(VecDeque::new()));
        let transcript_history: TranscriptHistory = Rc::new(RefCell::new(Vec::new()));
        let full_repaint = Rc::new(Cell::new(false));
        let devtools_request = Rc::new(RefCell::new(None));
        let ui_snapshot_request = Rc::new(RefCell::new(None));
        let active_theme = Rc::new(RefCell::new(theme.clone()));
        let active_theme_generation = Rc::new(Cell::new(1));
        let effect_phase = Rc::new(Cell::new(0));
        let contexts = Rc::new(RefCell::new(rustc_hash::FxHashMap::default()));
        let context_generations = Rc::new(RefCell::new(rustc_hash::FxHashMap::default()));
        let host_terminal_colors = Rc::new(Cell::new(None));
        let host_terminal_color_generation = Rc::new(Cell::new(0));
        let host_terminal_color_refresh_requested = Rc::new(Cell::new(false));
        let mouse_capture_generation = Rc::new(Cell::new(1));
        let memo_dependency_recorder = Rc::new(RefCell::new(None));
        let command_chord_pending = Rc::new(Cell::new(false));

        let env = RuntimeEnv {
            command_registry: CommandRegistry::default(),
            quit,
            focus: focus.clone(),
            hover: hover.clone(),
            scroll: scroll.clone(),
            animations: animations.clone(),
            overlay_manager: overlay_manager.clone(),
            focus_request,
            mouse_capture,
            surface_mode,
            transcript_history: transcript_history.clone(),
            pending_transcript_entries: pending_transcript_entries.clone(),
            clipboard,
            clipboard_config,
            active_theme,
            active_theme_generation,
            effect_phase,
            contexts,
            context_generations,
            host_terminal_colors,
            host_terminal_color_generation,
            host_terminal_color_refresh_requested,
            host_terminal_color_refresh_enabled,
            mouse_capture_generation,
            memo_dependency_recorder,
            full_repaint,
            devtools_request,
            ui_snapshot_request,
            command_chord_pending,
        };

        let components = crate::core::nested::ComponentRegistry::new(
            crate::core::nested::ComponentRegistryConfig {
                dispatcher: dispatcher.clone(),
                command_tx: command_tx.clone(),
                env: env.clone(),
            },
        );

        let scope = ScopeId(1);
        let ctx = Context::new(&component, scope, dispatcher, props, env, viewport);

        Self {
            component,
            surface_mode,
            theme,
            ctx,
            queue,
            pending_transcript_entries,
            transcript_history,
            command_tx,
            command_rx,
            focus,
            hover,
            scroll,
            overlay_manager,
            components,
            root_host: crate::core::nested::HostState::default(),
            extra_root_host: crate::core::nested::HostState::default(),
            tree: NodeTree::new(),
            cached_expanded_element: None,
            cached_extra_expanded_element: None,
            extra_root_element: None,
            cached_overlays: None,
            #[cfg(debug_assertions)]
            debug_last_root_view_before_expand: None,
        }
    }

    #[cfg(feature = "devtools")]
    pub(crate) fn set_extra_root_element(&mut self, element: Option<Element>) {
        if element.is_none() && self.extra_root_element.is_some() {
            // Clear the host state so stale ComponentIds from the previous
            // devtools session are not fed back into `reuse_plan` when the
            // panel is re-opened.  Without this, swept arena slots are
            // accessed through dangling IDs, causing an "invalid arena id"
            // panic.
            self.extra_root_host = Default::default();
        }
        self.extra_root_element = element;
    }

    /// Create a `RuntimeCore` with a no-op clipboard for tests.
    pub(crate) fn new_test(
        component: C,
        props: C::Properties,
        viewport: Rect,
        theme: Theme,
        surface_mode: SurfaceMode,
        mouse_capture: Rc<Cell<bool>>,
    ) -> Self {
        Self::new(
            component,
            props,
            RuntimeCoreConfig {
                viewport,
                theme,
                surface_mode,
                mouse_capture,
                clipboard: crate::clipboard::test_clipboard(),
                clipboard_config: crate::clipboard::ClipboardConfig::default(),
                host_terminal_color_refresh_enabled: false,
            },
        )
    }

    pub(crate) fn new_test_transcript(
        component: C,
        props: C::Properties,
        viewport: Rect,
        theme: Theme,
        mouse_capture: Rc<Cell<bool>>,
    ) -> Self {
        Self::new(
            component,
            props,
            RuntimeCoreConfig {
                viewport,
                theme,
                surface_mode: SurfaceMode::InlineTranscript {
                    height: crate::app::context::InlineHeight::Fixed(8),
                    startup: crate::app::context::InlineStartupPolicy::PreserveHost,
                },
                mouse_capture,
                clipboard: crate::clipboard::test_clipboard(),
                clipboard_config: crate::clipboard::ClipboardConfig::default(),
                host_terminal_color_refresh_enabled: false,
            },
        )
    }

    /// Return a cached clone of the overlay entries, only re-cloning when the
    /// overlay manager's generation has advanced. The `Rc` wrapper makes cache
    /// hits essentially free (just a ref-count bump).
    fn overlay_snapshot(&mut self) -> Rc<[OverlayEntry]> {
        let current_gen = self.overlay_manager.borrow().generation();
        if let Some((cached_gen, ref entries)) = self.cached_overlays
            && cached_gen == current_gen
        {
            return Rc::clone(entries);
        }
        let entries: Rc<[OverlayEntry]> = self.overlay_manager.borrow().entries().to_vec().into();
        self.cached_overlays = Some((current_gen, Rc::clone(&entries)));
        entries
    }

    pub(crate) fn init(&mut self) {
        self.ctx.set_active_theme(self.theme.clone());
        if let Some(cmd) = self.component.init(&mut self.ctx) {
            cmd.run(CommandRuntime {
                scope: ScopeId(1),
                tx: self.command_tx.clone(),
            });
        }
    }

    pub(crate) fn drain_commands(&mut self) {
        let mut queue = self.queue.borrow_mut();
        while let Ok((scope, msg)) = self.command_rx.try_recv() {
            let msg: Box<dyn Any> = msg;
            queue.push_back((scope, msg));
        }
    }

    pub(crate) fn has_pending_transcript_entries(&self) -> bool {
        !self.pending_transcript_entries.borrow().is_empty()
    }

    pub(crate) fn take_pending_transcript_entries(&self) -> VecDeque<TranscriptEntry> {
        std::mem::take(&mut *self.pending_transcript_entries.borrow_mut())
    }

    pub(crate) fn clear_pending_transcript_entries(&self) {
        self.pending_transcript_entries.borrow_mut().clear();
    }

    pub(crate) fn has_transcript_history(&self) -> bool {
        !self.transcript_history.borrow().is_empty()
    }

    pub(crate) fn transcript_history_snapshot(&self) -> Vec<TranscriptEntry> {
        self.transcript_history.borrow().clone()
    }

    pub(crate) fn transcript_replay_document(
        &self,
        include_live_viewport: bool,
    ) -> Vec<TranscriptEntry> {
        let mut document = self.transcript_history.borrow().clone();
        if include_live_viewport && let Some(current) = self.cached_expanded_element.clone() {
            document.push(TranscriptEntry::Element(Box::new(current)));
        }
        document
    }

    pub(crate) fn take_full_repaint_request(&self) -> bool {
        self.ctx.take_full_repaint_request()
    }

    pub(crate) fn set_effect_phase(&self, phase: u64) {
        self.ctx.env().set_effect_phase(phase);
    }

    pub(crate) fn update_from_boxed(
        &mut self,
        scope: ScopeId,
        msg: Box<dyn Any>,
    ) -> Result<UpdateLevel> {
        let update = if scope == ScopeId(1) {
            self.ctx.set_active_theme(self.theme.clone());
            let actual = std::any::type_name_of_val(msg.as_ref());
            let expected = std::any::type_name::<C::Message>();
            let component = std::any::type_name::<C>();
            let msg =
                msg.downcast::<C::Message>()
                    .map_err(|_| crate::Error::MessageTypeMismatch {
                        component,
                        expected,
                        actual,
                    })?;
            self.component.update(*msg, &mut self.ctx)
        } else {
            self.components.update_by_scope(scope, msg)?
        };

        let update_level = update.level();
        if let Some(cmd) = update.command {
            cmd.run(CommandRuntime {
                scope,
                tx: self.command_tx.clone(),
            });
        }

        Ok(update_level)
    }

    /// Bubble a key event up through component scopes, starting from the scope
    /// that contains `focused`.
    ///
    /// This is the core logic shared by `AppRunner::bubble_key` and `TestBackend::send_key`.
    pub(crate) fn bubble_key(
        &mut self,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
        key: KeyEvent,
    ) -> BubbleKeyResult {
        let scope = self.resolve_bubble_key_scope(focused, focused_key);

        let mut dirty = false;
        let mut handled_any = false;
        let mut cur = scope;

        loop {
            let KeyUpdate { handled, update } = if cur == ScopeId(1) {
                self.ctx.set_active_theme(self.theme.clone());
                self.component.on_key(key, &mut self.ctx)
            } else {
                self.components.on_key_by_scope(cur, key)
            };

            if let Some(cmd) = update.command {
                cmd.run(CommandRuntime {
                    scope: cur,
                    tx: self.command_tx.clone(),
                });
            }

            dirty |= update.dirty;
            handled_any |= handled;

            if handled || cur == ScopeId(1) {
                break;
            }

            let next = self.components.parent_scope(cur).unwrap_or(ScopeId(1));
            if next == cur {
                break;
            }
            cur = next;
        }

        BubbleKeyResult {
            handled: handled_any,
            dirty,
        }
    }

    fn resolve_bubble_key_scope(
        &self,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
    ) -> ScopeId {
        if let Some(scope) = focused
            .filter(|id| self.tree.is_valid(*id))
            .and_then(|id| focus::scope_for_node(&self.tree, id))
        {
            return scope;
        }

        if let Some(scope) = focused_key
            .and_then(|key| {
                self.tree
                    .iter()
                    .find(|n| n.key.as_ref() == Some(key))
                    .map(|n| n.id)
            })
            .and_then(|id| focus::scope_for_node(&self.tree, id))
        {
            return scope;
        }

        self.deepest_mounted_group_scope().unwrap_or(ScopeId(1))
    }

    fn deepest_mounted_group_scope(&self) -> Option<ScopeId> {
        let mut best: Option<(usize, ScopeId)> = None;

        for node in self.tree.iter() {
            let crate::core::node::NodeKind::Group(group) = &node.kind else {
                continue;
            };
            if self.components.parent_scope(group.scope).is_none() {
                continue;
            }

            let mut depth = 0usize;
            let mut cur = Some(node.id);
            while let Some(id) = cur {
                if !self.tree.is_valid(id) {
                    break;
                }
                cur = self.tree.node(id).parent;
                depth += 1;
            }

            if best.is_none_or(|(best_depth, _)| depth >= best_depth) {
                best = Some((depth, group.scope));
            }
        }

        best.map(|(_, scope)| scope)
    }

    pub(crate) fn render_element(
        &mut self,
        bounds: Rect,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
        hovered: Option<NodeId>,
    ) {
        self.ctx.set_viewport(bounds);
        self.ctx.set_active_theme(self.theme.clone());
        self.focus
            .update_from_tree(&self.tree, focused, focused_key);
        self.hover.update_from_tree(&self.tree, hovered);

        #[cfg(feature = "profiling-tracing")]
        let view_start = web_time::Instant::now();
        self.scroll.begin_view(ScopeId(1));
        let element = self.component.view(&self.ctx);
        #[cfg(feature = "profiling-tracing")]
        let view_ms = view_start.elapsed().as_secs_f64() * 1000.0;

        #[cfg(debug_assertions)]
        {
            self.debug_last_root_view_before_expand = Some(element.clone());
        }

        #[cfg(feature = "profiling-tracing")]
        let expand_start = web_time::Instant::now();
        let epoch = self.components.begin_epoch();
        let app_root_theme = root_active_theme_for_extra_root(&self.theme, &element);
        let element =
            self.components
                .expand_in_host(&mut self.root_host, None, element, epoch, bounds);
        let element = crate::style::apply_document_theme_carve_out(&self.theme, element);
        self.ctx.set_active_theme(app_root_theme.clone());
        let extra = self.extra_root_element.clone().map(|extra| {
            let expanded = self.components.expand_in_host(
                &mut self.extra_root_host,
                None,
                extra,
                epoch,
                bounds,
            );
            crate::widgets::ThemeProvider::new(app_root_theme.clone())
                .child(crate::style::apply_document_theme_carve_out(
                    &app_root_theme,
                    expanded,
                ))
                .into()
        });
        self.ctx.set_active_theme(self.theme.clone());
        self.components.sweep(epoch);
        #[cfg(feature = "profiling-tracing")]
        let expand_ms = expand_start.elapsed().as_secs_f64() * 1000.0;

        let overlays = self.overlay_snapshot();
        // Move element into cache instead of cloning - the previous deep clone
        // was the single largest allocation spike per frame.
        self.cached_expanded_element = Some(element);
        self.cached_extra_expanded_element = extra;

        #[cfg(feature = "profiling-tracing")]
        let reconcile_start = web_time::Instant::now();
        let base = self
            .cached_expanded_element
            .as_ref()
            .expect("expanded element should exist before reconciliation");
        let root = root_element_for_reconcile(
            base,
            self.extra_root_element.is_some(),
            self.cached_extra_expanded_element.as_ref(),
        )
        .expect("expanded extra root element should exist before reconciliation");
        self.tree.set_base_active_theme(self.theme.clone());
        LayoutEngine::reconcile_with_overlays_mode(
            &mut self.tree,
            root.as_element(),
            bounds,
            Some(self.focus.as_ref()),
            &overlays,
            !self.surface_mode.is_inline(),
        );
        self.scroll.update_from_tree(&self.tree);
        self.ctx.env().animations.end_frame_gc();
        #[cfg(feature = "profiling-tracing")]
        tracing::trace!(
            target: "tui_lipan::perf",
            view_ms,
            expand_ms,
            reconcile_ms = reconcile_start.elapsed().as_secs_f64() * 1000.0,
        );
        // Hoverables/move-handlers/animation flags are tracked incrementally via
        // note_kind_set() during reconciliation - no DFS needed.
    }

    pub(crate) fn reconcile_cached_element(
        &mut self,
        bounds: Rect,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
        hovered: Option<NodeId>,
    ) -> bool {
        if self.cached_expanded_element.is_none() {
            return false;
        }

        self.ctx.set_viewport(bounds);
        self.focus
            .update_from_tree(&self.tree, focused, focused_key);
        self.hover.update_from_tree(&self.tree, hovered);

        let overlays = self.overlay_snapshot();
        let base = self
            .cached_expanded_element
            .as_ref()
            .expect("expanded element should exist before reconciliation");
        let root = root_element_for_reconcile(
            base,
            self.extra_root_element.is_some(),
            self.cached_extra_expanded_element.as_ref(),
        )
        .expect("expanded extra root element should exist before reconciliation");
        self.tree.set_base_active_theme(self.theme.clone());
        LayoutEngine::reconcile_with_overlays_mode(
            &mut self.tree,
            root.as_element(),
            bounds,
            Some(self.focus.as_ref()),
            &overlays,
            !self.surface_mode.is_inline(),
        );
        self.scroll.update_from_tree(&self.tree);

        true
    }

    /// Refresh the cached element tree for the given scopes.
    ///
    /// Callers must provide an already-deduplicated list (the runner deduplicates
    /// at collection time via `dirty_scope_set`).
    pub(crate) fn refresh_cached_scopes(&mut self, scopes: &[ScopeId], viewport: Rect) -> bool {
        if self.cached_expanded_element.is_none() && self.cached_overlays.is_none() {
            return false;
        }

        if scopes.is_empty() {
            return true;
        }

        if scopes.contains(&ScopeId(1)) {
            self.ctx.set_viewport(viewport);
            self.ctx.set_active_theme(self.theme.clone());

            self.scroll.begin_view(ScopeId(1));
            let element = self.component.view(&self.ctx);

            let app_root_theme = root_active_theme_for_extra_root(&self.theme, &element);

            #[cfg(debug_assertions)]
            {
                self.debug_last_root_view_before_expand = Some(element.clone());
            }

            let epoch = self.components.begin_epoch();
            let element =
                self.components
                    .expand_in_host(&mut self.root_host, None, element, epoch, viewport);
            let element = crate::style::apply_document_theme_carve_out(&self.theme, element);
            self.ctx.set_active_theme(app_root_theme.clone());
            let extra = self.extra_root_element.clone().map(|extra| {
                let expanded = self.components.expand_in_host(
                    &mut self.extra_root_host,
                    None,
                    extra,
                    epoch,
                    viewport,
                );
                crate::widgets::ThemeProvider::new(app_root_theme.clone())
                    .child(crate::style::apply_document_theme_carve_out(
                        &app_root_theme,
                        expanded,
                    ))
                    .into()
            });
            self.ctx.set_active_theme(self.theme.clone());
            self.components.sweep(epoch);
            self.cached_expanded_element = Some(element);
            self.cached_extra_expanded_element = extra;
            self.ctx.env().animations.end_frame_gc();

            if scopes.len() == 1 {
                return true;
            }
        }

        let mut replacements = Vec::with_capacity(scopes.len());
        for &scope in scopes {
            if scope == ScopeId(1) {
                continue;
            }
            let Some(refreshed) = self.components.refresh_scope_in_place(scope, viewport) else {
                return false;
            };
            let expanded =
                crate::style::apply_document_theme_carve_out(&refreshed.theme, refreshed.expanded);
            replacements.push((refreshed.scope, expanded));
        }

        for (scope, replacement) in replacements {
            if !self.replace_scope_in_cached_trees(scope, replacement) {
                return false;
            }
        }

        true
    }

    fn replace_scope_in_cached_trees(&mut self, scope: ScopeId, replacement: Element) -> bool {
        if scope == ScopeId(1) {
            self.cached_expanded_element = Some(replacement);
            return true;
        }

        let mut replacement = Some(replacement);

        if let Some(root) = self.cached_expanded_element.as_mut()
            && root.replace_group_child_by_scope(scope, &mut replacement)
        {
            return true;
        }

        if let Some(extra_root) = self.cached_extra_expanded_element.as_mut()
            && extra_root.replace_group_child_by_scope(scope, &mut replacement)
        {
            return true;
        }

        if let Some((_gen, overlays)) = self.cached_overlays.as_mut() {
            let overlays_mut = Rc::make_mut(overlays);
            for entry in overlays_mut.iter_mut() {
                if entry
                    .content
                    .replace_group_child_by_scope(scope, &mut replacement)
                {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
pub(crate) fn assert_inline_transcript_append_rejects_component_nodes() {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::core::component::{Component, Context, Update};
    use crate::core::element::Element;
    use crate::style::Theme;
    use crate::widgets::Text;

    #[derive(Clone)]
    enum Msg {
        WithComponent,
    }

    struct Probe;

    impl Component for Probe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("probe").into()
        }
    }

    struct CommitComponent;

    impl Component for CommitComponent {
        type Message = Msg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::WithComponent => {
                    ctx.append_transcript_element(
                        crate::widgets::Frame::new().child(crate::child(|| Probe, ())),
                    );
                }
            }
            Update::full()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("root").into()
        }
    }

    let mouse_capture = Rc::new(Cell::new(true));
    let mut runtime = RuntimeCore::new_test_transcript(
        CommitComponent,
        (),
        Rect::default(),
        Theme::default(),
        mouse_capture,
    );

    assert!(!matches!(
        runtime
            .update_from_boxed(crate::callback::ScopeId(1), Box::new(Msg::WithComponent))
            .expect("update should succeed"),
        UpdateLevel::None
    ));
    assert!(runtime.take_pending_transcript_entries().is_empty());
}

#[cfg(test)]
pub(crate) fn assert_inline_surface_commit_render_path_is_unified() {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::core::component::{Component, Context, Update};
    use crate::core::element::Element;
    use crate::core::runtime_env::TranscriptEntry;
    use crate::style::{Rect, Theme};
    use crate::widgets::Text;

    #[derive(Clone)]
    enum Msg {
        AppendLinesAndElement,
    }

    struct Probe;

    impl Component for Probe {
        type Message = Msg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::AppendLinesAndElement => {
                    ctx.append_transcript_lines(["line-a", "line-b"]);
                    ctx.append_transcript_element(Text::new("element-c"));
                }
            }
            Update::full()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("root").into()
        }
    }

    let mouse_capture = Rc::new(Cell::new(true));
    let mut runtime = RuntimeCore::new_test_transcript(
        Probe,
        (),
        Rect::default(),
        Theme::default(),
        mouse_capture,
    );

    assert!(!matches!(
        runtime
            .update_from_boxed(
                crate::callback::ScopeId(1),
                Box::new(Msg::AppendLinesAndElement),
            )
            .expect("update should succeed"),
        UpdateLevel::None
    ));

    let pending = runtime.take_pending_transcript_entries();
    assert_eq!(pending.len(), 2);
    assert!(matches!(pending.front(), Some(TranscriptEntry::Lines(lines)) if lines.len() == 2));
    assert!(matches!(
        pending.back(),
        Some(TranscriptEntry::Element(element)) if matches!(element.kind, crate::core::element::ElementKind::Text(_))
    ));

    // Task-8 guardrail: transcript element commits must not use a TestBackend
    // terminal scratch path.
    let render_service_src = include_str!("app/runner/render_service/inline.rs");
    assert!(
        !render_service_src.contains("scratch.draw(|f| render(f, &ctx))"),
        "inline element commits must render directly via frame/buffer path, not scratch draw()"
    );
    assert!(
        !render_service_src.contains(
            "inline_commit_scratch: Option<ratatui::Terminal<ratatui::backend::TestBackend>>"
        ),
        "inline element commit scratch terminal must not use TestBackend"
    );
    assert!(
        !render_service_src
            .contains("let backend = ratatui::backend::TestBackend::new(width, height);"),
        "inline element commit path must not allocate TestBackend terminals"
    );
    assert!(
        render_service_src.contains("let mut frame = scratch.get_frame();")
            && render_service_src.contains("frame.buffer_mut().reset();")
            && render_service_src.contains("render(&mut frame, &ctx);"),
        "inline element commits must render through a direct frame/buffer path"
    );
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::{ReconcileRoot, RuntimeCore, root_element_for_reconcile};
    use crate::app::context::SurfaceMode;
    use crate::callback::{Link, ScopeId};
    use crate::core::component::{Component, Context, KeyUpdate, Update, UpdateLevel};
    use crate::core::element::{Element, ElementKind, IntoElement, Key};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::core::node::NodeKind;
    use crate::core::runtime_env::TranscriptEntry;
    use crate::style::{Color, Rect, RichText, Span, Style, Theme};
    use crate::widgets::{Frame, Input, Modal, Text, ThemeProvider};

    #[test]
    fn root_element_for_reconcile_borrows_without_extra_root() {
        let base: Element = Text::new("base").into();

        let root = root_element_for_reconcile(&base, false, None).unwrap();

        match root {
            ReconcileRoot::Borrowed(element) => assert!(std::ptr::eq(element, &base)),
            ReconcileRoot::Owned(_) => panic!("root should be borrowed without an extra root"),
        }
    }

    #[test]
    fn root_element_for_reconcile_wraps_extra_root() {
        let base: Element = Text::new("base").into();
        let extra: Element = Text::new("extra").into();

        let root = root_element_for_reconcile(&base, true, Some(&extra)).unwrap();

        match &root.as_element().kind {
            ElementKind::ZStack(stack) => {
                assert!(stack.passthrough);
                assert_eq!(stack.children.len(), 2);
            }
            _ => panic!("extra root should be wrapped in a ZStack"),
        }
    }

    #[test]
    fn root_active_theme_for_extra_root_descends_through_provider_wrappers() {
        let default_theme = Theme::default().accent(Style::new().fg(Color::Rgb(1, 1, 1)));
        let root_theme = Theme::default().accent(Style::new().fg(Color::Rgb(2, 2, 2)));
        let inner_theme = Theme::default().accent(Style::new().fg(Color::Rgb(3, 3, 3)));
        let accent = |theme: &Theme| theme.accent.fg;

        // Top-level provider: its theme wins.
        let top: Element = ThemeProvider::new(root_theme.clone())
            .child(Text::new("root"))
            .into();
        assert_eq!(
            accent(&super::root_active_theme_for_extra_root(
                &default_theme,
                &top
            )),
            accent(&root_theme)
        );

        // Nested providers: reconcile bakes the innermost theme into the content
        // node, so the resolver must return the innermost theme too.
        let nested: Element = ThemeProvider::new(root_theme.clone())
            .child(ThemeProvider::new(inner_theme.clone()).child(Text::new("root")))
            .into();
        assert_eq!(
            accent(&super::root_active_theme_for_extra_root(
                &default_theme,
                &nested
            )),
            accent(&inner_theme)
        );

        // A node-creating wrapper (Frame) stops the walk before any nested
        // provider, matching the Frame node's inherited (default) active theme.
        let framed: Element = Frame::new()
            .child(ThemeProvider::new(inner_theme.clone()).child(Text::new("root")))
            .into();
        assert_eq!(
            accent(&super::root_active_theme_for_extra_root(
                &default_theme,
                &framed
            )),
            accent(&default_theme)
        );
    }

    #[derive(Clone)]
    enum ChildMsg {
        Increment,
    }

    #[derive(Clone)]
    struct ChildComponent {
        link_slot: Rc<RefCell<Option<Link<ChildMsg>>>>,
        view_count: Rc<Cell<usize>>,
    }

    impl Component for ChildComponent {
        type Message = ChildMsg;
        type Properties = ();
        type State = usize;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn init(&mut self, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
            *self.link_slot.borrow_mut() = Some(ctx.link().clone());
            None
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            self.view_count.set(self.view_count.get() + 1);
            Text::new(format!("child:{}", ctx.state)).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                ChildMsg::Increment => {
                    ctx.state += 1;
                    Update::full()
                }
            }
        }
    }

    struct RootComponent {
        child_link_slot: Rc<RefCell<Option<Link<ChildMsg>>>>,
        root_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
    }

    struct OverlayRootComponent {
        child_link_slot: Rc<RefCell<Option<Link<ChildMsg>>>>,
        root_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
    }

    #[derive(Clone)]
    enum CommitMsg {
        Plain,
        WithComponent,
    }

    #[derive(Clone)]
    enum DevToolsControlMsg {
        Show,
        Hide,
        Toggle,
    }

    struct CommitProbe;

    struct CommitComponent;

    struct DevToolsControlProbe;

    struct ExtraRootThemeProbe;

    #[derive(Clone)]
    struct DynamicRootThemeProbe {
        accent_fg: Rc<Cell<Color>>,
    }

    impl Component for RootComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.root_view_count.set(self.root_view_count.get() + 1);
            crate::child(
                {
                    let child_link_slot = Rc::clone(&self.child_link_slot);
                    let child_view_count = Rc::clone(&self.child_view_count);
                    move || ChildComponent {
                        link_slot: Rc::clone(&child_link_slot),
                        view_count: Rc::clone(&child_view_count),
                    }
                },
                (),
            )
            .key("child")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    impl Component for OverlayRootComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.root_view_count.set(self.root_view_count.get() + 1);
            Modal::new()
                .title("Overlay")
                .child(
                    crate::child(
                        {
                            let child_link_slot = Rc::clone(&self.child_link_slot);
                            let child_view_count = Rc::clone(&self.child_view_count);
                            move || ChildComponent {
                                link_slot: Rc::clone(&child_link_slot),
                                view_count: Rc::clone(&child_view_count),
                            }
                        },
                        (),
                    )
                    .key("overlay-child"),
                )
                .key("overlay-modal")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    impl Component for CommitProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("probe").into()
        }
    }

    impl Component for ExtraRootThemeProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Frame::new()
                .border(true)
                .tab_titles(["Stats", "Logs"])
                .active_tab(1)
                .child(Input::new("logs").border(true))
                .into()
        }
    }

    impl Component for DynamicRootThemeProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let theme = Theme::default().accent(Style::new().fg(self.accent_fg.get()));
            ThemeProvider::new(theme).child(Text::new("root")).into()
        }
    }

    impl Component for CommitComponent {
        type Message = CommitMsg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                CommitMsg::Plain => ctx.append_transcript_element(Text::new("committed")),
                CommitMsg::WithComponent => ctx.append_transcript_element(
                    crate::widgets::Frame::new().child(crate::child(|| CommitProbe, ())),
                ),
            }
            Update::full()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("root").into()
        }
    }

    impl Component for DevToolsControlProbe {
        type Message = DevToolsControlMsg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                DevToolsControlMsg::Show => ctx.show_devtools(),
                DevToolsControlMsg::Hide => ctx.hide_devtools(),
                DevToolsControlMsg::Toggle => ctx.toggle_devtools(),
            }
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("root").into()
        }
    }

    fn collect_texts(element: &Element, out: &mut Vec<String>) {
        if let ElementKind::Text(text) = &element.kind {
            out.push(text.plain_content());
        }
        for child in element.kind.children() {
            collect_texts(child, out);
        }
    }

    struct BubbleLeaf {
        calls: Rc<RefCell<Vec<&'static str>>>,
        handled: bool,
    }

    impl Component for BubbleLeaf {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, _key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            self.calls.borrow_mut().push("leaf");
            if self.handled {
                KeyUpdate::handled(Update::none())
            } else {
                KeyUpdate::unhandled(Update::none())
            }
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("leaf").into()
        }
    }

    struct BubbleParent {
        calls: Rc<RefCell<Vec<&'static str>>>,
        handled: bool,
        leaf_handled: bool,
    }

    impl Component for BubbleParent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, _key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            self.calls.borrow_mut().push("parent");
            if self.handled {
                KeyUpdate::handled(Update::none())
            } else {
                KeyUpdate::unhandled(Update::none())
            }
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::child(
                {
                    let calls = Rc::clone(&self.calls);
                    let leaf_handled = self.leaf_handled;
                    move || BubbleLeaf {
                        calls: Rc::clone(&calls),
                        handled: leaf_handled,
                    }
                },
                (),
            )
            .key("leaf-scope")
        }
    }

    struct BubbleRoot {
        calls: Rc<RefCell<Vec<&'static str>>>,
        parent_handled: bool,
        leaf_handled: bool,
    }

    impl Component for BubbleRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, _key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            self.calls.borrow_mut().push("root");
            KeyUpdate::unhandled(Update::none())
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::child(
                {
                    let calls = Rc::clone(&self.calls);
                    let parent_handled = self.parent_handled;
                    let leaf_handled = self.leaf_handled;
                    move || BubbleParent {
                        calls: Rc::clone(&calls),
                        handled: parent_handled,
                        leaf_handled,
                    }
                },
                (),
            )
            .key("parent-scope")
        }
    }

    fn render_bubble_runtime(
        calls: Rc<RefCell<Vec<&'static str>>>,
        parent_handled: bool,
        leaf_handled: bool,
    ) -> RuntimeCore<BubbleRoot> {
        let mut runtime = RuntimeCore::new_test(
            BubbleRoot {
                calls,
                parent_handled,
                leaf_handled,
            },
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(Cell::new(false)),
        );
        runtime.init();
        runtime.render_element(
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 8,
            },
            None,
            None,
            None,
        );
        runtime
    }

    fn bubble_key_event() -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char('k'),
            mods: KeyMods::default(),
        }
    }

    #[test]
    fn bubble_key_none_focus_uses_deepest_scope_chain() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = render_bubble_runtime(Rc::clone(&calls), false, false);

        let bubble = runtime.bubble_key(None, None, bubble_key_event());

        assert!(!bubble.handled);
        assert_eq!(calls.borrow().as_slice(), ["leaf", "parent", "root"]);
    }

    #[test]
    fn bubble_key_stops_at_handled_intermediate_scope() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = render_bubble_runtime(Rc::clone(&calls), true, false);

        let bubble = runtime.bubble_key(None, None, bubble_key_event());

        assert!(bubble.handled);
        assert_eq!(calls.borrow().as_slice(), ["leaf", "parent"]);
    }

    #[test]
    fn bubble_key_none_focus_uses_focused_key_scope_before_deepest_fallback() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = render_bubble_runtime(Rc::clone(&calls), false, false);

        let focused_key = Key::from("parent-scope");
        let bubble = runtime.bubble_key(None, Some(&focused_key), bubble_key_event());

        assert!(!bubble.handled);
        assert_eq!(calls.borrow().as_slice(), ["parent", "root"]);
    }

    #[test]
    fn refresh_cached_scopes_keeps_root_view_intact() {
        let child_link_slot = Rc::new(RefCell::new(None));
        let root_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));
        let mouse_capture = Rc::new(Cell::new(true));
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };

        let mut runtime = RuntimeCore::new_test(
            RootComponent {
                child_link_slot: Rc::clone(&child_link_slot),
                root_view_count: Rc::clone(&root_view_count),
                child_view_count: Rc::clone(&child_view_count),
            },
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 1);

        let child_link = child_link_slot
            .borrow()
            .clone()
            .expect("child init should publish its link");
        child_link.send(ChildMsg::Increment);

        let (scope, msg) = runtime
            .queue
            .borrow_mut()
            .pop_front()
            .expect("child message should be queued");
        assert!(!matches!(
            runtime
                .update_from_boxed(scope, msg)
                .expect("update should succeed"),
            UpdateLevel::None
        ));
        assert!(runtime.refresh_cached_scopes(&[scope], bounds));
        assert!(runtime.reconcile_cached_element(bounds, None, None, None));

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);

        let mut texts = Vec::new();
        collect_texts(
            runtime
                .cached_expanded_element
                .as_ref()
                .expect("expanded element should be cached"),
            &mut texts,
        );
        assert_eq!(texts, vec!["child:1"]);
    }

    #[derive(Clone)]
    struct MidWrapper {
        child_link_slot: Rc<RefCell<Option<Link<ChildMsg>>>>,
        child_view_count: Rc<Cell<usize>>,
        wrapper_view_count: Rc<Cell<usize>>,
    }

    impl Component for MidWrapper {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.wrapper_view_count
                .set(self.wrapper_view_count.get() + 1);
            Frame::new()
                .child(
                    crate::child(
                        {
                            let child_link_slot = Rc::clone(&self.child_link_slot);
                            let child_view_count = Rc::clone(&self.child_view_count);
                            move || ChildComponent {
                                link_slot: Rc::clone(&child_link_slot),
                                view_count: Rc::clone(&child_view_count),
                            }
                        },
                        (),
                    )
                    .key("deep-child"),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct RootWithMidWrapper {
        child_link_slot: Rc<RefCell<Option<Link<ChildMsg>>>>,
        root_view_count: Rc<Cell<usize>>,
        child_view_count: Rc<Cell<usize>>,
        wrapper_view_count: Rc<Cell<usize>>,
    }

    impl Component for RootWithMidWrapper {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            self.root_view_count.set(self.root_view_count.get() + 1);
            crate::child(
                {
                    let child_link_slot = Rc::clone(&self.child_link_slot);
                    let child_view_count = Rc::clone(&self.child_view_count);
                    let wrapper_view_count = Rc::clone(&self.wrapper_view_count);
                    move || MidWrapper {
                        child_link_slot: Rc::clone(&child_link_slot),
                        child_view_count: Rc::clone(&child_view_count),
                        wrapper_view_count: Rc::clone(&wrapper_view_count),
                    }
                },
                (),
            )
            .key("mid-wrapper")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn refresh_cached_scopes_survives_nonmemoized_intermediate_ancestor() {
        let child_link_slot = Rc::new(RefCell::new(None));
        let root_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));
        let wrapper_view_count = Rc::new(Cell::new(0));
        let mouse_capture = Rc::new(Cell::new(true));
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        let mut runtime = RuntimeCore::new_test(
            RootWithMidWrapper {
                child_link_slot: Rc::clone(&child_link_slot),
                root_view_count: Rc::clone(&root_view_count),
                child_view_count: Rc::clone(&child_view_count),
                wrapper_view_count: Rc::clone(&wrapper_view_count),
            },
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(wrapper_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 1);

        let child_link = child_link_slot
            .borrow()
            .clone()
            .expect("child init should publish its link");
        child_link.send(ChildMsg::Increment);

        let (scope, msg) = runtime
            .queue
            .borrow_mut()
            .pop_front()
            .expect("child message should be queued");
        assert_eq!(
            runtime
                .update_from_boxed(scope, msg)
                .expect("update should succeed"),
            UpdateLevel::Full
        );

        assert!(runtime.refresh_cached_scopes(&[scope], bounds));
        assert!(runtime.reconcile_cached_element(bounds, None, None, None));

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(wrapper_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);

        let mut texts = Vec::new();
        collect_texts(
            runtime
                .cached_expanded_element
                .as_ref()
                .expect("expanded element should be cached"),
            &mut texts,
        );
        assert_eq!(texts, vec!["child:1"]);
    }

    #[test]
    fn refresh_cached_scopes_handles_root_scope_layout_refresh() {
        #[derive(Clone)]
        enum Msg {
            Bump,
        }

        struct RootLayoutProbe {
            root_view_count: Rc<Cell<usize>>,
        }

        impl Component for RootLayoutProbe {
            type Message = Msg;
            type Properties = ();
            type State = usize;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                0
            }

            fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                match msg {
                    Msg::Bump => {
                        ctx.state += 1;
                        Update::layout()
                    }
                }
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                self.root_view_count.set(self.root_view_count.get() + 1);
                Text::new(format!("root:{}", ctx.state)).into()
            }
        }

        let root_view_count = Rc::new(Cell::new(0));
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test(
            RootLayoutProbe {
                root_view_count: Rc::clone(&root_view_count),
            },
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);
        assert_eq!(root_view_count.get(), 1);

        assert_eq!(
            runtime
                .update_from_boxed(ScopeId(1), Box::new(Msg::Bump))
                .expect("root update should succeed"),
            UpdateLevel::Layout
        );

        assert!(runtime.refresh_cached_scopes(&[ScopeId(1)], bounds));
        assert!(runtime.reconcile_cached_element(bounds, None, None, None));
        assert_eq!(root_view_count.get(), 2);

        let mut texts = Vec::new();
        collect_texts(
            runtime
                .cached_expanded_element
                .as_ref()
                .expect("expanded element should be cached"),
            &mut texts,
        );
        assert_eq!(texts, vec!["root:1"]);
    }

    #[test]
    fn refresh_cached_scopes_updates_overlay_content_without_root_rerender() {
        let child_link_slot = Rc::new(RefCell::new(None));
        let root_view_count = Rc::new(Cell::new(0));
        let child_view_count = Rc::new(Cell::new(0));
        let mouse_capture = Rc::new(Cell::new(true));
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };

        let mut runtime = RuntimeCore::new_test(
            OverlayRootComponent {
                child_link_slot: Rc::clone(&child_link_slot),
                root_view_count: Rc::clone(&root_view_count),
                child_view_count: Rc::clone(&child_view_count),
            },
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.render_element(bounds, None, None, None);

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 1);

        let child_link = child_link_slot
            .borrow()
            .clone()
            .expect("child init should publish its link");
        child_link.send(ChildMsg::Increment);

        let (scope, msg) = runtime
            .queue
            .borrow_mut()
            .pop_front()
            .expect("child message should be queued");
        assert!(!matches!(
            runtime
                .update_from_boxed(scope, msg)
                .expect("update should succeed"),
            UpdateLevel::None
        ));
        assert!(runtime.refresh_cached_scopes(&[scope], bounds));
        assert!(runtime.reconcile_cached_element(bounds, None, None, None));

        assert_eq!(root_view_count.get(), 1);
        assert_eq!(child_view_count.get(), 2);

        let mut texts = Vec::new();
        collect_texts(
            runtime
                .cached_expanded_element
                .as_ref()
                .expect("expanded element should be cached"),
            &mut texts,
        );
        assert_eq!(texts, vec!["child:1"]);
    }

    #[test]
    fn commit_queues_plain_widget_subtrees_in_inline_mode() {
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test_transcript(
            CommitComponent,
            (),
            Rect::default(),
            Theme::default(),
            mouse_capture,
        );

        assert!(!matches!(
            runtime
                .update_from_boxed(crate::callback::ScopeId(1), Box::new(CommitMsg::Plain))
                .expect("update should succeed"),
            UpdateLevel::None
        ));

        let commits = runtime.take_pending_transcript_entries();
        assert_eq!(commits.len(), 1);
        assert!(matches!(commits.front(), Some(TranscriptEntry::Element(_))));
    }

    #[test]
    fn commit_rejects_subtrees_that_still_contain_components() {
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test_transcript(
            CommitComponent,
            (),
            Rect::default(),
            Theme::default(),
            mouse_capture,
        );

        assert!(!matches!(
            runtime
                .update_from_boxed(
                    crate::callback::ScopeId(1),
                    Box::new(CommitMsg::WithComponent),
                )
                .expect("update should succeed"),
            UpdateLevel::None
        ));

        assert!(runtime.take_pending_transcript_entries().is_empty());
    }

    #[test]
    fn transcript_replay_document_orders_history_before_live_viewport() {
        #[derive(Clone)]
        enum Msg {
            Seed,
            SetLive(&'static str),
        }

        struct ReplayProbe;

        impl Component for ReplayProbe {
            type Message = Msg;
            type Properties = ();
            type State = String;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                "initial".to_string()
            }

            fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                match msg {
                    Msg::Seed => {
                        ctx.append_transcript_lines(["line-a", "line-b"]);
                        ctx.append_transcript_element(Text::new("elem-1"));
                    }
                    Msg::SetLive(label) => {
                        ctx.state.clear();
                        ctx.state.push_str(label);
                    }
                }
                Update::full()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                Text::new(format!("live:{}", ctx.state)).into()
            }
        }

        fn summarize(entry: &TranscriptEntry) -> String {
            match entry {
                TranscriptEntry::Lines(lines) => {
                    let joined = lines
                        .iter()
                        .map(|line| line.plain_content().into_owned())
                        .collect::<Vec<_>>()
                        .join("|");
                    format!("lines:{joined}")
                }
                TranscriptEntry::Element(element) => {
                    let mut texts = Vec::new();
                    collect_texts(element, &mut texts);
                    format!("element:{}", texts.join("|"))
                }
            }
        }

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test_transcript(
            ReplayProbe,
            (),
            bounds,
            Theme::default(),
            mouse_capture,
        );
        runtime.init();
        runtime.render_element(bounds, None, None, None);

        assert!(!matches!(
            runtime
                .update_from_boxed(crate::callback::ScopeId(1), Box::new(Msg::Seed))
                .expect("seed transcript history"),
            UpdateLevel::None
        ));
        assert!(!matches!(
            runtime
                .update_from_boxed(crate::callback::ScopeId(1), Box::new(Msg::SetLive("after")))
                .expect("update live viewport"),
            UpdateLevel::None
        ));
        runtime.render_element(bounds, None, None, None);

        assert_eq!(
            runtime
                .transcript_replay_document(false)
                .iter()
                .map(summarize)
                .collect::<Vec<_>>(),
            vec!["lines:line-a|line-b", "element:elem-1"]
        );
        assert_eq!(
            runtime
                .transcript_replay_document(true)
                .iter()
                .map(summarize)
                .collect::<Vec<_>>(),
            vec![
                "lines:line-a|line-b".to_string(),
                "element:elem-1".to_string(),
                "element:live:after".to_string(),
            ]
        );
    }

    #[test]
    fn context_devtools_visibility_requests_are_recorded() {
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test(
            DevToolsControlProbe,
            (),
            Rect::default(),
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        assert!(matches!(
            runtime
                .update_from_boxed(
                    crate::callback::ScopeId(1),
                    Box::new(DevToolsControlMsg::Show)
                )
                .expect("show request should succeed"),
            UpdateLevel::None
        ));
        assert!(matches!(
            runtime.ctx.take_devtools_request(),
            Some(crate::core::runtime_env::DevToolsRequest::Show)
        ));

        assert!(matches!(
            runtime
                .update_from_boxed(
                    crate::callback::ScopeId(1),
                    Box::new(DevToolsControlMsg::Hide)
                )
                .expect("hide request should succeed"),
            UpdateLevel::None
        ));
        assert!(matches!(
            runtime.ctx.take_devtools_request(),
            Some(crate::core::runtime_env::DevToolsRequest::Hide)
        ));

        assert!(matches!(
            runtime
                .update_from_boxed(
                    crate::callback::ScopeId(1),
                    Box::new(DevToolsControlMsg::Toggle),
                )
                .expect("toggle request should succeed"),
            UpdateLevel::None
        ));
        assert!(matches!(
            runtime.ctx.take_devtools_request(),
            Some(crate::core::runtime_env::DevToolsRequest::Toggle)
        ));
    }

    #[test]
    fn extra_root_elements_receive_theme_provider_styling() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let theme = Theme::default();
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test(
            CommitProbe,
            (),
            bounds,
            theme.clone(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.extra_root_element = Some(crate::child(|| ExtraRootThemeProbe, ()));
        runtime.render_element(bounds, None, None, None);

        let frame = runtime
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Frame(frame) => Some((node, frame)),
                _ => None,
            })
            .expect("extra root frame should exist");
        assert_eq!(
            crate::style::resolve::resolve_accent_style(
                frame.0.active_theme(),
                frame.1.active_tab_style
            )
            .fg,
            theme.accent.fg
        );
        assert_eq!(
            crate::style::resolve::resolve_muted_style(
                frame.0.active_theme(),
                frame.1.inactive_tab_style
            )
            .fg,
            theme.muted.fg.or(theme.primary.fg)
        );
        assert_eq!(
            frame
                .1
                .focus_style()
                .or(Some(Style::default()))
                .map(|style| {
                    crate::style::resolve::resolve_slot(
                        frame.0.active_theme(),
                        crate::style::ThemeRole::Focus,
                        &crate::style::StyleSlot::Extend(style),
                    )
                })
                .and_then(|style| style.fg),
            theme.focus.fg
        );

        let input = runtime
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Input(input) => Some((node, input)),
                _ => None,
            })
            .expect("extra root input should exist");
        assert_eq!(
            crate::style::resolve::resolve_base_style(input.0.active_theme(), input.1.style).fg,
            theme.primary.fg
        );
        assert_eq!(
            crate::style::resolve_slot(
                input.0.active_theme(),
                crate::style::ThemeRole::Focus,
                &input.1.focus_style,
            )
            .fg,
            theme.focus.fg
        );
    }

    #[test]
    fn extra_root_elements_follow_live_root_theme_provider_changes() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let accent_fg = Rc::new(Cell::new(Color::Rgb(10, 20, 30)));
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test(
            DynamicRootThemeProbe {
                accent_fg: Rc::clone(&accent_fg),
            },
            (),
            bounds,
            Theme::default(),
            SurfaceMode::Fullscreen,
            mouse_capture,
        );

        runtime.init();
        runtime.extra_root_element = Some(crate::child(|| ExtraRootThemeProbe, ()));
        runtime.render_element(bounds, None, None, None);

        accent_fg.set(Color::Rgb(40, 50, 60));
        runtime.render_element(bounds, None, None, None);

        let frame = runtime
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                NodeKind::Frame(frame) => Some((node, frame)),
                _ => None,
            })
            .expect("extra root frame should exist");
        assert_eq!(
            crate::style::resolve::resolve_accent_style(
                frame.0.active_theme(),
                frame.1.active_tab_style
            )
            .fg,
            Some(crate::style::Paint::Solid(Color::Rgb(40, 50, 60)))
        );
    }

    #[test]
    fn transcript_history_snapshot_preserves_line_styles() {
        #[derive(Clone)]
        enum Msg {
            SeedStyled,
        }

        struct StyledProbe;

        impl Component for StyledProbe {
            type Message = Msg;
            type Properties = ();
            type State = ();

            fn create_state(&self, _props: &Self::Properties) -> Self::State {}

            fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                match msg {
                    Msg::SeedStyled => {
                        let styled = RichText::new().span(
                            Span::new("styled-line")
                                .style(Style::new().fg(Color::Rgb(12, 200, 120))),
                        );
                        ctx.append_transcript_lines([styled]);
                    }
                }
                Update::full()
            }

            fn view(&self, _ctx: &Context<Self>) -> Element {
                Text::new("live").into()
            }
        }

        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        };
        let mouse_capture = Rc::new(Cell::new(true));
        let mut runtime = RuntimeCore::new_test_transcript(
            StyledProbe,
            (),
            bounds,
            Theme::default(),
            mouse_capture,
        );
        runtime.init();

        assert!(!matches!(
            runtime
                .update_from_boxed(crate::callback::ScopeId(1), Box::new(Msg::SeedStyled))
                .expect("append styled transcript line"),
            UpdateLevel::None
        ));

        let snapshot = runtime.transcript_history_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert!(matches!(
            snapshot.first(),
            Some(TranscriptEntry::Lines(lines))
                if lines.len() == 1
                    && lines[0].spans.len() == 1
                    && lines[0].spans[0].content.as_ref() == "styled-line"
                    && lines[0].spans[0].style.fg == Some(Color::Rgb(12, 200, 120).into())
        ));
    }

    #[test]
    fn inline_transcript_append_rejects_component_nodes() {
        super::assert_inline_transcript_append_rejects_component_nodes();
    }
}
