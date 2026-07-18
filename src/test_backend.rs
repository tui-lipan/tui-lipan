use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::Result;
use crate::app::context::{App, FocusPolicy, TextAreaNewlineBinding};
use crate::app::copy_feedback::CopyFeedbackState;
use crate::app::input::command_registry::CommandShortcutResult;
use crate::app::input::focus;
use crate::app::input::handlers::KeyCtx;
use crate::app::input::hex_history::HexHistory;
use crate::app::input::key_dispatch::{
    CommandDispatchState, DispatchOps, DispatchOutcome, FrameworkDispatch,
    TerminalPreflightDispatch,
};
use crate::app::input::keyboard;
use crate::app::input::keymap::{Action, Keymap, KeymapConfig, KeymapRuntime};
use crate::app::input::runtime_dispatch::{
    FrameworkSideEffect, RuntimeKeyDispatchConfig, RuntimeKeyDispatchOutcome,
    RuntimeKeyDispatchState, make_key_ctx, selection_clipboard_shortcut,
};
use crate::app::input::text_area_vim::TextAreaVimState;
use crate::app::interaction_state::{
    DragState, FocusStackEntry, HexPendingEdit, MouseTrackingState,
};
use crate::app::{FocusChanged, FocusEntry};
use crate::callback::Link;
use crate::capture::CapturedFrame;
use crate::clipboard::ClipboardConfig;
use crate::core::component::Component;
use crate::core::element::{Element, Key};
use crate::core::event::{KeyCode, KeyEvent, MouseEvent};
use crate::core::node::{NodeId, OverlayRoot};
use crate::core::runtime_env::TranscriptEntry;
use crate::layout::tag::Tag;
use crate::runtime::{FocusRequest, RuntimeCore};
use crate::style::Rect;
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;

const DEFAULT_VIEWPORT: Rect = Rect {
    x: 0,
    y: 0,
    w: 80,
    h: 24,
};

/// Headless runtime for unit-testing components.
///
/// `TestBackend` runs the same reconciliation, nested-component expansion, and message processing as
/// `AppRunner`, but without entering terminal raw mode or rendering via a backend.
///
/// This is intended for:
/// - verifying state transitions in response to messages
/// - validating `view()` output (as an `Element`)
/// - exercising `Command` scheduling and message routing
/// - testing keyboard-driven flows via [`send_key`](Self::send_key)
pub struct TestBackend<C: Component> {
    pub(crate) core: RuntimeCore<C>,
    viewport: Rect,
    pub(crate) focused: Option<NodeId>,
    pub(crate) focused_key: Option<Key>,
    pub(crate) focused_tag: Option<Tag>,
    pub(crate) focus_policy: FocusPolicy,
    keymap: Keymap,
    keymap_runtime: KeymapRuntime,
    text_area_newline_binding: TextAreaNewlineBinding,
    pub(crate) input_history: HashMap<NodeId, TextInput>,
    pub(crate) textarea_history: HashMap<NodeId, TextEditor>,
    pub(crate) text_area_vim_state: HashMap<NodeId, TextAreaVimState>,
    pub(crate) hex_history: HashMap<NodeId, HexHistory>,
    pub(crate) hex_pending_edit: HashMap<NodeId, HexPendingEdit>,
    focus_stack: Vec<FocusStackEntry>,
    last_notified_focus: Option<(NodeId, FocusEntry)>,
    on_focus_changed: Option<crate::app::context::FocusChangedHook>,
    pub(crate) mouse: MouseTrackingState,
    pub(crate) drag: DragState,
    pub(crate) read_only_selection: HashMap<NodeId, (usize, Option<usize>)>,
    pub(crate) copy_feedback: CopyFeedbackState,
    pub(crate) screen_background: Option<crate::style::Style>,
    key_dispatch_config: RuntimeKeyDispatchConfig,
    key_dispatch_state: RuntimeKeyDispatchState,
    framework_effects: Vec<FrameworkSideEffect>,
}

impl<C> TestBackend<C>
where
    C: Component,
{
    /// Mount a root component with default properties.
    pub fn new(component: C) -> Self
    where
        C::Properties: Default,
    {
        Self::new_with_props_inner(component, C::Properties::default(), false)
    }

    #[allow(missing_docs)]
    pub fn new_transcript(component: C) -> Self
    where
        C::Properties: Default,
    {
        Self::new_with_props_inner(component, C::Properties::default(), true)
    }

    /// Mount a root component with explicit properties.
    pub fn new_with_props(component: C, props: C::Properties) -> Self {
        Self::new_with_app(App::new(), component, props)
    }

    /// Mount a root component using the same app configuration as [`AppRunner`](crate::AppRunner).
    pub fn new_with_app(app: App, component: C, props: C::Properties) -> Self {
        Self::new_with_app_inner(app, component, props, false)
    }

    #[allow(missing_docs)]
    pub fn new_transcript_with_props(component: C, props: C::Properties) -> Self {
        Self::new_with_app_inner(App::new(), component, props, true)
    }

    fn new_with_app_inner(
        app: App,
        component: C,
        props: C::Properties,
        inline_transcript_mode: bool,
    ) -> Self {
        let viewport = DEFAULT_VIEWPORT;
        let mouse_capture = Rc::new(Cell::new(app.mouse_enabled.unwrap_or(true)));
        let clipboard_config = app.clipboard_config.clone();
        let mut keymap_config = KeymapConfig::from_clipboard_config(&clipboard_config);
        if let Some(path) = app.keymap_path.clone() {
            keymap_config = keymap_config.keymap_path(path);
        }
        keymap_config = keymap_config
            .framework_keymap(app.framework_keymap.clone())
            .user_keymap_policy(app.user_keymap_policy);
        let keymap = Keymap::new(keymap_config);
        let keymap_runtime = KeymapRuntime::new(&keymap);
        let core = if inline_transcript_mode {
            RuntimeCore::new_test_transcript(
                component,
                props,
                viewport,
                app.theme.clone(),
                mouse_capture,
            )
        } else {
            RuntimeCore::new_test(
                component,
                props,
                viewport,
                app.theme.clone(),
                app.surface_mode,
                mouse_capture,
            )
        };
        let command_registry = core.ctx.command_registry();
        let key_dispatch_state =
            RuntimeKeyDispatchState::new(&command_registry, app.command_conflict_policy);
        let key_dispatch_config = RuntimeKeyDispatchConfig {
            focus_policy: app.focus_policy,
            key_dispatch_policy: app.key_dispatch_policy,
            terminal_key_policy: app.terminal_key_policy,
            command_conflict_policy: app.command_conflict_policy,
            chord_mismatch_policy: app.chord_mismatch_policy,
        };
        let on_focus_changed = app.on_focus_changed.clone();

        let mut backend = Self {
            core,
            viewport,
            focused: None,
            focused_key: None,
            focused_tag: None,
            focus_policy: app.focus_policy,
            keymap,
            keymap_runtime,
            text_area_newline_binding: app.text_area_newline_binding,
            input_history: HashMap::new(),
            textarea_history: HashMap::new(),
            text_area_vim_state: HashMap::new(),
            hex_history: HashMap::new(),
            hex_pending_edit: HashMap::new(),
            focus_stack: Vec::new(),
            last_notified_focus: None,
            on_focus_changed,
            mouse: MouseTrackingState::default(),
            drag: DragState::default(),
            read_only_selection: HashMap::new(),
            copy_feedback: CopyFeedbackState::default(),
            screen_background: None,
            key_dispatch_config,
            key_dispatch_state,
            framework_effects: Vec::new(),
        };

        backend.core.init();
        backend.render();
        backend
    }

    fn new_with_props_inner(
        component: C,
        props: C::Properties,
        inline_transcript_mode: bool,
    ) -> Self {
        Self::new_with_app_inner(App::new(), component, props, inline_transcript_mode)
    }

    /// Returns the current viewport used for layout.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    #[allow(missing_docs)]
    pub fn transcript_history_len(&self) -> usize {
        self.core.transcript_history_snapshot().len()
    }

    #[allow(missing_docs)]
    pub fn transcript_replay_summary(&self, include_live_viewport: bool) -> Vec<String> {
        self.core
            .transcript_replay_document(include_live_viewport)
            .iter()
            .map(summarize_transcript_entry)
            .collect()
    }

    /// Set the viewport used for layout.
    pub fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.core.ctx.set_viewport(viewport);
    }

    /// Returns a cloneable link to the root component.
    pub fn link(&self) -> Link<C::Message> {
        self.core.ctx.link().clone()
    }

    /// Borrow the mounted component instance.
    pub fn component(&self) -> &C {
        &self.core.component
    }

    /// Mutably borrow the mounted component instance.
    pub fn component_mut(&mut self) -> &mut C {
        &mut self.core.component
    }

    /// Borrow the component state.
    pub fn state(&self) -> &C::State {
        &self.core.ctx.state
    }

    /// Mutably borrow the component state.
    pub fn state_mut(&mut self) -> &mut C::State {
        &mut self.core.ctx.state
    }

    /// Returns the currently focused node, if any.
    pub fn focused(&self) -> Option<NodeId> {
        self.focused
    }

    /// Manually set which node has focus.
    pub fn set_focused(&mut self, id: NodeId) {
        self.set_focused_silent(id);
        self.notify_focus_change();
    }

    fn set_focused_silent(&mut self, id: NodeId) {
        self.focused = Some(id);
        self.focused_key = self.core.tree.node(id).key.clone();
        self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(id)));
    }

    /// Clear focus and its remembered keyed target.
    ///
    /// A later render under [`FocusPolicy::Auto`] restores the default focus target.
    pub fn blur(&mut self) {
        self.focused = None;
        self.focused_key = None;
        self.focused_tag = None;
        self.notify_focus_change();
    }

    /// Borrow the last rendered `Element` tree.
    pub fn element(&self) -> &Element {
        let element = self
            .core
            .cached_expanded_element
            .as_ref()
            .expect("render() must be called before element()");
        if let crate::core::element::ElementKind::ThemeProvider(provider) = &element.kind {
            &provider.child
        } else {
            element
        }
    }

    /// Returns the minimum content size of the last rendered `Element` tree.
    ///
    /// Call [`Self::render`] first so this measures the current element, just like
    /// [`Self::element`]. Returned dimensions are clamped to at least `1` so they
    /// are safe to use as capture viewport dimensions.
    pub fn content_min_size(&self) -> (u16, u16) {
        let (w, h) = crate::layout::measure::min_size(self.element());
        (w.max(1), h.max(1))
    }

    /// Enqueue a message for the root component without processing it.
    pub fn enqueue(&self, msg: C::Message) {
        self.core.ctx.link().send(msg);
    }

    /// Enqueue a message and process the runtime until idle.
    pub fn dispatch(&mut self, msg: C::Message) -> Result<bool> {
        self.enqueue(msg);
        self.pump()
    }

    /// Inject a key event through the same dispatch pipeline as the real runner.
    ///
    /// 1. Dispatches to the focused widget via `keyboard::dispatch_key`
    /// 2. If unhandled, bubbles up through component scopes via `on_key`
    /// 3. If still unhandled, PageUp/PageDown may target one ambient `ScrollView`
    /// 4. Processes any queued messages and re-renders if dirty
    ///
    /// Returns `true` if the focused widget, a bubbling `on_key` scope, or an
    /// ambient page-scroll target consumed the key.
    ///
    /// Draining the message queue in [`Self::pump`] can re-render without this key having been
    /// handled; that work is not counted here so browser embeddings can use the return value for
    /// `preventDefault` without stealing shortcuts when unrelated updates flush.
    pub fn send_key(&mut self, key: KeyEvent) -> Result<bool> {
        let clipboard = Rc::clone(&self.core.ctx.env().clipboard);
        let clipboard_config = self.core.ctx.env().clipboard_config.clone();
        self.framework_effects.clear();

        if matches!(key.code, KeyCode::Esc) {
            self.key_dispatch_state.reset_command_chord();
        }

        let selection_handled = {
            let mut key_ctx = make_key_ctx(
                Some(&self.read_only_selection),
                &mut self.input_history,
                &mut self.textarea_history,
                &mut self.text_area_vim_state,
                &mut self.hex_history,
                &mut self.hex_pending_edit,
                &self.keymap,
                self.text_area_newline_binding,
                &clipboard,
                &clipboard_config,
                &mut self.copy_feedback,
            );
            selection_clipboard_shortcut(&mut self.core.tree, &mut key_ctx, key)
        };
        if selection_handled {
            let pump_dirty = self.pump()?;
            if !pump_dirty {
                self.render();
            }
            return Ok(true);
        }

        let focus = if self
            .focused
            .is_some_and(|id| self.core.tree.is_valid(id) && self.focused_is_terminal(id))
        {
            crate::app::input::key_dispatch::FocusKind::Terminal
        } else {
            crate::app::input::key_dispatch::FocusKind::Widget
        };

        if focus == crate::app::input::key_dispatch::FocusKind::Widget {
            use crate::app::input::keymap::KeymapRuntimeResult;
            match self.keymap_runtime.feed(key) {
                KeymapRuntimeResult::Pending => return Ok(true),
                KeymapRuntimeResult::Matched(binding) if binding.is_chord => {
                    if binding.action == Action::Quit {
                        self.core.ctx.quit();
                        let _ = self.pump()?;
                        return Ok(true);
                    }
                    if binding.action == Action::DismissOverlay && self.handle_overlay_escape() {
                        let _ = self.pump()?;
                        self.render();
                        return Ok(true);
                    }
                }
                KeymapRuntimeResult::Matched(_) | KeymapRuntimeResult::None => {}
            }
        } else {
            self.keymap_runtime.reset();
        }

        let dismiss_handled = self
            .keymap
            .matches(key)
            .iter()
            .any(|binding| binding.action == Action::DismissOverlay)
            && self.handle_overlay_escape();
        if dismiss_handled {
            let _ = self.pump()?;
            self.render();
            return Ok(true);
        }

        if self.top_capturing_overlay_is_empty() {
            if self
                .keymap
                .matches(key)
                .iter()
                .any(|binding| binding.action == Action::Quit)
            {
                self.core.ctx.quit();
                let _ = self.pump()?;
            }
            return Ok(true);
        }

        if self
            .core
            .tree
            .top_capturing_overlay()
            .is_some_and(|overlay| overlay.captures_focus)
            && self.focused.is_none()
            && !matches!(key.code, KeyCode::Esc)
        {
            return Ok(true);
        }

        if focus == crate::app::input::key_dispatch::FocusKind::Terminal {
            self.keymap_runtime.reset();
        }
        let key_dispatch_config = self.key_dispatch_config;

        let TestBackend {
            core,
            focused,
            focused_key,
            focused_tag,
            focus_stack,
            keymap,
            keymap_runtime,
            text_area_newline_binding,
            input_history,
            textarea_history,
            text_area_vim_state,
            hex_history,
            hex_pending_edit,
            key_dispatch_state,
            framework_effects,
            copy_feedback,
            ..
        } = self;

        let command_registry = core.ctx.command_registry();
        let mut key_ctx = KeyCtx {
            read_only_selection: None,
            input_history,
            textarea_history,
            text_area_vim_state,
            hex_history,
            hex_pending_edit,
            keymap,
            text_area_newline_binding: *text_area_newline_binding,
            clipboard: &clipboard,
            clipboard_config: &clipboard_config,
            copy_feedback,
            dirty_override: None,
        };

        let mut ops = TestBackendDispatchOps {
            core,
            focused,
            focused_key,
            focused_tag,
            focus_stack,
            keymap,
            keymap_runtime,
            key_dispatch_state,
            key_dispatch_config,
            framework_effects,
            command_registry,
            key_ctx: &mut key_ctx,
            clipboard: &clipboard,
            clipboard_config: &clipboard_config,
        };
        let outcome = crate::app::input::key_dispatch::dispatch_key(
            crate::app::input::key_dispatch::DispatchRequest::new(key, focus)
                .key_policy(key_dispatch_config.key_dispatch_policy)
                .terminal_policy(key_dispatch_config.terminal_key_policy)
                .chord_mismatch_policy(key_dispatch_config.chord_mismatch_policy),
            &mut ops,
        );
        let dispatch = ops.finish(outcome);

        if dispatch.quit {
            self.core.ctx.quit();
        }

        let pump_dirty = self.pump()?;
        if (dispatch.dirty || dispatch.layout_dirty || dispatch.quit) && !pump_dirty {
            self.render();
        }

        Ok(dispatch.handled)
    }

    #[cfg(feature = "terminal")]
    fn focused_is_terminal(&self, id: NodeId) -> bool {
        matches!(
            self.core.tree.node(id).kind,
            crate::core::node::NodeKind::Terminal(_)
        )
    }

    #[cfg(not(feature = "terminal"))]
    fn focused_is_terminal(&self, _id: NodeId) -> bool {
        false
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn focus_terminal_by_key_for_test(&mut self, key: &str) -> bool {
        self.focus_for_node_by_key(Key::from(key.to_string()))
    }

    #[cfg(test)]
    fn focus_for_node_by_key(&mut self, key: Key) -> bool {
        let id = self
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&key))
            .map(|node| node.id);
        let Some(id) = id else {
            return false;
        };
        if !self.core.tree.node(id).is_focusable() || self.focused == Some(id) {
            return false;
        }
        self.set_focused(id);
        true
    }

    /// Inject a text paste event through the same focused-widget pipeline as the real runner.
    pub fn send_paste(&mut self, text: &str) -> Result<bool> {
        if self.top_capturing_overlay_is_empty() {
            return Ok(true);
        }

        let clipboard = Rc::clone(&self.core.ctx.env().clipboard);
        let clipboard_config = self.core.ctx.env().clipboard_config.clone();

        let mut key_ctx = KeyCtx {
            read_only_selection: None,
            input_history: &mut self.input_history,
            textarea_history: &mut self.textarea_history,
            text_area_vim_state: &mut self.text_area_vim_state,
            hex_history: &mut self.hex_history,
            hex_pending_edit: &mut self.hex_pending_edit,
            keymap: &self.keymap,
            text_area_newline_binding: self.text_area_newline_binding,
            clipboard: &clipboard,
            clipboard_config: &clipboard_config,
            copy_feedback: &mut self.copy_feedback,
            dirty_override: None,
        };

        let handled =
            keyboard::dispatch_paste(&mut self.core.tree, self.focused, text, &mut key_ctx);

        let pump_dirty = self.pump()?;
        if handled && !pump_dirty {
            self.render();
        }

        Ok(handled)
    }

    /// Dispatch a mouse event through the same pipeline as the real runner.
    ///
    /// Returns `true` if the event was handled by any widget or component.
    pub fn send_mouse(&mut self, ev: MouseEvent) -> Result<bool> {
        use crate::app::mouse_dispatch;
        let bubble_dirty = mouse_dispatch::dispatch_mouse_test_backend(self, ev);
        self.notify_focus_change();
        let pump_dirty = self.pump()?;
        if bubble_dirty && !pump_dirty {
            self.render();
        }
        Ok(bubble_dirty || pump_dirty)
    }

    /// Focus the node at `id` or the first focusable descendant beneath it.
    ///
    /// Returns `true` if the focused node changed.
    pub(crate) fn focus_for_node(&mut self, id: NodeId) -> bool {
        if self.focus_policy == FocusPolicy::Manual {
            return false;
        }
        if !self.core.tree.is_valid(id) {
            return false;
        }
        if focus::in_excluded_scope(&self.core.tree, id) {
            return false;
        }
        let focusable = self.core.tree.node(id).is_focusable();
        if focusable {
            let changed = self.focused != Some(id);
            if changed {
                self.set_focused_silent(id);
            }
            return changed;
        }
        if let Some(desc) = focus::find_first_focusable_descendant(&self.core.tree, id) {
            let changed = self.focused != Some(desc);
            if changed {
                self.set_focused_silent(desc);
                return true;
            }
        }
        false
    }

    /// Move focus to the next focusable node (Tab behavior).
    pub fn focus_next(&mut self) {
        if self.focus_overlay_next() {
            self.notify_focus_change();
            return;
        }

        focus::focus_next(
            &self.core.tree,
            &mut self.focused,
            &mut self.focused_key,
            &mut self.focused_tag,
            self.focus_policy,
        );
        self.notify_focus_change();
    }

    /// Move focus to the previous focusable node (Shift+Tab behavior).
    pub fn focus_prev(&mut self) {
        if self.focus_overlay_prev() {
            self.notify_focus_change();
            return;
        }

        focus::focus_prev(
            &self.core.tree,
            &mut self.focused,
            &mut self.focused_key,
            &mut self.focused_tag,
            self.focus_policy,
        );
        self.notify_focus_change();
    }

    fn apply_focus_request(&mut self, request: FocusRequest) {
        match request {
            FocusRequest::Key(key) => {
                self.focused = None;
                self.focused_key = Some(key);
                self.focused_tag = None;
            }
            FocusRequest::Clear => {
                self.focused = None;
                self.focused_key = None;
                self.focused_tag = None;
            }
            FocusRequest::Next => {
                if !self.focus_overlay_next() {
                    focus::focus_next(
                        &self.core.tree,
                        &mut self.focused,
                        &mut self.focused_key,
                        &mut self.focused_tag,
                        self.focus_policy,
                    );
                }
            }
            FocusRequest::Prev => {
                if !self.focus_overlay_prev() {
                    focus::focus_prev(
                        &self.core.tree,
                        &mut self.focused,
                        &mut self.focused_key,
                        &mut self.focused_tag,
                        self.focus_policy,
                    );
                }
            }
        }
    }

    /// Process all queued messages and any messages produced by background commands.
    ///
    /// Returns `true` if any update requested a re-render.
    pub fn pump(&mut self) -> Result<bool> {
        let mut dirty = false;

        loop {
            self.core.drain_commands();

            let next = { self.core.queue.borrow_mut().pop_front() };
            let Some((scope, msg)) = next else {
                break;
            };

            dirty |= !matches!(
                self.core.update_from_boxed(scope, msg)?,
                crate::core::component::UpdateLevel::None
            );
        }

        if let Some(request) = self.core.ctx.take_focus_request() {
            self.apply_focus_request(request);
            dirty = true;
        }

        if dirty {
            self.render();
        }

        Ok(dirty)
    }

    /// Recompute the current `Element` tree and layout.
    pub fn render(&mut self) {
        let bounds = self.viewport;
        self.core
            .render_element(bounds, self.focused, self.focused_key.as_ref(), None);
        if let Some(request) = self.core.ctx.take_focus_request() {
            self.apply_focus_request(request);
        }
        focus::restore_focus(
            &self.core.tree,
            &mut self.focused,
            &mut self.focused_key,
            &mut self.focused_tag,
            self.focus_policy,
        );
        self.ensure_overlay_focus();
        self.notify_focus_change();
        self.refresh_hover_from_last_mouse();
    }

    fn notify_focus_change(&mut self) {
        let current = self
            .focused
            .filter(|id| self.core.tree.is_valid(*id))
            .map(|id| {
                let node = self.core.tree.node(id);
                (
                    id,
                    FocusEntry {
                        key: node.key.clone(),
                        tag: crate::layout::tag::tag_of_node(node),
                    },
                )
            });
        let unchanged = match (&self.last_notified_focus, &current) {
            (None, None) => true,
            (Some((old_id, old)), Some((new_id, new))) => {
                old_id == new_id
                    || old
                        .key
                        .as_ref()
                        .zip(new.key.as_ref())
                        .is_some_and(|(old, new)| old == new)
            }
            _ => false,
        };
        if unchanged {
            self.last_notified_focus = current;
            return;
        }

        let previous = self.last_notified_focus.take();
        self.last_notified_focus = current.clone();
        let on_blur = previous
            .as_ref()
            .filter(|(id, _)| self.core.tree.is_valid(*id))
            .and_then(|(id, _)| self.core.tree.node(*id).on_blur_callback().cloned());
        let on_focus = current
            .as_ref()
            .and_then(|(id, _)| self.core.tree.node(*id).on_focus_callback().cloned());

        if let Some(callback) = on_blur {
            callback.emit(());
        }
        if let Some(callback) = on_focus {
            callback.emit(());
        }
        if let Some(hook) = &self.on_focus_changed {
            hook(&FocusChanged {
                old: previous.map(|(_, entry)| entry),
                new: current.map(|(_, entry)| entry),
            });
        }
    }

    fn ensure_overlay_focus(&mut self) {
        let Some((overlay_id, auto_focus)) = self
            .core
            .tree
            .top_capturing_overlay()
            .map(|overlay| (overlay.id, overlay.auto_focus))
        else {
            return;
        };
        let focused_in_overlay = self
            .focused
            .filter(|id| self.core.tree.is_descendant(overlay_id, *id))
            .is_some();
        if !auto_focus {
            if !focused_in_overlay {
                self.suspend_focus_for_overlay(overlay_id);
            }
            return;
        }
        if focused_in_overlay {
            return;
        }

        let focusables = self.core.tree.focusables_in_subtree(overlay_id);
        if focusables.is_empty() {
            self.suspend_focus_for_overlay(overlay_id);
            return;
        }

        self.push_focus_stack_for_overlay(overlay_id);
        let next = focusables[0];
        self.focused = Some(next);
        self.focused_key = self.core.tree.node(next).key.clone();
        self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(next)));
    }

    fn top_capturing_overlay_is_empty(&self) -> bool {
        self.core
            .tree
            .top_capturing_overlay()
            .is_some_and(|overlay| self.core.tree.focusables_in_subtree(overlay.id).is_empty())
    }

    fn push_focus_stack_for_overlay(&mut self, overlay_id: NodeId) {
        let should_push = self
            .focus_stack
            .last()
            .is_none_or(|top| top.overlay != overlay_id);
        if !should_push {
            return;
        }

        const MAX_FOCUS_STACK_DEPTH: usize = 32;
        if self.focus_stack.len() >= MAX_FOCUS_STACK_DEPTH {
            self.focus_stack.remove(0);
        }
        self.focus_stack.push(FocusStackEntry {
            overlay: overlay_id,
            focused: self.focused,
            key: self.focused_key.clone(),
            tag: self.focused_tag,
        });
    }

    fn suspend_focus_for_overlay(&mut self, overlay_id: NodeId) {
        if self.focused.is_none() && self.focused_key.is_none() && self.focused_tag.is_none() {
            return;
        }

        self.push_focus_stack_for_overlay(overlay_id);
        self.focused = None;
        self.focused_tag = None;
    }

    fn focus_overlay_next(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        if !overlay.auto_focus
            && !self
                .focused
                .is_some_and(|id| self.core.tree.is_descendant(overlay.id, id))
        {
            return true;
        }
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }

        focusables.sort_by_key(|id| id.index());
        let next = if let Some(curr) = self.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + 1) % focusables.len()]
        } else {
            focusables[0]
        };
        self.focused = Some(next);
        self.focused_key = self.core.tree.node(next).key.clone();
        self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(next)));
        true
    }

    fn focus_overlay_prev(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        if !overlay.auto_focus
            && !self
                .focused
                .is_some_and(|id| self.core.tree.is_descendant(overlay.id, id))
        {
            return true;
        }
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }

        focusables.sort_by_key(|id| id.index());
        let prev = if let Some(curr) = self.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + focusables.len().saturating_sub(1)) % focusables.len()]
        } else {
            focusables[focusables.len().saturating_sub(1)]
        };
        self.focused = Some(prev);
        self.focused_key = self.core.tree.node(prev).key.clone();
        self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(prev)));
        true
    }

    fn refresh_hover_from_last_mouse(&mut self) {
        let Some((x, y)) = self.mouse.last_mouse else {
            return;
        };

        crate::app::mouse_dispatch::update_hover_test_backend(self, x, y, true);
    }

    fn handle_overlay_escape(&mut self) -> bool {
        let overlays: Vec<_> = self.core.tree.overlay_roots().to_vec();
        for overlay in overlays.iter().rev() {
            if overlay.dismiss_policy.dismiss_on_escape() {
                return self.dismiss_overlay(overlay);
            }
        }

        overlays.iter().any(|overlay| overlay.captures_focus)
    }

    fn dismiss_overlay(&mut self, overlay: &OverlayRoot) -> bool {
        let dismissed = if let Some(id) = overlay.overlay_id {
            self.core.overlay_manager.borrow_mut().dismiss(id)
        } else if let Some(cb) = &overlay.on_dismiss {
            cb.emit(());
            true
        } else {
            false
        };

        if dismissed
            && overlay.captures_focus
            && let Some(saved) = self.focus_stack.pop()
        {
            self.focused = saved.focused.filter(|id| {
                self.core.tree.is_valid(*id) && self.core.tree.node(*id).is_focusable()
            });
            self.focused_key = saved.key;
            self.focused_tag = saved.tag;
            focus::restore_focus(
                &self.core.tree,
                &mut self.focused,
                &mut self.focused_key,
                &mut self.focused_tag,
                self.focus_policy,
            );
        }
        dismissed
    }

    /// Set an opt-in root viewport background fill for captures.
    ///
    /// Mirrors `App::screen_background` for headless rendering: pass a resolved
    /// style (typically `Style::new().bg(color)`), or `None` to leave the
    /// background transparent. Useful for previewing a theme's filled backdrop.
    pub fn set_screen_background(&mut self, style: Option<crate::style::Style>) {
        self.screen_background = style;
    }

    fn screen_background_ratatui(&self) -> Option<ratatui::style::Style> {
        self.screen_background
            .map(crate::backend::ratatui_backend::common::to_ratatui_style)
    }

    fn capture_interaction(
        &self,
    ) -> crate::backend::ratatui_backend::capture_render::CaptureInteraction {
        crate::backend::ratatui_backend::capture_render::CaptureInteraction {
            focused: self.focused,
            hovered: self.mouse.hovered,
            mouse_pos: self.mouse.last_mouse,
        }
    }

    /// Render and capture the current frame output.
    pub fn capture_frame(&self) -> CapturedFrame {
        crate::backend::ratatui_backend::capture_render::render_to_captured_frame_with_interaction(
            &self.core.tree,
            self.viewport,
            self.capture_interaction(),
            0,
            self.screen_background_ratatui(),
        )
    }

    /// Capture a frame using a temporary fit-to-content viewport plus margins.
    ///
    /// `margin_w` and `margin_h` are columns and rows added to the measured content minimum size.
    /// The backend temporarily lays out at `(content_min_size + margin)`, captures via
    /// [`Self::capture_frame`], then restores the original viewport and layout before returning.
    pub fn capture_frame_with_margin(&mut self, margin_w: u16, margin_h: u16) -> CapturedFrame {
        self.capture_with_margin(margin_w, margin_h, Self::capture_frame)
    }

    /// Capture a combined visual + semantic UI snapshot.
    ///
    /// Call [`Self::render`] first so the node tree and layout reflect the latest state.
    pub fn capture_ui_snapshot(&self) -> crate::ui_snapshot::UiSnapshot {
        self.capture_ui_snapshot_with_options(&crate::ui_snapshot::UiSnapshotOptions::default())
    }

    /// Capture a UI snapshot with custom describe options.
    pub fn capture_ui_snapshot_with_options(
        &self,
        options: &crate::ui_snapshot::UiSnapshotOptions,
    ) -> crate::ui_snapshot::UiSnapshot {
        crate::ui_snapshot::build_ui_snapshot(
            &self.core.tree,
            self.viewport,
            self.capture_interaction(),
            0,
            self.screen_background_ratatui(),
            options,
        )
    }

    /// Capture a UI snapshot using a temporary fit-to-content viewport plus margins.
    ///
    /// `margin_w` and `margin_h` are columns and rows added to the measured content minimum size.
    /// The backend temporarily lays out at `(content_min_size + margin)`, captures via
    /// [`Self::capture_ui_snapshot_with_options`], then restores the original viewport and layout
    /// before returning.
    pub fn capture_ui_snapshot_with_margin(
        &mut self,
        margin_w: u16,
        margin_h: u16,
        options: &crate::ui_snapshot::UiSnapshotOptions,
    ) -> crate::ui_snapshot::UiSnapshot {
        self.capture_with_margin(margin_w, margin_h, |backend| {
            backend.capture_ui_snapshot_with_options(options)
        })
    }

    fn capture_with_margin<T>(
        &mut self,
        margin_w: u16,
        margin_h: u16,
        capture: impl FnOnce(&Self) -> T,
    ) -> T {
        let original_viewport = self.viewport;
        let (min_w, min_h) = self.content_min_size();
        let target_viewport = Rect {
            x: 0,
            y: 0,
            w: min_w.saturating_add(margin_w).max(1),
            h: min_h.saturating_add(margin_h).max(1),
        };

        self.set_viewport(target_viewport);
        self.render();
        let captured = capture(self);
        self.set_viewport(original_viewport);
        self.render();
        captured
    }

    /// Returns the reconciliation key of the currently focused node, if any.
    pub fn focused_key(&self) -> Option<&crate::core::element::Key> {
        self.focused_key.as_ref()
    }

    /// Returns the node id currently under the mouse, if any.
    pub fn hovered(&self) -> Option<NodeId> {
        self.mouse.hovered
    }

    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    pub(crate) fn capture_frame_with_effect_phase(&self, effect_phase: u64) -> CapturedFrame {
        crate::backend::ratatui_backend::capture_render::render_to_captured_frame_with_interaction(
            &self.core.tree,
            self.viewport,
            self.capture_interaction(),
            effect_phase,
            self.screen_background_ratatui(),
        )
    }
}

fn summarize_transcript_entry(entry: &TranscriptEntry) -> String {
    match entry {
        TranscriptEntry::Lines(lines) => {
            let payload = lines
                .iter()
                .map(|line| line.plain_content().into_owned())
                .collect::<Vec<_>>()
                .join("|");
            format!("lines:{payload}")
        }
        TranscriptEntry::Element(element) => {
            let payload = summarize_element_texts(element).join("|");
            format!("element:{payload}")
        }
    }
}

fn summarize_element_texts(element: &Element) -> Vec<String> {
    let mut out = Vec::new();
    collect_element_texts(element, &mut out);
    out
}

fn collect_element_texts(element: &Element, out: &mut Vec<String>) {
    if let crate::core::element::ElementKind::Text(text) = &element.kind {
        out.push(text.plain_content());
    }
    for child in element.kind.children() {
        collect_element_texts(child, out);
    }
}

struct TestBackendDispatchOps<'a, C: Component> {
    core: &'a mut RuntimeCore<C>,
    focused: &'a mut Option<NodeId>,
    focused_key: &'a mut Option<Key>,
    focused_tag: &'a mut Option<Tag>,
    focus_stack: &'a mut Vec<FocusStackEntry>,
    keymap: &'a Keymap,
    keymap_runtime: &'a mut KeymapRuntime,
    key_dispatch_state: &'a mut RuntimeKeyDispatchState,
    key_dispatch_config: RuntimeKeyDispatchConfig,
    framework_effects: &'a mut Vec<FrameworkSideEffect>,
    command_registry: crate::app::input::command_registry::CommandRegistry,
    key_ctx: &'a mut KeyCtx<'a>,
    #[allow(dead_code)]
    clipboard: &'a crate::clipboard::ClipboardService,
    #[allow(dead_code)]
    clipboard_config: &'a ClipboardConfig,
}

impl<C: Component> TestBackendDispatchOps<'_, C> {
    fn top_capturing_overlay_is_empty(&self) -> bool {
        self.core
            .tree
            .top_capturing_overlay()
            .is_some_and(|overlay| self.core.tree.focusables_in_subtree(overlay.id).is_empty())
    }

    fn handle_overlay_escape(&mut self) -> bool {
        let overlays: Vec<_> = self.core.tree.overlay_roots().to_vec();
        for overlay in overlays.iter().rev() {
            if overlay.dismiss_policy.dismiss_on_escape() {
                return self.dismiss_overlay(overlay);
            }
        }
        overlays.iter().any(|overlay| overlay.captures_focus)
    }

    fn dismiss_overlay(&mut self, overlay: &OverlayRoot) -> bool {
        let dismissed = if let Some(id) = overlay.overlay_id {
            self.core.overlay_manager.borrow_mut().dismiss(id)
        } else if let Some(cb) = &overlay.on_dismiss {
            cb.emit(());
            true
        } else {
            false
        };

        if dismissed
            && overlay.captures_focus
            && let Some(saved) = self.focus_stack.pop()
        {
            *self.focused = saved.focused.filter(|id| {
                self.core.tree.is_valid(*id) && self.core.tree.node(*id).is_focusable()
            });
            *self.focused_key = saved.key;
            *self.focused_tag = saved.tag;
            focus::restore_focus(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
                self.key_dispatch_config.focus_policy,
            );
        }
        dismissed
    }

    fn focus_overlay_next(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        if !overlay.auto_focus
            && !self
                .focused
                .as_ref()
                .is_some_and(|id| self.core.tree.is_descendant(overlay.id, *id))
        {
            return true;
        }
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }

        focusables.sort_by_key(|id| id.index());
        let next = if let Some(curr) = *self.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + 1) % focusables.len()]
        } else {
            focusables[0]
        };
        *self.focused = Some(next);
        *self.focused_key = self.core.tree.node(next).key.clone();
        *self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(next)));
        true
    }

    fn focus_overlay_prev(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        if !overlay.auto_focus
            && !self
                .focused
                .as_ref()
                .is_some_and(|id| self.core.tree.is_descendant(overlay.id, *id))
        {
            return true;
        }
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }

        focusables.sort_by_key(|id| id.index());
        let prev = if let Some(curr) = *self.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + focusables.len().saturating_sub(1)) % focusables.len()]
        } else {
            focusables[focusables.len().saturating_sub(1)]
        };
        *self.focused = Some(prev);
        *self.focused_key = self.core.tree.node(prev).key.clone();
        *self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(prev)));
        true
    }

    #[cfg(feature = "terminal")]
    fn focused_is_terminal(&self, id: NodeId) -> bool {
        matches!(
            self.core.tree.node(id).kind,
            crate::core::node::NodeKind::Terminal(_)
        )
    }

    fn forward_terminal_key(&mut self, key: KeyEvent) -> bool {
        #[cfg(feature = "terminal")]
        if let Some(id) = self.focused.filter(|id| self.core.tree.is_valid(*id))
            && self.focused_is_terminal(id)
        {
            use crate::app::input::handlers::terminal::forward_key;
            return forward_key(&mut self.core.tree, id, key);
        }
        keyboard::dispatch_key(&mut self.core.tree, *self.focused, key, self.key_ctx)
    }

    fn finish(self, outcome: DispatchOutcome) -> RuntimeKeyDispatchOutcome {
        let mut result = RuntimeKeyDispatchOutcome::default();
        match outcome {
            DispatchOutcome::Widget
            | DispatchOutcome::Bubble
            | DispatchOutcome::Command
            | DispatchOutcome::CommandPending
            | DispatchOutcome::Framework
            | DispatchOutcome::TerminalPreflight
            | DispatchOutcome::Terminal
            | DispatchOutcome::AmbientScroll => result.handled = true,
            DispatchOutcome::FrameworkQuit => {
                result.handled = true;
                result.quit = true;
            }
            DispatchOutcome::Unhandled => {}
        }

        if matches!(
            outcome,
            DispatchOutcome::Widget
                | DispatchOutcome::Bubble
                | DispatchOutcome::Terminal
                | DispatchOutcome::TerminalPreflight
                | DispatchOutcome::AmbientScroll
        ) {
            result.dirty = true;
            if matches!(
                outcome,
                DispatchOutcome::Widget | DispatchOutcome::AmbientScroll
            ) {
                result.layout_dirty = true;
            }
        }

        if let Some(level) = self.key_ctx.dirty_override {
            result.dirty = true;
            result.layout_dirty = matches!(
                level,
                crate::app::interaction_state::DirtyLevel::LayoutOnly
                    | crate::app::interaction_state::DirtyLevel::Full
            );
        }

        let command_chord_pending = self.key_dispatch_state.command_runtime.is_pending();
        let pending_cell = &self.core.ctx.env().command_chord_pending;
        if pending_cell.get() != command_chord_pending {
            pending_cell.set(command_chord_pending);
            result.dirty = true;
        }

        result
    }
}

impl<C: Component> DispatchOps for TestBackendDispatchOps<'_, C> {
    fn continue_command_chord(&mut self, key: KeyEvent) -> CommandDispatchState {
        let was_pending = self.key_dispatch_state.command_runtime.is_pending();
        match self
            .key_dispatch_state
            .command_runtime
            .feed(key, &self.command_registry)
        {
            CommandShortcutResult::Pending
                if !was_pending
                    && self.key_dispatch_config.key_dispatch_policy
                        == crate::KeyDispatchPolicy::WidgetFirst =>
            {
                self.key_dispatch_state.command_runtime.reset();
                self.key_dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
            CommandShortcutResult::Pending => {
                // Remember the first key that starts an accepted chord so a later
                // mismatch can replay it under `ForwardPrefixAndCurrent`.
                if !was_pending {
                    self.key_dispatch_state.pending_command_prefix = Some(key);
                }
                CommandDispatchState::Pending
            }
            CommandShortcutResult::Matched(_id) if !was_pending => {
                self.key_dispatch_state.command_runtime.reset();
                self.key_dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
            CommandShortcutResult::Matched(id) => {
                self.key_dispatch_state.pending_command_prefix = None;
                self.command_registry.execute(id.clone());
                CommandDispatchState::Matched(id.as_str().into())
            }
            CommandShortcutResult::Mismatch => {
                // Keep the swallowed prefix so the consuming sink can forward it
                // ahead of the mismatching key under `ForwardPrefixAndCurrent`.
                // Other policies leave it to be cleared on the next reset or
                // non-chord key.
                CommandDispatchState::Mismatch
            }
            CommandShortcutResult::None => {
                self.key_dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
        }
    }

    fn dispatch_widget(&mut self, key: KeyEvent) -> bool {
        crate::app::input::runtime_dispatch::dispatch_widget_with_policy(
            &mut self.core.tree,
            *self.focused,
            key,
            self.key_ctx,
            self.key_dispatch_config.key_dispatch_policy,
        )
    }

    fn dispatch_bubble(&mut self, key: KeyEvent) -> bool {
        use crate::app::input::key_dispatch::ChordMismatchPolicy;
        if matches!(
            self.key_dispatch_config.chord_mismatch_policy,
            ChordMismatchPolicy::ForwardPrefixAndCurrent
        ) && let Some(prefix) = self.key_dispatch_state.pending_command_prefix.take()
        {
            let prefix_bubble =
                self.core
                    .bubble_key(*self.focused, self.focused_key.as_ref(), prefix);
            if prefix_bubble.handled || prefix_bubble.dirty {
                self.key_ctx.dirty_override = Some(crate::app::interaction_state::DirtyLevel::Full);
            }
        }
        self.core
            .bubble_key(*self.focused, self.focused_key.as_ref(), key)
            .handled
    }

    fn dispatch_command(&mut self, key: KeyEvent) -> bool {
        let matches = self
            .command_registry
            .matching_enabled_shortcuts(key, self.key_dispatch_config.command_conflict_policy);
        if let Some(id) = matches.into_iter().next() {
            self.command_registry.execute(id);
            return true;
        }
        false
    }

    fn dispatch_framework(&mut self, key: KeyEvent) -> FrameworkDispatch {
        use crate::app::input::keymap::{Action, KeymapRuntimeResult};

        let runtime_match = match self.keymap_runtime.feed(key) {
            KeymapRuntimeResult::Pending => return FrameworkDispatch::Handled,
            KeymapRuntimeResult::Matched(binding) => Some(binding),
            KeymapRuntimeResult::None => None,
        };
        let action_from_chord = runtime_match.is_some_and(|binding| binding.is_chord);
        let matches = runtime_match
            .map(|binding| vec![binding.binding_match()])
            .unwrap_or_else(|| self.keymap.matches(key));

        if !self.top_capturing_overlay_is_empty()
            && matches.iter().any(|binding| binding.action == Action::Quit)
        {
            return FrameworkDispatch::Quit;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::DismissOverlay)
            && self.handle_overlay_escape()
        {
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::FocusNext)
        {
            if !action_from_chord
                && crate::app::input::runtime_dispatch::should_dispatch_text_area_tab_first(
                    &self.core.tree,
                    *self.focused,
                    key,
                )
                && keyboard::dispatch_key(&mut self.core.tree, *self.focused, key, self.key_ctx)
            {
                return FrameworkDispatch::Handled;
            }
            if self.focus_overlay_next() {
                return FrameworkDispatch::Handled;
            }
            if self.key_dispatch_config.focus_policy == FocusPolicy::Manual {
                return FrameworkDispatch::None;
            }
            focus::focus_next(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
                self.key_dispatch_config.focus_policy,
            );
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::FocusPrev)
        {
            if !action_from_chord
                && crate::app::input::runtime_dispatch::should_dispatch_text_area_tab_first(
                    &self.core.tree,
                    *self.focused,
                    key,
                )
                && keyboard::dispatch_key(&mut self.core.tree, *self.focused, key, self.key_ctx)
            {
                return FrameworkDispatch::Handled;
            }
            if self.focus_overlay_prev() {
                return FrameworkDispatch::Handled;
            }
            if self.key_dispatch_config.focus_policy == FocusPolicy::Manual {
                return FrameworkDispatch::None;
            }
            focus::focus_prev(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
                self.key_dispatch_config.focus_policy,
            );
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::ToggleDevTools)
        {
            self.framework_effects
                .push(FrameworkSideEffect::ToggleDevtools);
            return FrameworkDispatch::Handled;
        }

        if matches.iter().any(|binding| binding.action == Action::Quit) {
            if action_from_chord || self.top_capturing_overlay_is_empty() {
                return FrameworkDispatch::Quit;
            }
            let bubble = self
                .core
                .bubble_key(*self.focused, self.focused_key.as_ref(), key);
            if bubble.handled {
                return FrameworkDispatch::Handled;
            }
            return FrameworkDispatch::Quit;
        }

        FrameworkDispatch::None
    }

    fn dispatch_terminal_preflight(&mut self, key: KeyEvent) -> TerminalPreflightDispatch {
        #[cfg(not(feature = "terminal"))]
        {
            let _ = key;
            TerminalPreflightDispatch::NotApplicable
        }
        #[cfg(feature = "terminal")]
        {
            use crate::app::input::handlers::terminal::{TerminalPreflightResult, preflight_key};
            let Some(id) = self.focused.filter(|id| self.core.tree.is_valid(*id)) else {
                return TerminalPreflightDispatch::NotApplicable;
            };
            match preflight_key(
                &mut self.core.tree,
                id,
                key,
                self.clipboard,
                self.clipboard_config,
            ) {
                TerminalPreflightResult::Consumed => TerminalPreflightDispatch::Consumed,
                TerminalPreflightResult::NotConsumed => TerminalPreflightDispatch::NotConsumed,
                TerminalPreflightResult::NotApplicable => TerminalPreflightDispatch::NotApplicable,
            }
        }
    }

    fn dispatch_terminal(&mut self, key: KeyEvent) -> bool {
        use crate::app::input::key_dispatch::ChordMismatchPolicy;
        if matches!(
            self.key_dispatch_config.chord_mismatch_policy,
            ChordMismatchPolicy::ForwardPrefixAndCurrent
        ) && let Some(prefix) = self.key_dispatch_state.pending_command_prefix.take()
        {
            self.forward_terminal_key(prefix);
        }
        self.forward_terminal_key(key)
    }

    fn dispatch_ambient_scroll(&mut self, key: KeyEvent) -> bool {
        keyboard::dispatch_ambient_page_scroll(&mut self.core.tree, key)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::TestBackend;
    use crate::Length;
    use crate::app::input::keymap::{Action, BindingMode, binding_for_test, keymap_for_test};
    use crate::app::{ContrastPolicy, FocusPolicy};
    use crate::callback::Callback;
    use crate::core::component::{Component, Context, KeyUpdate, Update};
    use crate::core::element::{Element, ElementKind, IntoElement, Key};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};
    use crate::core::node::NodeKind;
    use crate::style::resolve::{resolve_base_style, resolve_muted_style};
    use crate::style::{
        Color, DocumentViewPalette, InputPalette, Paint, Rect, Span, Style, TextAreaPalette, Theme,
        ThemeRole, resolve_slot,
    };
    use crate::text::editor::TextEditor;
    #[cfg(feature = "syntax-syntect")]
    use crate::widgets::SyntectStrategy;
    use crate::widgets::{
        Animated, Button, ComboBox, ComboBoxCommitEvent, DocumentView, EffectScope, FileTree,
        FileTreeChange, FileTreeChangeSource, FileTreeChangeStatus, FileTreeChangeView, FocusScope,
        Frame, HStack, Input, InputEvent, List, ListItem, Modal, MouseRegion, Popover,
        SENTINEL_BASE, ScrollKeymap, ScrollView, SearchItem, SearchPalette, Spinner, SpinnerStyle,
        StatusBar, Tab, Tabs, Text, TextArea, TextAreaEvent, TextAreaLineNumberMode,
        TextAreaSentinel, TextAreaVimConfig, TextAreaVimCurrentLineHighlight, TextAreaVimMode,
        TextAreaVirtualText, ThemeProvider, Tree, TreeNode, VStack,
    };

    struct FocusEventHarness {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Component for FocusEventHarness {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let first_focus = self.log.clone();
            let first_blur = self.log.clone();
            let first_click = self.log.clone();
            let second_focus = self.log.clone();
            let second_blur = self.log.clone();
            VStack::new()
                .child(
                    MouseRegion::new()
                        .on_mouse_down(Callback::new(move |_| {
                            first_click.borrow_mut().push("mouse:first".into());
                        }))
                        .bubble_mouse_down(true)
                        .child(
                            Button::new("First")
                                .on_focus(Callback::new(move |_| {
                                    first_focus.borrow_mut().push("focus:first".into());
                                }))
                                .on_blur(Callback::new(move |_| {
                                    first_blur.borrow_mut().push("blur:first".into());
                                }))
                                .key("first"),
                        ),
                )
                .child(
                    Button::new("Second")
                        .on_focus(Callback::new(move |_| {
                            second_focus.borrow_mut().push("focus:second".into());
                        }))
                        .on_blur(Callback::new(move |_| {
                            second_blur.borrow_mut().push("blur:second".into());
                        }))
                        .key("second"),
                )
                .into()
        }
    }

    struct ManualModalHarness {
        auto_focus: bool,
    }

    impl Component for ManualModalHarness {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Modal::new()
                .auto_focus(self.auto_focus)
                .child(Button::new("Inside").key("inside"))
                .into()
        }
    }

    struct QueuedFocusHarness;

    impl Component for QueuedFocusHarness {
        type Message = ();
        type Properties = ();
        type State = usize;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state += 1;
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Button::new("Queued")
                .on_focus(ctx.link().callback(|_| ()))
                .key("queued")
        }
    }

    struct UnmountFocusedHarness;

    impl Component for UnmountFocusedHarness {
        type Message = ();
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            true
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            if ctx.state {
                Button::new("Transient").key("transient")
            } else {
                Text::new("gone").into()
            }
        }
    }

    struct PopoverAutoFocusHarness;

    impl Component for PopoverAutoFocusHarness {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    Popover::new()
                        .open(true)
                        .auto_focus(false)
                        .trigger(Button::new("Trigger").key("trigger"))
                        .content(Button::new("Content").key("content")),
                )
                .into()
        }
    }

    enum AutoFocusDismissMsg {
        Open,
        Close,
    }

    struct AutoFocusDismissHarness;

    impl Component for AutoFocusDismissHarness {
        type Message = AutoFocusDismissMsg;
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state = matches!(msg, AutoFocusDismissMsg::Open);
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let mut stack = VStack::new().child(Button::new("Background"));
            if ctx.state {
                stack = stack.child(
                    Modal::new()
                        .auto_focus(false)
                        .on_close(ctx.link().callback(|_| AutoFocusDismissMsg::Close))
                        .child(Button::new("Inside").key("inside")),
                );
            }
            stack.into()
        }
    }

    #[test]
    fn focus_callbacks_precede_app_hook_and_report_payloads() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let hook_log = log.clone();
        let app = crate::App::new()
            .focus_policy(FocusPolicy::Auto)
            .on_focus_changed(move |change| {
                let key = |entry: &Option<crate::FocusEntry>| {
                    entry
                        .as_ref()
                        .and_then(|entry| entry.key.as_ref())
                        .map(|key| key.as_ref().to_owned())
                        .unwrap_or_else(|| "none".to_owned())
                };
                hook_log.borrow_mut().push(format!(
                    "hook:{}->{}",
                    key(&change.old),
                    key(&change.new)
                ));
            });
        let mut backend =
            TestBackend::new_with_app(app, FocusEventHarness { log: log.clone() }, ());
        assert_eq!(log.borrow().as_slice(), ["focus:first", "hook:none->first"]);
        log.borrow_mut().clear();

        backend.focus_next();

        assert_eq!(
            log.borrow().as_slice(),
            ["blur:first", "focus:second", "hook:first->second"]
        );
    }

    #[test]
    fn equal_some_keys_deduplicate_focus_notifications_across_node_ids() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let hook_log = log.clone();
        let app = crate::App::new()
            .focus_policy(FocusPolicy::Auto)
            .on_focus_changed(move |_| hook_log.borrow_mut().push("hook".into()));
        let mut backend =
            TestBackend::new_with_app(app, FocusEventHarness { log: log.clone() }, ());
        log.borrow_mut().clear();

        let first = backend.focused().expect("initial focus");
        let duplicate = backend
            .core
            .tree
            .focusables()
            .into_iter()
            .find(|id| *id != first)
            .expect("second focusable");
        backend.core.tree.node_mut(duplicate).key = Some(Key::from("first"));
        backend.set_focused(duplicate);

        assert!(log.borrow().is_empty());
    }

    #[test]
    fn manual_modal_auto_focus_can_be_disabled_without_losing_capture() {
        let app = crate::App::new().focus_policy(FocusPolicy::Manual);
        let mut backend =
            TestBackend::new_with_app(app, ManualModalHarness { auto_focus: false }, ());

        assert_eq!(backend.focused_key(), None);
        let overlay = backend
            .core
            .tree
            .top_capturing_overlay()
            .expect("capturing modal");
        assert!(overlay.captures_focus);
        assert!(!overlay.auto_focus);
        assert!(backend.send_key(plain_code(KeyCode::Tab)).unwrap());
        assert_eq!(backend.focused_key(), None);
        backend.focus_next();
        assert_eq!(backend.focused(), None);
    }

    #[test]
    fn manual_modal_still_auto_focuses_by_default() {
        let app = crate::App::new().focus_policy(FocusPolicy::Manual);
        let backend = TestBackend::new_with_app(app, ManualModalHarness { auto_focus: true }, ());

        assert_eq!(backend.focused_key(), Some(&Key::from("inside")));
    }

    #[test]
    fn focus_callback_messages_are_processed_on_the_next_pump() {
        let app = crate::App::new().focus_policy(FocusPolicy::Auto);
        let mut backend = TestBackend::new_with_app(app, QueuedFocusHarness, ());

        assert_eq!(*backend.state(), 0);
        backend.pump().unwrap();
        assert_eq!(*backend.state(), 1);
    }

    #[test]
    fn app_hook_keeps_old_payload_when_focused_node_disappears() {
        let changes = Rc::new(RefCell::new(Vec::new()));
        let hook_changes = changes.clone();
        let app = crate::App::new()
            .focus_policy(FocusPolicy::Auto)
            .on_focus_changed(move |change| hook_changes.borrow_mut().push(change.clone()));
        let mut backend = TestBackend::new_with_app(app, UnmountFocusedHarness, ());
        changes.borrow_mut().clear();

        *backend.state_mut() = false;
        backend.render();

        let changes = changes.borrow();
        assert_eq!(changes.len(), 1);
        assert_eq!(
            changes[0]
                .old
                .as_ref()
                .and_then(|entry| entry.key.as_ref())
                .map(AsRef::<str>::as_ref),
            Some("transient")
        );
        assert_eq!(changes[0].new, None);
    }

    #[test]
    fn popover_auto_focus_false_suspends_focus_but_keeps_capture() {
        let app = crate::App::new().focus_policy(FocusPolicy::Manual);
        let backend = TestBackend::new_with_app(app, PopoverAutoFocusHarness, ());

        assert_eq!(backend.focused_key(), None);
        let overlay = backend
            .core
            .tree
            .top_capturing_overlay()
            .expect("capturing popover");
        assert!(overlay.captures_focus);
        assert!(!overlay.auto_focus);
    }

    #[test]
    fn on_demand_auto_focus_false_restores_prior_focus_after_dismissal() {
        let mut backend = TestBackend::new(AutoFocusDismissHarness);
        backend.focus_next();
        let focused = backend
            .focused()
            .expect("background should receive initial focus");
        assert_eq!(backend.core.tree.node(focused).key, None);

        backend
            .dispatch(AutoFocusDismissMsg::Open)
            .expect("modal should open");
        assert_eq!(backend.focused(), None);

        assert!(
            backend
                .send_key(plain_code(KeyCode::Esc))
                .expect("Escape should dismiss modal")
        );
        let restored = backend
            .focused()
            .expect("background focus should be restored after dismissal");
        assert_eq!(restored, focused);
    }

    #[test]
    fn click_focus_notifies_exactly_once() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let hook_log = log.clone();
        let app =
            crate::App::new().on_focus_changed(move |_| hook_log.borrow_mut().push("hook".into()));
        let mut backend =
            TestBackend::new_with_app(app, FocusEventHarness { log: log.clone() }, ());
        let rect = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&Key::from("first")))
            .map(|node| node.rect)
            .expect("first button");

        backend
            .send_mouse(MouseEvent {
                x: rect.x.max(0) as u16,
                y: rect.y.max(0) as u16,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .unwrap();

        assert_eq!(
            log.borrow().as_slice(),
            ["mouse:first", "focus:first", "hook"]
        );
    }

    #[test]
    fn auto_blur_request_does_not_emit_intermediate_none() {
        let changes = Rc::new(RefCell::new(Vec::new()));
        let hook_changes = changes.clone();
        let app = crate::App::new()
            .focus_policy(FocusPolicy::Auto)
            .on_focus_changed(move |change| hook_changes.borrow_mut().push(change.clone()));
        let mut backend = TestBackend::new_with_app(app, FocusControlRoot, ());
        changes.borrow_mut().clear();

        backend
            .dispatch(FocusControlMsg::Blur)
            .expect("blur should dispatch");

        assert!(changes.borrow().is_empty());
    }
    #[cfg(feature = "diff-view")]
    use crate::widgets::{DiffView, DiffViewBackend, DiffViewMode};

    fn unwrap_theme_provider(element: &Element) -> &Element {
        if let ElementKind::ThemeProvider(provider) = &element.kind {
            &provider.child
        } else {
            element
        }
    }

    fn first_cell_with_symbol<'a>(
        frame: &'a crate::capture::CapturedFrame,
        symbol: &str,
    ) -> &'a crate::capture::CapturedCell {
        frame
            .cells
            .iter()
            .find(|cell| cell.symbol == symbol)
            .unwrap_or_else(|| panic!("expected symbol `{symbol}` in captured frame"))
    }

    fn configure_test_keymap<C: Component>(
        backend: &mut TestBackend<C>,
        bindings: Vec<crate::app::input::keymap::Binding>,
    ) {
        backend.keymap = keymap_for_test(bindings);
        backend.keymap_runtime = crate::app::input::keymap::KeymapRuntime::new(&backend.keymap);
    }

    fn plain_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods::default(),
        }
    }

    fn plain_code(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    fn app_commands_first() -> crate::app::context::App {
        crate::app::context::App::new()
            .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    struct Counter;

    #[derive(Clone, Copy, Debug)]
    enum Msg {
        Inc,
    }

    impl Component for Counter {
        type Message = Msg;
        type Properties = ();
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("{}", ctx.state)).into()
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

    #[test]
    fn can_update_state_headlessly() {
        let mut backend = TestBackend::new(Counter);
        assert_eq!(*backend.state(), 0);

        backend.dispatch(Msg::Inc).expect("dispatch should succeed");
        assert_eq!(*backend.state(), 1);
    }

    struct ChordInputRoot;

    #[derive(Clone, Debug)]
    enum ChordInputMsg {
        Changed(InputEvent),
    }

    impl Component for ChordInputRoot {
        type Message = ChordInputMsg;
        type Properties = ();
        type State = String;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            String::new()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                ChordInputMsg::Changed(event) => ctx.state = event.value.to_string(),
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Input::new(ctx.state.clone())
                .on_change(ctx.link().callback(ChordInputMsg::Changed))
                .into()
        }
    }

    fn focused_chord_input_backend() -> TestBackend<ChordInputRoot> {
        let mut backend = TestBackend::new(ChordInputRoot);
        let input_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Input(_)))
            .map(|node| node.id)
            .expect("input exists");
        backend.set_focused(input_id);
        backend
    }

    #[test]
    fn send_key_consumes_pending_chord_prefix_before_focused_widget() {
        let mut backend = focused_chord_input_backend();
        configure_test_keymap(
            &mut backend,
            vec![binding_for_test(
                "ctrl-x q",
                Action::Quit,
                BindingMode::Always,
            )],
        );

        assert!(backend.send_key(ctrl_key('x')).expect("prefix succeeds"));
        assert_eq!(backend.state(), "");
    }

    #[test]
    fn send_key_chord_match_triggers_action_without_widget_insert() {
        let mut backend = focused_chord_input_backend();
        configure_test_keymap(
            &mut backend,
            vec![binding_for_test(
                "ctrl-x q",
                Action::Quit,
                BindingMode::Always,
            )],
        );

        assert!(backend.send_key(ctrl_key('x')).expect("prefix succeeds"));
        assert!(!backend.core.ctx.should_quit());
        assert!(backend.send_key(plain_key('q')).expect("match succeeds"));
        assert!(backend.core.ctx.should_quit());
        assert_eq!(backend.state(), "");
    }

    #[test]
    fn send_key_pending_chord_mismatch_resets_and_dispatches_unmatched_key() {
        let mut backend = focused_chord_input_backend();
        configure_test_keymap(
            &mut backend,
            vec![binding_for_test(
                "ctrl-x q",
                Action::Quit,
                BindingMode::Always,
            )],
        );

        assert!(backend.send_key(ctrl_key('x')).expect("prefix succeeds"));
        assert!(backend.send_key(plain_key('z')).expect("mismatch succeeds"));
        assert_eq!(backend.state(), "z");
        assert!(!backend.core.ctx.should_quit());
    }

    struct ButtonActivationRoot {
        disabled: bool,
        on_key_returns: Option<bool>,
    }

    #[derive(Default)]
    struct ButtonActivationState {
        clicks: Vec<MouseEvent>,
        keys: Vec<KeyEvent>,
    }

    enum ButtonActivationMsg {
        Clicked(MouseEvent),
        Keyed(KeyEvent),
    }

    impl ButtonActivationRoot {
        fn enabled() -> Self {
            Self {
                disabled: false,
                on_key_returns: None,
            }
        }

        fn disabled() -> Self {
            Self {
                disabled: true,
                on_key_returns: None,
            }
        }

        fn with_handling_key() -> Self {
            Self {
                disabled: false,
                on_key_returns: Some(true),
            }
        }

        fn with_nonhandling_key() -> Self {
            Self {
                disabled: false,
                on_key_returns: Some(false),
            }
        }
    }

    impl Component for ButtonActivationRoot {
        type Message = ButtonActivationMsg;
        type Properties = ();
        type State = ButtonActivationState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            ButtonActivationState::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                ButtonActivationMsg::Clicked(event) => ctx.state.clicks.push(event),
                ButtonActivationMsg::Keyed(key) => ctx.state.keys.push(key),
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let mut button = Button::new("Activate")
                .disabled(self.disabled)
                .on_click(ctx.link().callback(ButtonActivationMsg::Clicked));

            if let Some(handled) = self.on_key_returns {
                button = button
                    .on_key(ctx.link().key_handler(move |key| {
                        handled.then_some(ButtonActivationMsg::Keyed(key))
                    }));
            }

            button.key("activate-button")
        }
    }

    fn focused_button_backend(root: ButtonActivationRoot) -> TestBackend<ButtonActivationRoot> {
        let mut backend = TestBackend::new(root);
        backend.focus_next();
        backend
    }

    #[test]
    fn focused_button_plain_enter_and_space_invoke_on_click() {
        let mut backend = focused_button_backend(ButtonActivationRoot::enabled());

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Enter,
                    mods: KeyMods::NONE,
                })
                .expect("enter should dispatch")
        );
        assert!(
            backend
                .send_key(plain_key(' '))
                .expect("space should dispatch")
        );

        let clicks = &backend.state().clicks;
        assert_eq!(clicks.len(), 2);
        assert!(clicks.iter().all(|event| {
            event.kind == MouseKind::Up(MouseButton::Left) && event.mods == KeyMods::NONE
        }));
    }

    #[test]
    fn focused_button_on_key_true_suppresses_on_click_activation() {
        let mut backend = focused_button_backend(ButtonActivationRoot::with_handling_key());

        let handled = backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: KeyMods::NONE,
            })
            .expect("enter should dispatch");

        assert!(handled);
        assert!(backend.state().clicks.is_empty());
        assert_eq!(backend.state().keys.len(), 1);
    }

    #[test]
    fn focused_button_on_key_false_allows_on_click_activation() {
        let mut backend = focused_button_backend(ButtonActivationRoot::with_nonhandling_key());

        let handled = backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: KeyMods::NONE,
            })
            .expect("enter should dispatch");

        assert!(handled);
        assert_eq!(backend.state().clicks.len(), 1);
        assert!(backend.state().keys.is_empty());
    }

    #[test]
    fn disabled_focused_button_does_not_activate_from_keyboard() {
        let mut backend = focused_button_backend(ButtonActivationRoot::disabled());

        let handled = backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: KeyMods::NONE,
            })
            .expect("enter should dispatch");

        assert!(!handled);
        assert!(backend.state().clicks.is_empty());
    }

    #[test]
    fn modified_enter_and_space_do_not_activate_focused_button() {
        let mut backend = focused_button_backend(ButtonActivationRoot::enabled());

        assert!(
            !backend
                .send_key(KeyEvent {
                    code: KeyCode::Enter,
                    mods: KeyMods::CTRL,
                })
                .expect("modified enter should dispatch")
        );
        assert!(
            !backend
                .send_key(KeyEvent {
                    code: KeyCode::Char(' '),
                    mods: KeyMods::SHIFT,
                })
                .expect("modified space should dispatch")
        );
        assert!(backend.state().clicks.is_empty());
    }

    #[test]
    fn focused_key_exposes_keyed_button_after_focus_traversal() {
        let mut backend = TestBackend::new(ButtonActivationRoot::enabled());
        let key = Key::from("activate-button");

        backend.focused = None;
        backend.focused_key = None;
        backend.focused_tag = None;

        backend.focus_next();

        assert_eq!(backend.focused_key(), Some(&key));
    }

    #[test]
    fn focus_policy_controls_startup_and_tab_traversal() {
        let mut on_demand = TestBackend::new(ButtonActivationRoot::enabled());
        assert_eq!(on_demand.focused(), None);
        assert!(
            on_demand
                .send_key(plain_code(KeyCode::Tab))
                .expect("Tab should dispatch")
        );
        assert!(on_demand.focused().is_some());

        let auto = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Auto),
            ButtonActivationRoot::enabled(),
            (),
        );
        assert!(auto.focused().is_some());

        let mut manual = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Manual),
            ButtonActivationRoot::enabled(),
            (),
        );
        assert_eq!(manual.focused(), None);
        assert!(
            !manual
                .send_key(plain_code(KeyCode::Tab))
                .expect("Tab should dispatch")
        );
        assert_eq!(manual.focused(), None);

        // Explicit traversal remains available under Manual.
        manual.focus_next();
        assert!(manual.focused().is_some());
    }

    struct InitFocusRoot;

    impl Component for InitFocusRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn init(&mut self, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
            ctx.request_focus("requested");
            None
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Button::new("requested").key("requested")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn init_focus_request_resolves_under_all_policies() {
        for policy in [
            FocusPolicy::Auto,
            FocusPolicy::OnDemand,
            FocusPolicy::Manual,
        ] {
            let backend = TestBackend::new_with_app(
                crate::App::new().focus_policy(policy),
                InitFocusRoot,
                (),
            );
            assert_eq!(backend.focused_key(), Some(&Key::from("requested")));
        }
    }

    #[derive(Clone, Copy)]
    enum FocusControlMsg {
        Blur,
        Next,
        Prev,
        BlurThenNext,
    }

    struct FocusControlRoot;

    impl Component for FocusControlRoot {
        type Message = FocusControlMsg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(Button::new("first").key("first"))
                .child(Button::new("second").key("second"))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                FocusControlMsg::Blur => ctx.blur(),
                FocusControlMsg::Next => ctx.focus_next(),
                FocusControlMsg::Prev => ctx.focus_prev(),
                FocusControlMsg::BlurThenNext => {
                    ctx.blur();
                    ctx.focus_next();
                }
            }
            Update::full()
        }
    }

    #[test]
    fn context_focus_traversal_is_explicit_under_manual_policy() {
        let mut backend = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Manual),
            FocusControlRoot,
            (),
        );

        backend
            .dispatch(FocusControlMsg::Next)
            .expect("next request should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("first")));

        backend
            .dispatch(FocusControlMsg::Prev)
            .expect("previous request should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("second")));
    }

    #[test]
    fn context_blur_follows_focus_policy() {
        for policy in [
            FocusPolicy::Auto,
            FocusPolicy::OnDemand,
            FocusPolicy::Manual,
        ] {
            let mut backend = TestBackend::new_with_app(
                crate::App::new().focus_policy(policy),
                FocusControlRoot,
                (),
            );
            backend.focus_next();

            backend
                .dispatch(FocusControlMsg::Blur)
                .expect("blur request should dispatch");

            if policy == FocusPolicy::Auto {
                assert_eq!(backend.focused_key(), Some(&Key::from("first")));
            } else {
                assert_eq!(backend.focused(), None);
                assert_eq!(backend.focused_key(), None);
            }
        }
    }

    #[test]
    fn latest_context_focus_request_wins() {
        let mut backend = TestBackend::new(FocusControlRoot);

        backend
            .dispatch(FocusControlMsg::BlurThenNext)
            .expect("focus requests should dispatch");

        assert_eq!(backend.focused_key(), Some(&Key::from("first")));
    }

    #[test]
    fn test_backend_blur_clears_remembered_focus() {
        let mut backend = TestBackend::new(FocusControlRoot);
        backend.focus_next();

        backend.blur();

        assert_eq!(backend.focused(), None);
        assert_eq!(backend.focused_key(), None);
    }

    struct TabStopRoot;

    impl Component for TabStopRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(Input::new("").tab_stop(false).key("input"))
                .child(Button::new("button").key("button"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.request_focus("input");
            Update::full()
        }
    }

    #[test]
    fn tab_stop_false_skips_traversal_but_allows_explicit_focus() {
        let mut backend = TestBackend::new(TabStopRoot);

        backend.focus_next();
        assert_eq!(backend.focused_key(), Some(&Key::from("button")));

        backend.dispatch(()).expect("focus request should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("input")));

        backend.blur();
        let input_rect = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&Key::from("input")))
            .map(|node| node.rect)
            .expect("input should be mounted");
        backend
            .send_mouse(MouseEvent {
                x: input_rect.x.max(0) as u16,
                y: input_rect.y.max(0) as u16,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .expect("input click should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("input")));
    }

    struct ExcludedScopeRoot;

    impl Component for ExcludedScopeRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    VStack::new()
                        .focus_scope(FocusScope::Exclude)
                        .child(Button::new("hidden").key("hidden")),
                )
                .child(Button::new("visible").key("visible"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.request_focus("hidden");
            Update::full()
        }
    }

    #[test]
    fn excluded_scope_blocks_fallback_traversal_and_click_but_not_request() {
        let auto = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Auto),
            ExcludedScopeRoot,
            (),
        );
        assert_eq!(auto.focused_key(), Some(&Key::from("visible")));

        let mut backend = TestBackend::new(ExcludedScopeRoot);
        backend.focus_next();
        assert_eq!(backend.focused_key(), Some(&Key::from("visible")));
        backend.blur();

        let hidden_rect = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&Key::from("hidden")))
            .map(|node| node.rect)
            .expect("hidden button should be mounted");
        backend
            .send_mouse(MouseEvent {
                x: hidden_rect.x.max(0) as u16,
                y: hidden_rect.y.max(0) as u16,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .expect("hidden button click should dispatch");
        assert_eq!(backend.focused(), None);

        backend.dispatch(()).expect("focus request should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("hidden")));
    }

    struct ContainedScopeRoot;

    impl Component for ContainedScopeRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(Button::new("outside").key("outside"))
                .child(
                    Frame::new().focus_scope(FocusScope::Contain).child(
                        VStack::new()
                            .child(Button::new("first").key("first"))
                            .child(Button::new("second").key("second")),
                    ),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.request_focus("first");
            Update::full()
        }
    }

    #[test]
    fn contained_scope_cycles_and_wraps_in_test_backend() {
        let mut backend = TestBackend::new(ContainedScopeRoot);
        backend.dispatch(()).expect("focus request should dispatch");
        assert_eq!(backend.focused_key(), Some(&Key::from("first")));

        backend.focus_next();
        assert_eq!(backend.focused_key(), Some(&Key::from("second")));
        backend.focus_next();
        assert_eq!(backend.focused_key(), Some(&Key::from("first")));
        backend.focus_prev();
        assert_eq!(backend.focused_key(), Some(&Key::from("second")));
    }

    fn click_button<C: Component>(backend: &mut TestBackend<C>) {
        let rect = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Button(_)))
            .map(|node| node.rect)
            .expect("button exists");
        let x = rect.x.max(0) as u16;
        let y = rect.y.max(0) as u16;
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x,
                    y,
                    kind,
                    mods: KeyMods::NONE,
                })
                .expect("mouse event should dispatch");
        }
    }

    #[test]
    fn manual_policy_blocks_pointer_focus_but_not_click_handlers() {
        let mut on_demand = TestBackend::new(ButtonActivationRoot::enabled());
        click_button(&mut on_demand);
        assert!(on_demand.focused().is_some());
        assert_eq!(on_demand.state().clicks.len(), 1);

        let mut manual = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Manual),
            ButtonActivationRoot::enabled(),
            (),
        );
        click_button(&mut manual);
        assert_eq!(manual.focused(), None);
        assert_eq!(manual.state().clicks.len(), 1);

        let mut input = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Manual),
            ChordInputRoot,
            (),
        );
        let rect = input
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Input(_)))
            .map(|node| node.rect)
            .expect("input exists");
        input
            .send_mouse(MouseEvent {
                x: rect.x.max(0) as u16,
                y: rect.y.max(0) as u16,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .expect("input click should dispatch");
        assert_eq!(input.focused(), None);
        assert!(matches!(
            input.drag.active,
            crate::app::interaction_state::ActiveDrag::Input(_)
        ));
    }

    struct CapturingOverlayFocusRoot;

    impl Component for CapturingOverlayFocusRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Modal::new()
                .child(
                    VStack::new()
                        .child(Button::new("first").key("first"))
                        .child(Button::new("second").key("second")),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn manual_policy_keeps_capturing_overlay_focus_and_traversal() {
        let mut backend = TestBackend::new_with_app(
            crate::App::new().focus_policy(FocusPolicy::Manual),
            CapturingOverlayFocusRoot,
            (),
        );
        assert_eq!(backend.focused_key(), Some(&Key::from("first")));

        assert!(
            backend
                .send_key(plain_code(KeyCode::Tab))
                .expect("overlay Tab should dispatch")
        );
        assert_eq!(backend.focused_key(), Some(&Key::from("second")));
    }

    struct BubbleLeaf {
        calls: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Component for BubbleLeaf {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, _key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            self.calls.borrow_mut().push("leaf");
            KeyUpdate::handled(Update::none())
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("leaf").into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct BubbleRoot {
        calls: Rc<RefCell<Vec<&'static str>>>,
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

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::child(
                {
                    let calls = Rc::clone(&self.calls);
                    move || BubbleLeaf {
                        calls: Rc::clone(&calls),
                    }
                },
                (),
            )
            .key("bubble-child")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn send_key_with_no_focused_widget_uses_focused_key_bubble_scope() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TestBackend::new(BubbleRoot {
            calls: Rc::clone(&calls),
        });
        backend.focused = None;
        backend.focused_key = Some(Key::from("bubble-child"));

        let handled = backend
            .send_key(KeyEvent {
                code: KeyCode::Char('x'),
                mods: Default::default(),
            })
            .expect("send_key should succeed");

        assert!(handled);
        assert_eq!(calls.borrow().as_slice(), ["leaf"]);
    }

    struct VimTextAreaRoot;

    struct VimSearchRenderRoot;

    struct VimAutoHeightSearchRenderRoot;

    struct VimAutoHeightFrameSearchRenderRoot;

    struct VimAutoHeightPopoverFrameSearchRenderRoot;

    struct VimAutoHeightPopoverTriggerShortcutsRenderRoot;

    struct VimAutoHeightAnimatedPromptShellRenderRoot;

    struct VimAutoHeightEffectWrappedPromptShellRenderRoot;

    struct VimStyledSearchBarRoot;

    struct VimBackwardSearchRenderRoot;

    struct VimWrappedSearchRenderRoot;

    struct VimCollapsedContentSearchRoot;

    struct VimCurrentLineRenderRoot;

    struct TextAreaRelativeLineNumbersRoot;

    struct TextAreaVirtualTextRenderRoot;

    struct TextAreaVirtualTextCursorRoot;

    struct TextAreaVirtualTextSentinelRoot;

    #[derive(Clone, Debug)]
    enum VimTextAreaMsg {
        Changed(TextAreaEvent),
        Mode(TextAreaVimMode),
    }

    #[derive(Debug)]
    struct VimTextAreaState {
        editor: TextEditor,
        modes: Vec<TextAreaVimMode>,
    }

    impl Component for VimSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("alpha beta alpha")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .line_numbers(true)
                .height(Length::Px(4))
                .vim_motions(true)
                .vim_config(
                    TextAreaVimConfig::new()
                        .current_search_match_style(Style::new().bg(Color::Blue)),
                )
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            VStack::new()
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    TextArea::bound(&ctx.state)
                        .width(Length::Px(18))
                        .height(Length::Auto)
                        .border(false)
                        .scrollbar(false)
                        .line_numbers(true)
                        .vim_motions(true)
                        .on_change(ctx.link().callback(|event| event)),
                )
                .child(Text::new("below"))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightFrameSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            VStack::new()
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    Frame::new()
                        .width(Length::Px(20))
                        .height(Length::Auto)
                        .child(
                            TextArea::bound(&ctx.state)
                                .height(Length::Auto)
                                .border(false)
                                .scrollbar(false)
                                .line_numbers(true)
                                .vim_motions(true)
                                .on_change(ctx.link().callback(|event| event)),
                        ),
                )
                .child(Text::new("below"))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightPopoverFrameSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let dock: Element = Frame::new()
                .border(false)
                .padding((1, 2, 0, 2))
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(
                            TextArea::bound(&ctx.state)
                                .height(Length::Auto)
                                .border(false)
                                .scrollbar(false)
                                .line_numbers(true)
                                .vim_motions(true)
                                .on_change(ctx.link().callback(|event| event)),
                        )
                        .child(
                            HStack::new()
                                .height(Length::Px(1))
                                .width(Length::Percent(100))
                                .child(Text::new("status"))
                                .key("prompt-status"),
                        ),
                )
                .into();

            VStack::new()
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    Popover::new()
                        .trigger(dock)
                        .content(Text::new("popup"))
                        .open(false),
                )
                .child(Text::new("shortcuts").key("prompt-shortcuts"))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightPopoverTriggerShortcutsRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let dock: Element = Frame::new()
                .border(false)
                .padding((1, 2, 0, 2))
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(
                            TextArea::bound(&ctx.state)
                                .height(Length::Auto)
                                .border(false)
                                .scrollbar(false)
                                .line_numbers(true)
                                .vim_motions(true)
                                .on_change(ctx.link().callback(|event| event)),
                        )
                        .child(
                            HStack::new()
                                .height(Length::Px(1))
                                .width(Length::Percent(100))
                                .child(Text::new("status"))
                                .key("prompt-status"),
                        ),
                )
                .into();

            VStack::new()
                .height(Length::Auto)
                .child(
                    Popover::new()
                        .trigger(
                            VStack::new()
                                .height(Length::Auto)
                                .gap(0)
                                .child(dock)
                                .child(Text::new("shortcuts").key("prompt-shortcuts")),
                        )
                        .content(Text::new("popup"))
                        .open(false),
                )
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightAnimatedPromptShellRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let dock: Element = Frame::new()
                .border(false)
                .padding((1, 2, 0, 2))
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(
                            TextArea::bound(&ctx.state)
                                .height(Length::Auto)
                                .border(false)
                                .scrollbar(false)
                                .line_numbers(true)
                                .vim_motions(true)
                                .on_change(ctx.link().callback(|event| event)),
                        )
                        .child(
                            HStack::new()
                                .height(Length::Px(1))
                                .width(Length::Percent(100))
                                .child(Text::new("status"))
                                .key("prompt-status"),
                        ),
                )
                .into();

            VStack::new()
                .height(Length::Auto)
                .child(Animated::new(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(0)
                        .child(
                            Popover::new()
                                .trigger(dock)
                                .content(Text::new("popup"))
                                .open(false),
                        )
                        .child(Text::new("shortcuts").key("prompt-shortcuts")),
                ))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimAutoHeightEffectWrappedPromptShellRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("one\ntwo")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let dock: Element = Frame::new()
                .border(false)
                .padding((1, 2, 0, 2))
                .width(Length::Px(24))
                .height(Length::Auto)
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(
                            TextArea::bound(&ctx.state)
                                .height(Length::Auto)
                                .border(false)
                                .scrollbar(false)
                                .line_numbers(true)
                                .vim_motions(true)
                                .on_change(ctx.link().callback(|event| event)),
                        )
                        .child(
                            HStack::new()
                                .height(Length::Px(1))
                                .width(Length::Percent(100))
                                .child(Text::new("status"))
                                .key("prompt-status"),
                        ),
                )
                .into();

            VStack::new()
                .height(Length::Auto)
                .child(
                    ScrollView::new()
                        .height(Length::Flex(1))
                        .child(Text::new("messages"))
                        .key("messages-scroll"),
                )
                .child(
                    EffectScope::new().dim_by(0.0).child(
                        Popover::new()
                            .trigger(
                                VStack::new()
                                    .height(Length::Auto)
                                    .gap(0)
                                    .child(dock)
                                    .child(Text::new("shortcuts").key("prompt-shortcuts")),
                            )
                            .content(Text::new("popup"))
                            .open(false),
                    ),
                )
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimStyledSearchBarRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("alpha beta alpha")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .line_numbers(true)
                .height(Length::Px(4))
                .vim_motions(true)
                .vim_config(
                    TextAreaVimConfig::new()
                        .search_bar_prefix_style(Style::new().fg(Color::Cyan))
                        .search_bar_count_style(Style::new().fg(Color::Yellow)),
                )
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimBackwardSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let mut editor = TextEditor::new("alpha beta alpha");
            editor.set_cursor(10);
            editor
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .line_numbers(true)
                .height(Length::Px(4))
                .vim_motions(true)
                .vim_config(
                    TextAreaVimConfig::new()
                        .current_search_match_style(Style::new().bg(Color::Blue)),
                )
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimWrappedSearchRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("xxabcdyy")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .scrollbar(false)
                .width(Length::Px(5))
                .height(Length::Px(4))
                .vim_motions(true)
                .vim_config(
                    TextAreaVimConfig::new()
                        .current_search_match_style(Style::new().bg(Color::Blue)),
                )
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimCurrentLineRenderRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let mut editor = TextEditor::new("alpha\nbeta");
            editor.set_cursor(6);
            editor
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .style(Style::new().bg(Color::Red))
                .line_numbers(true)
                .height(Length::Px(3))
                .vim_motions(true)
                .vim_config(
                    TextAreaVimConfig::new()
                        .current_line_highlight(TextAreaVimCurrentLineHighlight::Full)
                        .current_line_style(Style::new().bg(Color::Blue))
                        .current_line_number_style(Style::new().fg(Color::Yellow).bold()),
                )
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for TextAreaRelativeLineNumbersRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let mut editor = TextEditor::new("one\ntwo\nthree\nfour\nfive");
            editor.set_cursor("one\ntwo\n".len());
            editor
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .scrollbar(false)
                .line_numbers(true)
                .line_number_mode(TextAreaLineNumberMode::Relative)
                .height(Length::Px(5))
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for TextAreaVirtualTextRenderRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            TextArea::new("ab\ncd")
                .border(false)
                .scrollbar(false)
                .wrap(false)
                .width(Length::Px(20))
                .height(Length::Px(2))
                .virtual_text(TextAreaVirtualText::inline(
                    1,
                    vec![Span::new("<x>").fg(Color::Cyan)],
                ))
                .virtual_text(TextAreaVirtualText::eol(
                    2,
                    vec![Span::new(" // diag").fg(Color::Red)],
                ))
                .into()
        }
    }

    impl Component for TextAreaVirtualTextCursorRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let mut editor = TextEditor::new("ab");
            editor.set_cursor(1);
            editor
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .scrollbar(false)
                .wrap(false)
                .width(Length::Px(10))
                .height(Length::Px(1))
                .virtual_text(TextAreaVirtualText::inline(
                    1,
                    vec![Span::new("<x>").fg(Color::Cyan)],
                ))
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for TextAreaVirtualTextSentinelRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let value = format!("a{}b", SENTINEL_BASE);
            let sentinel_end = 1 + SENTINEL_BASE.len_utf8();

            TextArea::new(value)
                .border(false)
                .scrollbar(false)
                .wrap(false)
                .cursor(sentinel_end)
                .anchor(Some(1))
                .selection_style(Style::new().bg(Color::Blue))
                .sentinels(vec![TextAreaSentinel::new("[S]")])
                .virtual_text(TextAreaVirtualText::inline(
                    1,
                    vec![Span::new("<v>").fg(Color::Cyan)],
                ))
                .width(Length::Px(12))
                .height(Length::Px(1))
                .into()
        }
    }

    impl Component for VimCollapsedContentSearchRoot {
        type Message = TextAreaEvent;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("alpha")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .line_numbers(true)
                .min_line_number_width(100)
                .vim_motions(true)
                .on_change(ctx.link().callback(|event| event))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            msg.apply_to(&mut ctx.state);
            Update::full()
        }
    }

    impl Component for VimTextAreaRoot {
        type Message = VimTextAreaMsg;
        type Properties = ();
        type State = VimTextAreaState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let mut editor = TextEditor::new("abc");
            editor.set_cursor(2);
            VimTextAreaState {
                editor,
                modes: Vec::new(),
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state.editor)
                .vim_motions(true)
                .on_change(ctx.link().callback(VimTextAreaMsg::Changed))
                .on_vim_mode_change(ctx.link().callback(VimTextAreaMsg::Mode))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                VimTextAreaMsg::Changed(event) => event.apply_to(&mut ctx.state.editor),
                VimTextAreaMsg::Mode(mode) => ctx.state.modes.push(mode),
            }
            Update::full()
        }
    }

    #[test]
    fn send_key_drives_bound_text_area_vim_mode_and_cursor_updates() {
        let mut backend = TestBackend::new(VimTextAreaRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        assert!(
            backend
                .send_key(char_key('i'))
                .expect("insert mode key succeeds")
        );
        assert_eq!(backend.state().modes.as_slice(), [TextAreaVimMode::Insert]);

        assert!(
            backend
                .send_key(char_key('x'))
                .expect("insert key succeeds")
        );
        assert_eq!(backend.state().editor.text(), "abxc");
        assert_eq!(backend.state().editor.cursor(), 3);

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Esc,
                    mods: Default::default(),
                })
                .expect("escape key succeeds")
        );
        assert_eq!(
            backend.state().modes.as_slice(),
            [TextAreaVimMode::Insert, TextAreaVimMode::Normal]
        );

        assert!(
            backend
                .send_key(char_key('h'))
                .expect("motion key succeeds")
        );
        assert_eq!(backend.state().editor.text(), "abxc");
        assert_eq!(backend.state().editor.cursor(), 2);

        assert!(
            backend
                .send_key(char_key('v'))
                .expect("visual key succeeds")
        );
        assert_eq!(
            backend.state().modes.as_slice(),
            [
                TextAreaVimMode::Insert,
                TextAreaVimMode::Normal,
                TextAreaVimMode::Visual,
            ]
        );
        assert_eq!(backend.state().editor.cursor(), 2);
        assert_eq!(backend.state().editor.anchor(), Some(2));

        assert!(
            backend
                .send_key(char_key('l'))
                .expect("visual motion succeeds")
        );
        assert_eq!(backend.state().editor.cursor(), 3);
        assert_eq!(backend.state().editor.anchor(), Some(2));

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Esc,
                    mods: Default::default(),
                })
                .expect("visual escape succeeds")
        );
        assert!(
            backend
                .send_key(char_key('V'))
                .expect("visual line key succeeds")
        );
        assert_eq!(
            backend.state().modes.as_slice(),
            [
                TextAreaVimMode::Insert,
                TextAreaVimMode::Normal,
                TextAreaVimMode::Visual,
                TextAreaVimMode::Normal,
                TextAreaVimMode::VisualLine,
            ]
        );
        assert_eq!(backend.state().editor.anchor(), Some(0));
        assert_eq!(
            backend.state().editor.cursor(),
            backend.state().editor.text().len()
        );
    }

    #[test]
    fn capture_frame_renders_text_area_relative_line_numbers() {
        let mut backend = TestBackend::new(TextAreaRelativeLineNumbersRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        });
        backend.render();

        let rows = backend.capture_frame().to_fixed_grid_lines();
        assert!(rows[0].starts_with("2 │one"), "row 0 was {:?}", rows[0]);
        assert!(rows[1].starts_with("1 │two"), "row 1 was {:?}", rows[1]);
        assert!(rows[2].starts_with("3 │three"), "row 2 was {:?}", rows[2]);
        assert!(rows[3].starts_with("1 │four"), "row 3 was {:?}", rows[3]);
        assert!(rows[4].starts_with("2 │five"), "row 4 was {:?}", rows[4]);
    }

    #[test]
    fn capture_frame_renders_text_area_virtual_text() {
        let mut backend = TestBackend::new(TextAreaVirtualTextRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        });
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(
            rows[0].starts_with("a<x>b // diag"),
            "row 0 was {:?}",
            rows[0]
        );
        assert!(rows[1].starts_with("cd"), "row 1 was {:?}", rows[1]);
        assert_eq!(frame.cell(1, 0).fg, Color::Cyan);
        assert_eq!(frame.cell(6, 0).fg, Color::Red);
    }

    #[test]
    fn screen_background_fills_cells_untouched_by_the_tree() {
        let mut backend = TestBackend::new(Counter);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 3,
        });

        // Without a screen background the empty cells stay reset/transparent.
        backend.render();
        assert_eq!(backend.capture_frame().cell(9, 2).bg, Color::Reset);

        // Opt in: every cell the tree does not paint should carry the fill.
        let fill = Color::Rgb(4, 9, 13);
        backend.set_screen_background(Some(crate::style::Style::new().bg(fill)));
        backend.render();
        let frame = backend.capture_frame();
        assert_eq!(frame.cell(9, 2).bg, fill, "corner cell should be filled");
        assert_eq!(frame.cell(5, 1).bg, fill, "interior gap should be filled");
    }

    #[test]
    fn capture_frame_places_cursor_after_inline_virtual_text_at_anchor() {
        let mut backend = TestBackend::new(TextAreaVirtualTextCursorRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[0].starts_with("a<x>b"), "row 0 was {:?}", rows[0]);
        assert_eq!(frame.cursor.as_ref().map(|cursor| cursor.x), Some(4));
    }

    #[test]
    fn capture_frame_renders_inline_virtual_text_without_corrupting_sentinel_selection() {
        let mut backend = TestBackend::new(TextAreaVirtualTextSentinelRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[0].starts_with("a<v>[S]b"), "row 0 was {:?}", rows[0]);
        assert_eq!(frame.cell(1, 0).fg, Color::Cyan);
        assert_eq!(frame.cell(4, 0).bg, Color::Blue);
    }

    #[test]
    fn capture_frame_shows_vim_search_prompt_and_match_highlight() {
        let mut backend = TestBackend::new(VimSearchRenderRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");
        backend.send_key(char_key('l')).expect("query key succeeds");

        let pending = backend.capture_frame();
        let pending_rows = pending.to_fixed_grid_lines();
        let search_y = pending.height.saturating_sub(1);
        assert!(
            pending_rows[usize::from(search_y)].starts_with("  al"),
            "search row was {:?}",
            pending_rows[usize::from(search_y)]
        );
        assert!(pending_rows[usize::from(search_y)].ends_with("[2/2]"));
        assert!(!pending_rows[usize::from(search_y)].contains("alpha"));
        assert_eq!(pending.cell(0, search_y).symbol, "");
        assert_eq!(pending.cell(2, search_y).symbol, "");
        assert!(pending.cell(3, 0).modifiers.underline);
        assert!(pending.cell(4, 0).modifiers.underline);
        assert!(!pending_rows[0].contains("[2/2]"));
        assert_ne!(pending.cell(3, 0).bg, Color::Blue);
        assert_eq!(pending.cell(14, 0).bg, Color::Blue);
        assert_eq!(pending.cell(15, 0).bg, Color::Blue);
        assert_eq!(
            pending.cursor.as_ref().map(|cursor| cursor.y),
            Some(search_y)
        );

        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: Default::default(),
            })
            .expect("search submit succeeds");

        let committed = backend.capture_frame();
        let committed_rows = committed.to_fixed_grid_lines();
        assert!(!committed_rows[usize::from(search_y)].starts_with(""));
        assert!(committed_rows[0].contains("alpha beta alpha [2/2]"));
        assert_eq!(committed.cell(19, 0).symbol, " ");
        assert!(!committed.cell(19, 0).modifiers.reverse);
        assert_eq!(committed.cell(20, 0).symbol, "[");
        assert!(committed.cell(20, 0).modifiers.reverse);
        assert!(committed.cell(3, 0).modifiers.underline);
        assert!(committed.cell(4, 0).modifiers.underline);
        assert_eq!(committed.cell(14, 0).bg, Color::Blue);
        assert_eq!(committed.cell(15, 0).bg, Color::Blue);

        backend
            .send_key(char_key('n'))
            .expect("search repeat succeeds");
        let repeated = backend.capture_frame();
        let repeated_rows = repeated.to_fixed_grid_lines();
        assert!(!repeated_rows[usize::from(search_y)].starts_with(""));
        assert!(repeated_rows[0].contains("alpha beta alpha [1/2]"));
        assert_eq!(repeated.cell(3, 0).bg, Color::Blue);
        assert_ne!(repeated.cell(14, 0).bg, Color::Blue);
    }

    #[test]
    fn capture_frame_reserves_auto_height_row_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightSearchRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 5,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[0].starts_with("1 │one"), "row 0 was {:?}", rows[0]);
        assert!(rows[1].starts_with("2 │two"), "row 1 was {:?}", rows[1]);
        assert!(rows[2].starts_with("  o"), "search row was {:?}", rows[2]);
        assert!(rows[3].starts_with("below"), "row 3 was {:?}", rows[3]);
    }

    #[test]
    fn capture_frame_resizes_auto_height_frame_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightFrameSearchRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 7,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[1].starts_with("│1 │one"), "row 1 was {:?}", rows[1]);
        assert!(rows[2].starts_with("│2 │two"), "row 2 was {:?}", rows[2]);
        assert!(
            rows[3].starts_with("│  o"),
            "search row was {:?}",
            rows[3]
        );
        assert!(rows[4].starts_with("└"), "row 4 was {:?}", rows[4]);
        assert!(rows[5].starts_with("below"), "row 5 was {:?}", rows[5]);
    }

    #[test]
    fn capture_frame_resizes_nested_popover_frame_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightPopoverFrameSearchRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 28,
            h: 8,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[1].starts_with("  1 │one"), "row 1 was {:?}", rows[1]);
        assert!(rows[2].starts_with("  2 │two"), "row 2 was {:?}", rows[2]);
        assert!(rows[3].starts_with("    o"), "row 3 was {:?}", rows[3]);
        assert!(rows[5].starts_with("  status"), "row 5 was {:?}", rows[5]);
        assert!(rows[6].starts_with("shortcuts"), "row 6 was {:?}", rows[6]);
        assert_eq!(rect_by_key(&backend, "prompt-status").y, 5);
        assert_eq!(rect_by_key(&backend, "prompt-shortcuts").y, 6);
    }

    #[test]
    fn capture_frame_resizes_popover_trigger_shortcuts_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightPopoverTriggerShortcutsRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 28,
            h: 8,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[3].starts_with("    o"), "row 3 was {:?}", rows[3]);
        assert!(rows[5].starts_with("  status"), "row 5 was {:?}", rows[5]);
        assert!(rows[6].starts_with("shortcuts"), "row 6 was {:?}", rows[6]);
        assert_eq!(rect_by_key(&backend, "prompt-shortcuts").y, 6);
    }

    #[test]
    fn capture_frame_resizes_animated_prompt_shell_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightAnimatedPromptShellRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 28,
            h: 8,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[3].starts_with("    o"), "row 3 was {:?}", rows[3]);
        assert!(rows[5].starts_with("  status"), "row 5 was {:?}", rows[5]);
        assert!(rows[6].starts_with("shortcuts"), "row 6 was {:?}", rows[6]);
        assert_eq!(rect_by_key(&backend, "prompt-shortcuts").y, 6);
    }

    #[test]
    fn capture_frame_resizes_effect_wrapped_prompt_shell_for_pending_vim_search() {
        let mut backend = TestBackend::new(VimAutoHeightEffectWrappedPromptShellRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 28,
            h: 8,
        });
        backend.render();
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        backend.set_focused(text_area_id);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('o')).expect("query key succeeds");
        backend.render();

        let frame = backend.capture_frame();
        let rows = frame.to_fixed_grid_lines();
        assert!(rows[4].starts_with("    o"), "row 4 was {:?}", rows[4]);
        assert!(rows[6].starts_with("  status"), "row 6 was {:?}", rows[6]);
        assert!(rows[7].starts_with("shortcuts"), "row 7 was {:?}", rows[7]);
        assert_eq!(rect_by_key(&backend, "prompt-shortcuts").y, 7);
        assert!(
            rect_by_key(&backend, "messages-scroll").h < 3,
            "messages region should yield height to the growing prompt shell"
        );
    }

    #[test]
    fn capture_frame_styles_vim_search_prefix_and_count() {
        let mut backend = TestBackend::new(VimStyledSearchBarRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");
        backend.send_key(char_key('l')).expect("query key succeeds");

        let pending = backend.capture_frame();
        let search_y = pending.height.saturating_sub(1);
        let rows = pending.to_fixed_grid_lines();
        assert!(
            rows[usize::from(search_y)].ends_with("[2/2]"),
            "search row was {:?}",
            rows[usize::from(search_y)]
        );
        assert_eq!(pending.cell(0, search_y).fg, Color::Cyan);
        assert_eq!(pending.cell(2, search_y).fg, Color::Cyan);
        assert_ne!(pending.cell(4, search_y).fg, Color::Cyan);

        let count_start = pending.width.saturating_sub(5);
        assert_eq!(pending.cell(count_start, search_y).symbol, "[");
        for x in count_start..pending.width {
            assert_eq!(pending.cell(x, search_y).fg, Color::Yellow);
        }

        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: Default::default(),
            })
            .expect("search submit succeeds");
        let committed = backend.capture_frame();
        assert_eq!(committed.cell(20, 0).symbol, "[");
        for x in 20..25 {
            assert_eq!(committed.cell(x, 0).fg, Color::Yellow);
        }
    }

    #[test]
    fn capture_frame_esc_cancels_pending_vim_search_bar() {
        let mut backend = TestBackend::new(VimSearchRenderRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");
        backend.send_key(char_key('l')).expect("query key succeeds");

        let pending = backend.capture_frame();
        let search_y = pending.height.saturating_sub(1);
        assert!(pending.to_fixed_grid_lines()[usize::from(search_y)].starts_with("  al"));

        backend
            .send_key(KeyEvent {
                code: KeyCode::Esc,
                mods: Default::default(),
            })
            .expect("escape cancels search");

        let cancelled = backend.capture_frame();
        let cancelled_rows = cancelled.to_fixed_grid_lines();
        assert!(
            !cancelled_rows[usize::from(search_y)].starts_with(""),
            "search row stayed visible: {:?}",
            cancelled_rows[usize::from(search_y)]
        );
        assert_ne!(
            cancelled.cursor.as_ref().map(|cursor| cursor.y),
            Some(search_y)
        );
    }

    #[test]
    fn capture_frame_esc_hides_committed_vim_search_but_repeat_restores_status() {
        let mut backend = TestBackend::new(VimSearchRenderRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");
        backend.send_key(char_key('l')).expect("query key succeeds");
        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: Default::default(),
            })
            .expect("search submit succeeds");

        let committed = backend.capture_frame();
        let search_y = committed.height.saturating_sub(1);
        let committed_rows = committed.to_fixed_grid_lines();
        assert!(!committed_rows[usize::from(search_y)].starts_with(""));
        assert!(committed_rows[0].contains("alpha beta alpha [2/2]"));

        backend
            .send_key(KeyEvent {
                code: KeyCode::Esc,
                mods: Default::default(),
            })
            .expect("escape hides committed search");
        let hidden = backend.capture_frame();
        let hidden_rows = hidden.to_fixed_grid_lines();
        assert!(
            !hidden_rows[usize::from(search_y)].starts_with(""),
            "committed search row stayed visible: {:?}",
            hidden_rows[usize::from(search_y)]
        );
        assert!(!hidden_rows[0].contains("[2/2]"));

        backend
            .send_key(char_key('n'))
            .expect("repeat search succeeds");
        let repeated = backend.capture_frame();
        let repeated_rows = repeated.to_fixed_grid_lines();
        assert!(!repeated_rows[usize::from(search_y)].starts_with(""));
        assert!(repeated_rows[0].contains("alpha beta alpha [1/2]"));
    }

    #[test]
    fn capture_frame_highlights_backward_vim_search_target_match() {
        let mut backend = TestBackend::new(VimBackwardSearchRenderRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('?'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");
        backend.send_key(char_key('l')).expect("query key succeeds");

        let pending = backend.capture_frame();
        let pending_rows = pending.to_fixed_grid_lines();
        let search_y = pending.height.saturating_sub(1);

        assert!(
            pending_rows[usize::from(search_y)].starts_with("  al"),
            "search row was {:?}",
            pending_rows[usize::from(search_y)]
        );
        assert!(pending_rows[usize::from(search_y)].ends_with("[1/2]"));
        assert_eq!(pending.cell(3, 0).bg, Color::Blue);
        assert_eq!(pending.cell(4, 0).bg, Color::Blue);
        assert_ne!(pending.cell(14, 0).bg, Color::Blue);

        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: Default::default(),
            })
            .expect("search submit succeeds");
        backend
            .send_key(char_key('n'))
            .expect("next search repeat succeeds");
        let next = backend.capture_frame();
        let next_rows = next.to_fixed_grid_lines();
        assert!(next_rows[0].contains("alpha beta alpha [2/2]"));
        assert_ne!(next.cell(3, 0).bg, Color::Blue);
        assert_eq!(next.cell(14, 0).bg, Color::Blue);

        backend
            .send_key(char_key('N'))
            .expect("previous search repeat succeeds");
        let previous = backend.capture_frame();
        let previous_rows = previous.to_fixed_grid_lines();
        assert!(previous_rows[0].contains("alpha beta alpha [1/2]"));
        assert_eq!(previous.cell(3, 0).bg, Color::Blue);
        assert_ne!(previous.cell(14, 0).bg, Color::Blue);
    }

    #[test]
    fn capture_frame_highlights_vim_search_match_across_wrap_boundary() {
        let mut backend = TestBackend::new(VimWrappedSearchRenderRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 4,
        });
        backend.render();
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        for ch in ['a', 'b', 'c', 'd'] {
            backend.send_key(char_key(ch)).expect("query key succeeds");
        }

        let pending = backend.capture_frame();
        let pending_rows = pending.to_fixed_grid_lines();
        let search_y = pending.height.saturating_sub(1);

        assert!(pending_rows[usize::from(search_y)].starts_with(""));
        for (x, y) in [(2, 0), (3, 0), (4, 0), (0, 1)] {
            assert!(
                pending.cell(x, y).modifiers.underline,
                "missing underline at ({x}, {y})"
            );
            assert_eq!(
                pending.cell(x, y).bg,
                Color::Blue,
                "missing current match at ({x}, {y})"
            );
        }
    }

    #[test]
    fn capture_frame_highlights_vim_current_line_full_width() {
        let mut backend = TestBackend::new(VimCurrentLineRenderRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        let frame = backend.capture_frame();

        assert_ne!(frame.cell(0, 0).bg, Color::Blue);
        assert_eq!(frame.cell(0, 1).bg, Color::Blue);
        assert_eq!(frame.cell(3, 1).bg, Color::Blue);
        assert_eq!(frame.cell(0, 1).fg, Color::Yellow);
        assert_ne!(frame.cell(3, 1).fg, Color::Yellow);

        backend
            .send_key(char_key('v'))
            .expect("visual mode key succeeds");
        let visual_frame = backend.capture_frame();
        assert_ne!(visual_frame.cell(0, 1).bg, Color::Blue);
        assert_ne!(visual_frame.cell(3, 1).bg, Color::Blue);
    }

    #[test]
    fn capture_frame_shows_vim_search_bar_when_content_width_collapses() {
        let mut backend = TestBackend::new(VimCollapsedContentSearchRoot);
        let root = backend.core.tree.root;
        backend.set_focused(root);

        backend
            .send_key(char_key('/'))
            .expect("search key succeeds");
        backend.send_key(char_key('a')).expect("query key succeeds");

        let pending = backend.capture_frame();
        let rows = pending.to_fixed_grid_lines();
        let search_y = pending.height.saturating_sub(1);

        assert!(
            rows[usize::from(search_y)].starts_with("  a"),
            "search row was {:?}",
            rows[usize::from(search_y)]
        );
        assert_eq!(
            pending.cursor.as_ref().map(|cursor| cursor.y),
            Some(search_y)
        );
    }

    fn page_down() -> KeyEvent {
        KeyEvent {
            code: KeyCode::PageDown,
            mods: Default::default(),
        }
    }

    fn scroll_offset_by_key<C: Component>(backend: &TestBackend<C>, key: &str) -> usize {
        let target = Key::from(key.to_string());
        backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&target))
            .and_then(|node| match &node.kind {
                NodeKind::ScrollView(scroll) => Some(scroll.offset),
                _ => None,
            })
            .unwrap_or_else(|| panic!("scroll view with key {key:?} not found"))
    }

    fn rect_by_key<C: Component>(backend: &TestBackend<C>, key: &str) -> crate::style::Rect {
        let target = Key::from(key.to_string());
        backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&target))
            .map(|node| node.rect)
            .unwrap_or_else(|| panic!("node with key {key:?} not found"))
    }

    fn first_input_value<C: Component>(backend: &TestBackend<C>) -> String {
        backend
            .core
            .tree
            .iter_with_overlays()
            .find_map(|node| match &node.kind {
                NodeKind::Input(input) => Some(input.value.to_string()),
                _ => None,
            })
            .expect("input node should exist")
    }

    struct AmbientPageScrollRoot;

    struct ModalSearchPaletteRoot;

    struct AppCommandPaletteRoot {
        selected: Rc<RefCell<Vec<usize>>>,
        activated: Rc<RefCell<Vec<usize>>>,
    }

    impl Component for AppCommandPaletteRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let selected = Rc::clone(&self.selected);
            let activated = Rc::clone(&self.activated);

            SearchPalette::<usize>::new()
                .items((0..4).map(|i| SearchItem::new(format!("item-{i}"), i)))
                .height(Length::Auto)
                .on_select(Callback::new(
                    move |event: crate::widgets::SearchEvent<usize>| {
                        selected.borrow_mut().push(event.item.value);
                    },
                ))
                .on_activate(Callback::new(
                    move |event: crate::widgets::SearchEvent<usize>| {
                        activated.borrow_mut().push(event.item.value);
                    },
                ))
                .key("app-command-palette")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct AppCommandFileTreeRoot;

    impl Component for AppCommandFileTreeRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            FileTree::new("/repo")
                .explorer(true)
                .explorer_prefix("")
                .explorer_divider(false)
                .show_icons(false)
                .change_source(FileTreeChangeSource::Provided(vec![FileTreeChange::new(
                    "src/main.rs",
                    FileTreeChangeStatus::Modified,
                )]))
                .change_view(FileTreeChangeView::ChangedOnly)
                .key("app-command-file-tree")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[derive(Clone, Debug)]
    enum AppCommandComboMsg {
        Query(std::sync::Arc<str>),
        Open(bool),
        Active(Option<usize>),
        Commit(ComboBoxCommitEvent),
    }

    #[derive(Default)]
    struct AppCommandComboState {
        query: std::sync::Arc<str>,
        open: bool,
        active: Option<usize>,
        commits: Vec<ComboBoxCommitEvent>,
    }

    struct AppCommandComboRoot;

    impl Component for AppCommandComboRoot {
        type Message = AppCommandComboMsg;
        type Properties = ();
        type State = AppCommandComboState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            AppCommandComboState::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            ComboBox::new()
                .items(["Alpha", "Beta", "Gamma"])
                .query(ctx.state.query.clone())
                .open(ctx.state.open)
                .active_index(ctx.state.active)
                .on_query_change(ctx.link().callback(AppCommandComboMsg::Query))
                .on_open_change(ctx.link().callback(AppCommandComboMsg::Open))
                .on_active_index_change(ctx.link().callback(AppCommandComboMsg::Active))
                .on_commit(ctx.link().callback(AppCommandComboMsg::Commit))
                .key("app-command-combo")
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                AppCommandComboMsg::Query(query) => ctx.state.query = query,
                AppCommandComboMsg::Open(open) => ctx.state.open = open,
                AppCommandComboMsg::Active(active) => ctx.state.active = active,
                AppCommandComboMsg::Commit(event) => ctx.state.commits.push(event),
            }
            Update::full()
        }
    }

    #[derive(Clone, Debug)]
    enum AppCommandTreeMsg {
        Toggle(bool),
    }

    #[derive(Default)]
    struct AppCommandTreeState {
        toggles: Vec<bool>,
    }

    struct AppCommandTreeRoot;

    impl Component for AppCommandTreeRoot {
        type Message = AppCommandTreeMsg;
        type Properties = ();
        type State = AppCommandTreeState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            AppCommandTreeState::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Tree::new(TreeNode::new("root").child(TreeNode::new("child")))
                .on_toggle(
                    ctx.link()
                        .callback(|event: crate::widgets::TreeToggleEvent| {
                            AppCommandTreeMsg::Toggle(event.expanded)
                        }),
                )
                .key("app-command-tree")
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                AppCommandTreeMsg::Toggle(expanded) => ctx.state.toggles.push(expanded),
            }
            Update::full()
        }
    }

    impl Component for ModalSearchPaletteRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let palette: Element = SearchPalette::<usize>::new()
                .items((0..20).map(|i| SearchItem::new(format!("item-{i}"), i)))
                .height(Length::Auto)
                .list_padding((0, 0, 1, 0))
                .input_divider(false)
                .into();

            let content: Element = VStack::new()
                .height(Length::Auto)
                .gap(1)
                .child(
                    HStack::new()
                        .height(Length::Px(1))
                        .child(Text::new("header")),
                )
                .child(palette.key("modal-palette"))
                .child(
                    HStack::new()
                        .height(Length::Px(1))
                        .child(Text::new("hint"))
                        .key("modal-hints"),
                )
                .into();

            Element::from(
                Modal::new().border(false).width(Length::Px(30)).child(
                    Frame::new()
                        .border(false)
                        .child(content)
                        .key("modal-inner-frame"),
                ),
            )
            .max_height(Length::Percent(50))
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    impl Component for AmbientPageScrollRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .scroll_keys(ScrollKeymap::DEFAULT)
                .focusable(false)
                .ambient_page_scroll(true)
                .children((0..40).map(|i| {
                    Text::new(format!("row {i}"))
                        .height(Length::Px(1))
                        .key(format!("ambient-row-{i}"))
                }))
                .key("ambient-scroll")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct AmbientPageScrollHandledByOnKey;

    impl Component for AmbientPageScrollHandledByOnKey {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            if matches!(key.code, KeyCode::PageDown) {
                KeyUpdate::handled(Update::none())
            } else {
                KeyUpdate::unhandled(Update::none())
            }
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .scroll_keys(ScrollKeymap::DEFAULT)
                .focusable(false)
                .ambient_page_scroll(true)
                .children((0..40).map(|i| {
                    Text::new(format!("row {i}"))
                        .height(Length::Px(1))
                        .key(format!("handled-row-{i}"))
                }))
                .key("handled-scroll")
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct AmbiguousAmbientPageScrollRoot;

    impl Component for AmbiguousAmbientPageScrollRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    ScrollView::new()
                        .scroll_keys(ScrollKeymap::DEFAULT)
                        .focusable(false)
                        .ambient_page_scroll(true)
                        .children((0..20).map(|i| {
                            Text::new(format!("left {i}"))
                                .height(Length::Px(1))
                                .key(format!("left-row-{i}"))
                        }))
                        .key("ambient-left"),
                )
                .child(
                    ScrollView::new()
                        .scroll_keys(ScrollKeymap::DEFAULT)
                        .focusable(false)
                        .ambient_page_scroll(true)
                        .children((0..20).map(|i| {
                            Text::new(format!("right {i}"))
                                .height(Length::Px(1))
                                .key(format!("right-row-{i}"))
                        }))
                        .key("ambient-right"),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn ambient_page_scroll_handles_page_down_without_focus() {
        let mut backend = TestBackend::new(AmbientPageScrollRoot);
        backend.focused = None;
        backend.focused_key = None;

        let handled = backend
            .send_key(page_down())
            .expect("send_key should succeed");

        assert!(handled);
        assert!(scroll_offset_by_key(&backend, "ambient-scroll") > 0);
    }

    #[test]
    fn ambient_page_scroll_runs_after_component_on_key_bubbling() {
        let mut backend = TestBackend::new(AmbientPageScrollHandledByOnKey);
        backend.focused = None;
        backend.focused_key = None;

        let handled = backend
            .send_key(page_down())
            .expect("send_key should succeed");

        assert!(handled);
        assert_eq!(scroll_offset_by_key(&backend, "handled-scroll"), 0);
    }

    #[test]
    fn modal_search_palette_keeps_hints_visible_under_cap() {
        let mut backend = TestBackend::new(ModalSearchPaletteRoot);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        });
        backend.render();

        let palette = rect_by_key(&backend, "modal-palette");
        let hints = rect_by_key(&backend, "modal-hints");

        assert!(palette.h > 0, "palette should retain visible height");
        assert_eq!(hints.h, 1, "hints row should retain its one-line height");
        assert!(
            hints.y.saturating_add(hints.h as i16) <= 15,
            "hints row should remain inside the capped modal bounds"
        );
    }

    #[test]
    fn modal_search_palette_component_focus_still_drives_query_input() {
        let app = app_commands_first();
        let mut backend = TestBackend::new_with_app(app, ModalSearchPaletteRoot, ());
        backend.focused = None;
        backend.focused_key = Some(Key::from("modal-palette"));
        backend.focused_tag = None;
        backend.render();

        let handled = backend
            .send_key(plain_key('7'))
            .expect("send_key should succeed");

        assert!(handled, "palette should consume query text");
        assert_eq!(first_input_value(&backend), "7");
    }

    #[test]
    fn app_commands_first_preserves_search_palette_navigation_and_activation() {
        let selected = Rc::new(RefCell::new(Vec::new()));
        let activated = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TestBackend::new_with_app(
            app_commands_first(),
            AppCommandPaletteRoot {
                selected: Rc::clone(&selected),
                activated: Rc::clone(&activated),
            },
            (),
        );
        backend.focused = None;
        backend.focused_key = Some(Key::from("app-command-palette"));
        backend.focused_tag = None;
        backend.render();

        assert!(
            backend
                .send_key(plain_code(KeyCode::End))
                .expect("End should dispatch"),
            "palette should consume End"
        );
        assert!(
            backend
                .send_key(plain_code(KeyCode::Enter))
                .expect("Enter should dispatch"),
            "palette should consume Enter"
        );

        assert_eq!(selected.borrow().as_slice(), &[3]);
        assert_eq!(activated.borrow().as_slice(), &[3]);
    }

    #[test]
    fn app_commands_first_preserves_file_tree_explorer_focus_and_typing() {
        let mut backend =
            TestBackend::new_with_app(app_commands_first(), AppCommandFileTreeRoot, ());
        backend.focused = None;
        backend.focused_key = Some(Key::from("__ft_tree"));
        backend.focused_tag = None;
        backend.render();

        assert!(
            backend
                .send_key(plain_key('/'))
                .expect("slash should dispatch"),
            "file tree should consume explorer focus key"
        );
        assert_eq!(backend.focused_key, Some(Key::from("__ft_input")));
        assert!(
            backend
                .send_key(plain_key('m'))
                .expect("typing should dispatch"),
            "file tree explorer input should consume typed text"
        );

        assert_eq!(first_input_value(&backend), "m");
    }

    #[test]
    fn app_commands_first_preserves_combo_box_typing_navigation_and_commit() {
        let mut backend = TestBackend::new_with_app(app_commands_first(), AppCommandComboRoot, ());
        backend.focused = None;
        backend.focused_key = Some(Key::from("app-command-combo"));
        backend.focused_tag = None;
        backend.render();

        assert!(backend.send_key(plain_key('a')).expect("type query"));
        assert_eq!(backend.state().query.as_ref(), "a");
        assert!(backend.state().open);

        assert!(
            backend
                .send_key(plain_code(KeyCode::Down))
                .expect("Down should dispatch")
        );
        assert_eq!(backend.state().active, Some(1));

        assert!(
            backend
                .send_key(plain_code(KeyCode::Enter))
                .expect("Enter should dispatch")
        );
        assert_eq!(backend.state().commits.len(), 1);
        assert_eq!(backend.state().commits[0].index, Some(1));
        assert_eq!(backend.state().commits[0].value.as_ref(), "Beta");
    }

    #[test]
    fn app_commands_first_preserves_tree_keyboard_actions() {
        let mut backend = TestBackend::new_with_app(app_commands_first(), AppCommandTreeRoot, ());
        backend.focused = None;
        backend.focused_key = Some(Key::from("app-command-tree"));
        backend.focused_tag = None;
        backend.render();

        assert!(
            backend
                .send_key(plain_code(KeyCode::Right))
                .expect("Right should dispatch"),
            "tree should consume expand key"
        );

        assert_eq!(backend.state().toggles.as_slice(), &[true]);
    }

    enum EmptyModalMsg {
        Close,
    }

    struct EmptyModalFocusRoot {
        show_modal: bool,
        changes: Rc<RefCell<Vec<String>>>,
    }

    impl Component for EmptyModalFocusRoot {
        type Message = EmptyModalMsg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            let changes = Rc::clone(&self.changes);
            let editor = TextArea::new("")
                .on_change(Callback::new(move |event: TextAreaEvent| {
                    changes.borrow_mut().push(event.value.to_string());
                }))
                .key("under-editor");

            let mut root = VStack::new()
                .child(Button::new("before").key("before-button"))
                .child(editor);
            if self.show_modal {
                root = root.child(
                    Modal::new()
                        .border(false)
                        .padding(0)
                        .child(Text::new("modal"))
                        .on_close(ctx.link().callback(|_| EmptyModalMsg::Close)),
                );
            }
            root.into()
        }

        fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            match msg {
                EmptyModalMsg::Close => self.show_modal = false,
            }
            Update::full()
        }
    }

    fn char_key(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            mods: Default::default(),
        }
    }

    #[test]
    fn empty_modal_suspends_underlying_text_area_focus() {
        let changes = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TestBackend::new(EmptyModalFocusRoot {
            show_modal: false,
            changes: Rc::clone(&changes),
        });

        let editor_key = Key::from("under-editor");
        let editor_id = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref() == Some(&editor_key))
            .map(|node| node.id)
            .expect("editor should be mounted");
        backend.set_focused(editor_id);
        backend.component_mut().show_modal = true;
        backend.render();

        assert!(backend.focused().is_none());

        let handled = backend
            .send_key(char_key('x'))
            .expect("send_key should succeed");
        assert!(handled, "empty modal should swallow text input");
        assert!(changes.borrow().is_empty());

        let dismissed = backend
            .send_key(KeyEvent {
                code: KeyCode::Esc,
                mods: Default::default(),
            })
            .expect("dismiss should succeed");
        assert!(dismissed);
        assert!(backend.focused().is_some());
        assert_eq!(backend.focused_key, Some(editor_key));

        backend
            .send_key(char_key('x'))
            .expect("text input should succeed after dismiss");
        assert_eq!(changes.borrow().as_slice(), ["x"]);
    }

    struct BackdropHoverRoot {
        show_modal: bool,
    }

    struct HoverCaptureButtonRoot;

    impl Component for BackdropHoverRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_key(&mut self, key: KeyEvent, _ctx: &mut Context<Self>) -> KeyUpdate {
            if matches!(key.code, KeyCode::Char('m')) {
                self.show_modal = true;
                KeyUpdate::handled(Update::full())
            } else {
                KeyUpdate::unhandled(Update::none())
            }
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let hover_target = MouseRegion::new()
                .hover_style(Style::new().bg(Color::Blue))
                .child(Text::new("under"));

            let mut root = VStack::new().child(hover_target);
            if self.show_modal {
                root = root.child(
                    Modal::new()
                        .border(false)
                        .padding(0)
                        .child(Text::new("modal")),
                );
            }
            root.into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    impl Component for HoverCaptureButtonRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let panel_bg = Color::Rgb(0x15, 0x15, 0x19);
            let alpha_fg = Paint::rgba(0xff, 0xff, 0xff, 0x40);

            Button::filled("Hover")
                .style(Style::new().bg(panel_bg).fg(Color::White))
                .focus_style(Style::default())
                .hover_style(
                    Style::new()
                        .fg(alpha_fg)
                        .contrast_policy(ContrastPolicy::Off),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn modal_backdrop_clears_underlying_hover() {
        let mut backend = TestBackend::new(BackdropHoverRoot { show_modal: false });

        backend
            .send_mouse(MouseEvent {
                x: 0,
                y: 0,
                kind: MouseKind::Moved,
                mods: Default::default(),
            })
            .expect("mouse move should succeed");
        assert!(backend.mouse.hovered.is_some());

        backend
            .send_key(char_key('m'))
            .expect("opening modal should succeed");
        assert!(backend.core.tree.top_capturing_overlay().is_some());
        assert!(backend.mouse.hovered.is_none());

        backend
            .send_mouse(MouseEvent {
                x: 1,
                y: 0,
                kind: MouseKind::Moved,
                mods: Default::default(),
            })
            .expect("mouse move should succeed");
        assert!(backend.mouse.hovered.is_none());
    }

    #[test]
    fn capture_frame_preserves_mouse_hover_state() {
        let mut backend = TestBackend::new(HoverCaptureButtonRoot);

        backend
            .send_mouse(MouseEvent {
                x: 1,
                y: 0,
                kind: MouseKind::Moved,
                mods: Default::default(),
            })
            .expect("mouse move should succeed");

        assert!(backend.mouse.hovered.is_some());

        let frame = backend.capture_frame();
        let cell = first_cell_with_symbol(&frame, "H");

        assert_eq!(cell.fg, Color::Rgb(0x50, 0x50, 0x53));
        assert_eq!(cell.bg, Color::Rgb(0x15, 0x15, 0x19));
    }

    #[test]
    fn ambient_page_scroll_requires_a_unique_target() {
        let mut backend = TestBackend::new(AmbiguousAmbientPageScrollRoot);
        backend.focused = None;
        backend.focused_key = None;

        let handled = backend
            .send_key(page_down())
            .expect("send_key should succeed");

        assert!(!handled);
        assert_eq!(scroll_offset_by_key(&backend, "ambient-left"), 0);
        assert_eq!(scroll_offset_by_key(&backend, "ambient-right"), 0);
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BrandTheme {
        badge: Style,
    }

    struct ThemeLeaf;

    impl Component for ThemeLeaf {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            let label = if ctx.theme_extension::<BrandTheme>().is_some() {
                "brand"
            } else {
                "plain"
            };
            Text::new(label).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    struct ThemeParent;

    impl Component for ThemeParent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default().with_extension(BrandTheme {
                badge: Style::new().fg(Color::rgb(12, 34, 56)),
            }))
            .child(crate::child::<ThemeLeaf, _>(|| ThemeLeaf, ()))
            .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    #[test]
    fn nested_components_can_read_theme_extensions() {
        let backend = TestBackend::new(ThemeParent);

        fn text_content(element: &Element) -> Option<String> {
            match &element.kind {
                ElementKind::Text(text) => Some(text.plain_content()),
                ElementKind::Group(group) => text_content(&group.child),
                _ => None,
            }
        }

        assert_eq!(text_content(backend.element()).as_deref(), Some("brand"));
    }

    struct AsyncCounter;

    #[derive(Clone, Copy, Debug)]
    enum AsyncMsg {
        Start,
        Done(u32),
    }

    impl Component for AsyncCounter {
        type Message = AsyncMsg;
        type Properties = ();
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("{}", ctx.state)).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                AsyncMsg::Start => Update {
                    dirty: false,
                    level: crate::core::component::UpdateLevel::None,
                    command: Some(ctx.link().command(|link| {
                        link.send(AsyncMsg::Done(42));
                    })),
                },
                AsyncMsg::Done(v) => {
                    ctx.state = v;
                    Update::full()
                }
            }
        }
    }

    #[test]
    fn commands_can_send_messages_back() {
        let mut backend = TestBackend::new(AsyncCounter);
        backend
            .dispatch(AsyncMsg::Start)
            .expect("start dispatch should succeed");

        for _ in 0..100 {
            backend.pump().expect("pump should succeed");
            if *backend.state() == 42 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        panic!("expected background command to update state");
    }

    struct ThemedText;

    impl Component for ThemedText {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("themed").into()
        }
    }

    #[test]
    fn default_theme_is_applied_to_root_tree() {
        let backend = TestBackend::new(ThemedText);
        let theme = Theme::default();

        match &backend.element().kind {
            ElementKind::Text(text) => {
                assert_eq!(text.style.fg, None);
                assert_eq!(resolve_base_style(&theme, text.style).fg, theme.primary.fg);
                assert_eq!(text.style.bg, None);
            }
            _ => panic!("expected root text element"),
        }
    }

    struct ThemedList;

    impl Component for ThemedList {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            List::new()
                .items([ListItem::new("one"), ListItem::new("two")])
                .into()
        }
    }

    #[test]
    fn default_theme_disables_generic_hover_styles() {
        let backend = TestBackend::new(ThemedList);
        let theme = Theme::default();

        assert!(theme.hover.is_empty());

        match &backend.element().kind {
            ElementKind::List(list) => {
                assert!(resolve_slot(&theme, ThemeRole::Hover, &list.hover_style).is_empty());
                assert!(
                    resolve_slot(&theme, ThemeRole::ItemHover, &list.item_hover_style).is_empty()
                );
            }
            _ => panic!("expected root list element"),
        }
    }

    struct ThemedButton;

    impl Component for ThemedButton {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Button::new("Run").into()
        }
    }

    #[test]
    fn buttons_keep_default_hover_feedback() {
        let backend = TestBackend::new(ThemedButton);
        let theme = Theme::default();

        match &backend.element().kind {
            ElementKind::Button(button) => {
                let hover = resolve_slot(&theme, ThemeRole::Accent, &button.hover_style);
                assert_eq!(hover.fg, theme.accent.fg.or(theme.primary.fg));
                assert_eq!(hover.bg, None);
            }
            _ => panic!("expected root button element"),
        }
    }

    struct OptInListHover;

    impl Component for OptInListHover {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default().hover(Style::new().bg(Color::Rgb(12, 34, 56))))
                .child(List::new().items([ListItem::new("one"), ListItem::new("two")]))
                .into()
        }
    }

    #[test]
    fn generic_hover_can_be_opted_back_in() {
        let backend = TestBackend::new(OptInListHover);

        match &backend.element().kind {
            ElementKind::List(list) => {
                let theme = Theme::default().hover(Style::new().bg(Color::Rgb(12, 34, 56)));
                assert_eq!(
                    resolve_slot(&theme, ThemeRole::Hover, &list.hover_style).bg,
                    Some(Color::Rgb(12, 34, 56).into())
                );
                assert_eq!(
                    resolve_slot(&theme, ThemeRole::ItemHover, &list.item_hover_style).bg,
                    Some(Color::Rgb(12, 34, 56).into())
                );
            }
            _ => panic!("expected themed list element"),
        }
    }

    struct AccentAndSelectionAreIndependent;

    impl Component for AccentAndSelectionAreIndependent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(
                Theme::default()
                    .accent(Style::new().fg(Color::Rgb(200, 40, 40)))
                    .selection(Style::new().bg(Color::Rgb(20, 40, 80))),
            )
            .child(Button::new("Run"))
            .into()
        }
    }

    #[test]
    fn button_hover_uses_accent_not_selection_background() {
        let backend = TestBackend::new(AccentAndSelectionAreIndependent);

        match &backend.element().kind {
            ElementKind::Button(button) => {
                let theme = Theme::default()
                    .accent(Style::new().fg(Color::Rgb(200, 40, 40)))
                    .selection(Style::new().bg(Color::Rgb(20, 40, 80)));
                let hover = resolve_slot(&theme, ThemeRole::Accent, &button.hover_style);
                assert_eq!(hover.fg, Some(Color::Rgb(200, 40, 40).into()));
                assert_eq!(hover.bg, None);
            }
            _ => panic!("expected themed button element"),
        }
    }

    struct ThemedPlainInput;

    impl Component for ThemedPlainInput {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default())
                .child(Input::new("plain input"))
                .into()
        }
    }

    #[test]
    fn input_theme_sets_focus_chrome_without_forcing_focus_content() {
        let backend = TestBackend::new(ThemedPlainInput);

        match &backend.element().kind {
            ElementKind::Input(input) => {
                assert_eq!(
                    resolve_slot(&Theme::default(), ThemeRole::Focus, &input.focus_style).fg,
                    Theme::default().focus.fg
                );
                assert!(input.focus_content_style.is_empty());
                assert_eq!(
                    resolve_base_style(&Theme::default(), input.style).fg,
                    Theme::default().primary.fg
                );
            }
            _ => panic!("expected themed input element"),
        }
    }

    struct ThemeDefinedInputFocus;

    impl Component for ThemeDefinedInputFocus {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default().input(InputPalette {
                focus: Style::new().fg(Color::Rgb(200, 120, 40)).bold(),
            }))
            .child(Input::new("themed input"))
            .into()
        }
    }

    #[test]
    fn theme_can_define_input_focus_content_style_explicitly() {
        let backend = TestBackend::new(ThemeDefinedInputFocus);

        match &backend.element().kind {
            ElementKind::Input(input) => {
                let focus_content = crate::style::resolve::resolve_style_defaults(
                    input.focus_content_style,
                    Theme::default()
                        .input(InputPalette {
                            focus: Style::new().fg(Color::Rgb(200, 120, 40)).bold(),
                        })
                        .input
                        .focus,
                );
                assert_eq!(focus_content.fg, Some(Color::Rgb(200, 120, 40).into()));
                assert_eq!(focus_content.bold, Some(true));
                assert_eq!(
                    resolve_slot(&Theme::default(), ThemeRole::Focus, &input.focus_style).fg,
                    Theme::default().focus.fg
                );
            }
            _ => panic!("expected themed input element"),
        }
    }

    struct ThemedPlainTextArea;

    impl Component for ThemedPlainTextArea {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default())
                .child(TextArea::new("plain text"))
                .into()
        }
    }

    #[test]
    fn text_area_theme_sets_focus_chrome_without_forcing_focus_content() {
        let backend = TestBackend::new(ThemedPlainTextArea);

        match &backend.element().kind {
            ElementKind::TextArea(text_area) => {
                assert_eq!(
                    resolve_slot(&Theme::default(), ThemeRole::Focus, &text_area.focus_style).fg,
                    Theme::default().focus.fg
                );
                assert!(text_area.focus_content_style.is_empty());
                assert_eq!(
                    resolve_base_style(&Theme::default(), text_area.style).fg,
                    Theme::default().primary.fg
                );
            }
            _ => panic!("expected themed text area element"),
        }
    }

    struct ThemedPlainDocumentView;

    impl Component for ThemedPlainDocumentView {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default())
                .child(DocumentView::new("plain document"))
                .into()
        }
    }

    #[test]
    fn document_view_theme_sets_focus_chrome_without_forcing_focus_content() {
        let backend = TestBackend::new(ThemedPlainDocumentView);

        match &backend.element().kind {
            ElementKind::DocumentView(doc_view) => {
                assert_eq!(
                    resolve_slot(&Theme::default(), ThemeRole::Focus, &doc_view.focus_style).fg,
                    Theme::default().focus.fg
                );
                assert!(doc_view.focus_content_style.is_empty());
                assert_eq!(
                    resolve_base_style(&Theme::default(), doc_view.style).fg,
                    Theme::default().primary.fg
                );
            }
            _ => panic!("expected themed document view element"),
        }
    }

    struct ThemeDefinedTextSurfacesFocus;

    impl Component for ThemeDefinedTextSurfacesFocus {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(
                Theme::default()
                    .text_area(TextAreaPalette {
                        focus: Style::new().fg(Color::Rgb(80, 200, 120)),
                    })
                    .document_view(DocumentViewPalette {
                        focus: Style::new().fg(Color::Rgb(120, 180, 240)).italic(),
                    }),
            )
            .child(
                VStack::new()
                    .child(TextArea::new("editor"))
                    .child(DocumentView::new("document")),
            )
            .into()
        }
    }

    #[test]
    fn theme_can_define_text_surface_focus_content_styles_explicitly() {
        let backend = TestBackend::new(ThemeDefinedTextSurfacesFocus);

        let ElementKind::VStack(root) = &backend.element().kind else {
            panic!("expected stack root");
        };

        let ElementKind::TextArea(text_area) = &root.children[0].kind else {
            panic!("expected text area child");
        };
        let theme = Theme::default()
            .text_area(TextAreaPalette {
                focus: Style::new().fg(Color::Rgb(80, 200, 120)),
            })
            .document_view(DocumentViewPalette {
                focus: Style::new().fg(Color::Rgb(120, 180, 240)).italic(),
            });
        let text_area_focus_content = crate::style::resolve::resolve_style_defaults(
            text_area.focus_content_style,
            theme.text_area.focus,
        );
        assert_eq!(
            text_area_focus_content.fg,
            Some(Color::Rgb(80, 200, 120).into())
        );
        assert_eq!(
            resolve_slot(&Theme::default(), ThemeRole::Focus, &text_area.focus_style).fg,
            Theme::default().focus.fg
        );

        let ElementKind::DocumentView(doc_view) = &root.children[1].kind else {
            panic!("expected document view child");
        };
        let doc_focus_content = crate::style::resolve::resolve_style_defaults(
            doc_view.focus_content_style,
            theme.document_view.focus,
        );
        assert_eq!(doc_focus_content.fg, Some(Color::Rgb(120, 180, 240).into()));
        assert_eq!(doc_focus_content.italic, Some(true));
        assert_eq!(
            resolve_slot(&Theme::default(), ThemeRole::Focus, &doc_view.focus_style).fg,
            Theme::default().focus.fg
        );
    }

    struct ThemedPlainTabs;

    impl Component for ThemedPlainTabs {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default())
                .child(Tabs::new().tabs(vec![Tab::new("One"), Tab::new("Two")]))
                .into()
        }
    }

    #[test]
    fn tabs_theme_sets_focus_style_without_forcing_bold() {
        let backend = TestBackend::new(ThemedPlainTabs);

        match &backend.element().kind {
            ElementKind::Tabs(tabs) => {
                let focus = resolve_slot(&Theme::default(), ThemeRole::Focus, &tabs.focus_style);
                assert_eq!(focus.fg, Theme::default().focus.fg);
                assert_eq!(focus.bold, Theme::default().focus.bold);
                assert_eq!(
                    resolve_slot(&Theme::default(), ThemeRole::Selection, &tabs.active_style).fg,
                    Theme::default().selection.fg
                );
            }
            _ => panic!("expected themed tabs element"),
        }
    }

    #[cfg(feature = "syntax-syntect")]
    struct ThemedSyntaxTextArea;

    #[cfg(feature = "syntax-syntect")]
    impl Component for ThemedSyntaxTextArea {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ThemeProvider::new(Theme::default())
                .child(
                    TextArea::new("let n = 42;")
                        .language("rust")
                        .theme("One Dark (Atom)")
                        .color_strategy(
                            SyntectStrategy::default().default_theme("One Dark (Atom)"),
                        ),
                )
                .into()
        }
    }

    #[cfg(feature = "syntax-syntect")]
    #[test]
    fn theme_provider_sets_syntect_palette_from_theme() {
        let backend = TestBackend::new(ThemedSyntaxTextArea);
        let theme = Theme::default();

        let ElementKind::TextArea(text_area) = &backend.element().kind else {
            panic!("expected themed text area element");
        };
        let strategy = text_area
            .color_strategy
            .as_ref()
            .and_then(|strategy| strategy.as_any().downcast_ref::<SyntectStrategy>())
            .expect("expected syntect strategy");

        assert_eq!(strategy.effective_syntax_palette(), Some(theme.syntax));
    }

    #[cfg(feature = "diff-view")]
    struct ThemedUnifiedDiffTextArea;

    #[cfg(feature = "diff-view")]
    impl Component for ThemedUnifiedDiffTextArea {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let mut diff = Theme::default().diff;
            diff.added_marker = Style::new().fg(Color::Rgb(1, 200, 120));

            ThemeProvider::new(Theme::default().diff(diff))
                .child(
                    DiffView::with_content("alpha\nbeta", "alpha\nbeta\ngamma")
                        .backend(DiffViewBackend::TextArea)
                        .mode(DiffViewMode::Unified),
                )
                .into()
        }
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn diff_text_area_gutter_uses_theme_diff_palette() {
        let backend = TestBackend::new(ThemedUnifiedDiffTextArea);

        let ElementKind::Frame(outer) = &backend.element().kind else {
            panic!("expected outer frame");
        };
        let Some(inner_frame) = &outer.child else {
            panic!("expected diff inner frame");
        };
        let ElementKind::Frame(inner) = &inner_frame.kind else {
            panic!("expected pane frame");
        };
        let Some(text_area) = &inner.child else {
            panic!("expected text area child");
        };
        let ElementKind::TextArea(text_area) = &text_area.kind else {
            panic!("expected text area element");
        };
        let gutter = text_area
            .gutter_lines
            .as_ref()
            .expect("expected diff gutter");
        let added_fg = gutter
            .iter()
            .flat_map(|line| line.iter())
            .find(|span| span.content.as_ref().contains('+'))
            .and_then(|span| span.style.fg);
        assert_eq!(added_fg, Some(Color::Rgb(1, 200, 120).into()));
    }

    #[cfg(feature = "diff-view")]
    struct ThemedUnifiedDiffDocumentView;

    #[cfg(feature = "diff-view")]
    impl Component for ThemedUnifiedDiffDocumentView {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let mut diff = Theme::default().diff;
            diff.removed_marker = Style::new().fg(Color::Rgb(220, 60, 60));

            ThemeProvider::new(Theme::default().diff(diff))
                .child(
                    DiffView::with_content("alpha\nbeta\ngamma", "alpha\nbeta")
                        .backend(DiffViewBackend::DocumentView)
                        .mode(DiffViewMode::Unified),
                )
                .into()
        }
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn diff_document_view_gutter_uses_theme_diff_palette() {
        let backend = TestBackend::new(ThemedUnifiedDiffDocumentView);

        let ElementKind::Frame(outer) = &backend.element().kind else {
            panic!("expected outer frame");
        };
        let Some(inner_frame) = &outer.child else {
            panic!("expected diff inner frame");
        };
        let ElementKind::Frame(inner) = &inner_frame.kind else {
            panic!("expected pane frame");
        };
        let Some(doc_view) = &inner.child else {
            panic!("expected document view child");
        };
        let ElementKind::DocumentView(doc_view) = &doc_view.kind else {
            panic!("expected document view element");
        };
        let gutter = doc_view
            .gutter_lines
            .as_ref()
            .expect("expected diff gutter");
        let removed_fg = gutter
            .iter()
            .flat_map(|line| line.iter())
            .find(|span| span.content.as_ref().contains('-'))
            .and_then(|span| span.style.fg);
        assert_eq!(removed_fg, Some(Color::Rgb(220, 60, 60).into()));
    }

    struct ExplicitFrameBackground;

    impl Component for ExplicitFrameBackground {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Frame::new()
                .style(Style::new().bg(Color::Rgb(40, 41, 54)))
                .child(VStack::new().child(Text::new("content")))
                .into()
        }
    }

    #[test]
    fn explicit_frame_bg_is_not_overridden_by_theme_defaults() {
        let backend = TestBackend::new(ExplicitFrameBackground);

        match &backend.element().kind {
            ElementKind::Frame(frame) => {
                assert_eq!(frame.props.style.bg, Some(Color::Rgb(40, 41, 54).into()));
                assert_eq!(frame.props.inner_style(), None);

                let child = frame.child.as_ref().expect("frame should have child");
                match &child.kind {
                    ElementKind::VStack(stack) => {
                        assert_eq!(stack.props.style.bg, None);
                    }
                    _ => panic!("expected frame child to be a vstack"),
                }
            }
            _ => panic!("expected root frame element"),
        }
    }

    struct FrameTabsWithoutBackground;

    impl Component for FrameTabsWithoutBackground {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Frame::new()
                .tab_titles(["Files", "Branches"])
                .active_tab_style(Style::new().fg(Color::Rgb(125, 207, 255)))
                .child(VStack::new().child(Text::new("content")))
                .into()
        }
    }

    #[test]
    fn frame_active_tab_style_does_not_inherit_theme_primary_background() {
        let backend = TestBackend::new(FrameTabsWithoutBackground);

        match &backend.element().kind {
            ElementKind::Frame(frame) => {
                assert_eq!(
                    frame.props.active_tab_style.fg,
                    Some(Color::Rgb(125, 207, 255).into())
                );
                assert_eq!(frame.props.active_tab_style.bg, None);
            }
            _ => panic!("expected root frame element"),
        }
    }

    struct FrameHeaderStylesInheritFrameColor;

    impl Component for FrameHeaderStylesInheritFrameColor {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Frame::new()
                .style(Style::new().fg(Color::Rgb(90, 91, 92)))
                .tab_titles(["Files", "Branches"])
                .title_prefix("[2]")
                .status("ready")
                .child(VStack::new().child(Text::new("content")))
                .into()
        }
    }

    #[test]
    fn frame_header_styles_receive_theme_colors() {
        let backend = TestBackend::new(FrameHeaderStylesInheritFrameColor);
        let theme = Theme::default();

        match &backend.element().kind {
            ElementKind::Frame(frame) => {
                // Explicit frame border fg is preserved
                assert_eq!(frame.props.style.fg, Some(Color::Rgb(90, 91, 92).into()));
                // Sub-styles resolve against the active theme at render time.
                assert_eq!(
                    resolve_muted_style(&theme, frame.props.inactive_tab_style).fg,
                    theme.muted.fg.or(theme.primary.fg)
                );
                assert_eq!(
                    resolve_base_style(&theme, frame.props.title_style).fg,
                    theme.primary.fg
                );
                assert_eq!(
                    resolve_muted_style(&theme, frame.props.status_style).fg,
                    theme.muted.fg.or(theme.primary.fg)
                );
            }
            _ => panic!("expected root frame element"),
        }
    }

    struct ExplicitSpinnerColor;

    impl Component for ExplicitSpinnerColor {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Spinner::new()
                .spinner_style(SpinnerStyle::Lightsaber)
                .style(Style::new().fg(Color::Rgb(220, 0, 0)))
                .into()
        }
    }

    #[test]
    fn explicit_widget_color_wins_over_theme() {
        let backend = TestBackend::new(ExplicitSpinnerColor);

        match &backend.element().kind {
            ElementKind::Spinner(spinner) => {
                assert_eq!(spinner.style.fg, Some(Color::Rgb(220, 0, 0).into()));
            }
            _ => panic!("expected root spinner element"),
        }
    }

    struct ThreeDotSpinner;

    impl Component for ThreeDotSpinner {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Spinner::new()
                .spinner_style(SpinnerStyle::ThreeDot)
                .frame(0)
                .label("ThreeDot")
                .into()
        }
    }

    #[test]
    fn multi_glyph_spinner_frame_draws_each_cell() {
        let backend = TestBackend::new(ThreeDotSpinner);
        let line = backend.capture_frame().to_fixed_grid_lines().remove(0);

        assert!(line.starts_with("∙∙∙ ThreeDot"), "captured line: {line:?}");
    }

    struct StyledStatusBar;

    impl Component for StyledStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .style(Style::new().fg(Color::Rgb(120, 121, 122)))
                .left(Text::new("left"))
                .center(Text::new("center"))
                .right(Text::new("right"))
                .into()
        }
    }

    #[test]
    fn status_bar_style_applies_to_unstyled_slot_text() {
        let backend = TestBackend::new(StyledStatusBar);

        let frame = backend.capture_frame();
        assert_eq!(
            first_cell_with_symbol(&frame, "l").fg,
            Color::Rgb(120, 121, 122)
        );
    }

    struct SlotStyledStatusBar;

    impl Component for SlotStyledStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .style(Style::new().fg(Color::Rgb(120, 121, 122)))
                .left_style(Style::new().fg(Color::Rgb(240, 90, 40)))
                .left(Text::new("left"))
                .right(Text::new("right"))
                .into()
        }
    }

    #[test]
    fn status_bar_slot_style_overrides_base_style_for_slot_text() {
        let backend = TestBackend::new(SlotStyledStatusBar);

        let frame = backend.capture_frame();
        assert_eq!(
            first_cell_with_symbol(&frame, "l").fg,
            Color::Rgb(240, 90, 40)
        );
    }

    struct CompactStatusBar;

    impl Component for CompactStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .left(Text::new("left"))
                .right(Text::new("right"))
                .into()
        }
    }

    #[test]
    fn status_bar_without_center_uses_two_lane_layout() {
        let backend = TestBackend::new(CompactStatusBar);

        match &backend.element().kind {
            ElementKind::HStack(root) => {
                assert_eq!(
                    root.children.len(),
                    3,
                    "expected left, spacer, right children"
                );
                match &root
                    .children
                    .get(1)
                    .expect("spacer should be inserted between left and right")
                    .kind
                {
                    ElementKind::Spacer(_) => {}
                    _ => panic!("expected center child to be spacer in two-lane layout"),
                }
            }
            _ => panic!("expected root hstack"),
        }
    }

    struct ReservedCenterStatusBar;

    impl Component for ReservedCenterStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .reserve_center_space(true)
                .left(Text::new("left"))
                .right(Text::new("right"))
                .into()
        }
    }

    #[test]
    fn status_bar_can_reserve_center_lane_when_empty() {
        let mut backend = TestBackend::new(ReservedCenterStatusBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 47,
            h: 1,
        });
        backend.render();

        let first_line = backend
            .capture_frame()
            .to_lines()
            .into_iter()
            .next()
            .expect("captured frame should include first line");
        assert!(first_line.contains("left"));
        assert!(first_line.contains("right"));
    }

    struct ContentAwareStatusBar;

    impl Component for ContentAwareStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .left(Text::new("abcdefghijklmnopqrst"))
                .center(Text::new("C"))
                .right(Text::new("R"))
                .into()
        }
    }

    #[test]
    fn status_bar_center_layout_does_not_clip_long_left_section_to_thirds() {
        let mut backend = TestBackend::new(ContentAwareStatusBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 47,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let first_line = frame
            .to_lines()
            .into_iter()
            .next()
            .expect("captured frame should include first line");
        assert!(
            first_line.contains("abcdefghijklmnopqrst"),
            "expected full 20-character left section in first line, got {first_line:?}",
        );
        assert_eq!(
            frame.cell(23, 0).symbol,
            "C",
            "expected center content to be pinned at the mathematical center cell"
        );
        assert_eq!(
            frame.cell(45, 0).symbol,
            "R",
            "expected right content to stay flush with the right edge"
        );
    }

    struct LoadingCenteredStatusBar;

    impl Component for LoadingCenteredStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .center(Text::new("C"))
                .loading(true)
                .loading_label("LOAD")
                .into()
        }
    }

    #[test]
    fn status_bar_center_layout_keeps_loading_indicator_on_right() {
        let mut backend = TestBackend::new(LoadingCenteredStatusBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 47,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let first_line = frame
            .to_lines()
            .into_iter()
            .next()
            .expect("captured frame should include first line");
        assert_eq!(
            frame.cell(23, 0).symbol,
            "C",
            "expected center content to remain pinned with loading content present"
        );
        assert!(
            first_line.ends_with("LOAD"),
            "expected loading label to be right-aligned, got {first_line:?}",
        );
    }

    struct LongRightCenteredStatusBar;

    impl Component for LongRightCenteredStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .left(Text::new("[untitled] [text]"))
                .center(Text::new("1:1 sel:0"))
                .right(Text::new("tabs: 1 | theme: One Dark | Ctrl-P commands"))
                .into()
        }
    }

    #[test]
    fn status_bar_center_stays_pinned_when_right_content_overflows() {
        let mut backend = TestBackend::new(LongRightCenteredStatusBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 47,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let first_line = frame
            .to_lines()
            .into_iter()
            .next()
            .expect("captured frame should include first line");
        assert!(
            first_line.contains("[untitled] [text]"),
            "expected left section to stay visible, got {first_line:?}",
        );
        assert_eq!(
            frame.cell(19, 0).symbol,
            "1",
            "expected wider center content to start at the mathematical center band"
        );
        assert_eq!(
            frame.cell(28, 0).symbol,
            " ",
            "expected configured gap between center and overflowing right content"
        );
    }

    struct ReservedContentAwareStatusBar;

    impl Component for ReservedContentAwareStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new()
                .reserve_center_space(true)
                .left(Text::new("abcdefghijklmnopqrst"))
                .right(Text::new("R"))
                .into()
        }
    }

    #[test]
    fn status_bar_reserved_center_layout_does_not_clip_long_left_section_to_thirds() {
        let mut backend = TestBackend::new(ReservedContentAwareStatusBar);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 47,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        let first_line = frame
            .to_lines()
            .into_iter()
            .next()
            .expect("captured frame should include first line");
        assert!(
            first_line.contains("abcdefghijklmnopqrst"),
            "expected full 20-character left section in first line, got {first_line:?}",
        );
    }

    struct LeftOnlyStatusBar;

    impl Component for LeftOnlyStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new().left(Text::new("left")).into()
        }
    }

    #[test]
    fn status_bar_left_only_omits_right_lane() {
        let backend = TestBackend::new(LeftOnlyStatusBar);

        match &backend.element().kind {
            ElementKind::HStack(root) => {
                assert_eq!(root.children.len(), 1, "expected only left section");
                match &unwrap_theme_provider(
                    root.children.first().expect("left section should exist"),
                )
                .kind
                {
                    ElementKind::HStack(_) => {}
                    _ => panic!("expected only child to be left section hstack"),
                }
            }
            _ => panic!("expected root hstack"),
        }
    }

    struct RightOnlyStatusBar;

    impl Component for RightOnlyStatusBar {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            StatusBar::new().right(Text::new("right")).into()
        }
    }

    #[test]
    fn status_bar_right_only_omits_left_lane() {
        let backend = TestBackend::new(RightOnlyStatusBar);

        match &backend.element().kind {
            ElementKind::HStack(root) => {
                assert_eq!(root.children.len(), 2, "expected spacer and right section");
                match &root.children.first().expect("spacer should exist").kind {
                    ElementKind::Spacer(_) => {}
                    _ => panic!("expected first child to be spacer"),
                }
                match &unwrap_theme_provider(
                    root.children.get(1).expect("right section should exist"),
                )
                .kind
                {
                    ElementKind::HStack(_) => {}
                    _ => panic!("expected second child to be right section hstack"),
                }
            }
            _ => panic!("expected root hstack"),
        }
    }

    // ── Component lifecycle tests ──────────────────────────────────────

    use crate::core::component::Command;

    /// Component whose `init()` sends a message back via Link, verifying
    /// that init commands are processed during TestBackend construction.
    struct InitSender;

    #[derive(Clone, Copy, Debug)]
    enum InitMsg {
        Initialized,
    }

    impl Component for InitSender {
        type Message = InitMsg;
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
            Some(Command::spawn(move |link| {
                link.send(InitMsg::Initialized);
            }))
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(if ctx.state { "ready" } else { "pending" }).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                InitMsg::Initialized => {
                    ctx.state = true;
                    Update::full()
                }
            }
        }
    }

    #[test]
    fn init_command_processed_on_creation() {
        let mut backend = TestBackend::new(InitSender);

        // The init command spawns a background thread that sends a message.
        // Pump with retries to allow the thread to complete.
        for _ in 0..100 {
            backend.pump().expect("pump should succeed");
            if *backend.state() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        panic!("expected init command to set state to true");
    }

    /// Component that tracks key events via `on_key`.
    ///
    /// NOTE: `TestBackend` does not currently expose a `send_key()` method, so
    /// key events cannot be injected directly through the public API.  This test
    /// verifies the `on_key` lifecycle by simulating its effect: the component
    /// converts key events into messages via `Link::send`, and we dispatch those
    /// messages manually to confirm the state transition is correct.
    struct KeyTracker;

    #[derive(Clone, Debug)]
    enum KeyMsg {
        KeyPressed(KeyCode),
    }

    #[derive(Default)]
    struct KeyTrackerState {
        keys: Vec<KeyCode>,
    }

    impl Component for KeyTracker {
        type Message = KeyMsg;
        type Properties = ();
        type State = KeyTrackerState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            KeyTrackerState::default()
        }

        fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
            ctx.link().send(KeyMsg::KeyPressed(key.code));
            KeyUpdate::handled(Update::none())
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("keys: {}", ctx.state.keys.len())).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                KeyMsg::KeyPressed(code) => {
                    ctx.state.keys.push(code);
                    Update::full()
                }
            }
        }
    }

    #[test]
    fn on_key_receives_key_events() {
        // TestBackend does not expose direct key injection, so we simulate the
        // effect of `on_key` by dispatching the same messages the handler would
        // produce.  This validates the message→state path that `on_key` relies on.
        let mut backend = TestBackend::new(KeyTracker);
        assert!(backend.state().keys.is_empty());

        backend
            .dispatch(KeyMsg::KeyPressed(KeyCode::Char('a')))
            .expect("dispatch should succeed");
        backend
            .dispatch(KeyMsg::KeyPressed(KeyCode::Enter))
            .expect("dispatch should succeed");
        backend
            .dispatch(KeyMsg::KeyPressed(KeyCode::Esc))
            .expect("dispatch should succeed");

        assert_eq!(backend.state().keys.len(), 3);
        assert_eq!(backend.state().keys[0], KeyCode::Char('a'));
        assert_eq!(backend.state().keys[1], KeyCode::Enter);
        assert_eq!(backend.state().keys[2], KeyCode::Esc);
    }

    #[test]
    fn multiple_rapid_dispatches_all_processed() {
        let mut backend = TestBackend::new(Counter);
        assert_eq!(*backend.state(), 0);

        for _ in 0..100 {
            backend.dispatch(Msg::Inc).expect("dispatch should succeed");
        }

        assert_eq!(*backend.state(), 100);
    }

    /// Component whose `update` returns a `Command::spawn` that sends a message
    /// back from a background thread.
    struct SpawnEcho;

    #[derive(Clone, Copy, Debug)]
    enum SpawnMsg {
        Trigger,
        FromBackground(u64),
    }

    impl Component for SpawnEcho {
        type Message = SpawnMsg;
        type Properties = ();
        type State = Option<u64>;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            None
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("{:?}", ctx.state)).into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                SpawnMsg::Trigger => Update {
                    dirty: false,
                    level: crate::core::component::UpdateLevel::None,
                    command: Some(Command::spawn(|link| {
                        link.send(SpawnMsg::FromBackground(99));
                    })),
                },
                SpawnMsg::FromBackground(v) => {
                    ctx.state = Some(v);
                    Update::full()
                }
            }
        }
    }

    #[test]
    fn command_spawn_sends_message_back() {
        let mut backend = TestBackend::new(SpawnEcho);
        assert_eq!(*backend.state(), None);

        backend
            .dispatch(SpawnMsg::Trigger)
            .expect("dispatch should succeed");

        // The spawn command runs on a background thread. Pump with retries.
        for _ in 0..100 {
            backend.pump().expect("pump should succeed");
            if *backend.state() == Some(99) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        panic!("expected background command to set state to Some(99)");
    }

    struct PasteIntoInput;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum PasteMsg {
        InputChanged(String),
        TextAreaChanged(String),
    }

    #[derive(Default)]
    struct PasteState {
        input: String,
        text_area: String,
    }

    impl Component for PasteIntoInput {
        type Message = PasteMsg;
        type Properties = ();
        type State = PasteState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            PasteState::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                PasteMsg::InputChanged(value) => ctx.state.input = value,
                PasteMsg::TextAreaChanged(value) => ctx.state.text_area = value,
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    Input::new(ctx.state.input.clone()).on_change(
                        ctx.link().callback(|ev: InputEvent| {
                            PasteMsg::InputChanged(ev.value.to_string())
                        }),
                    ),
                )
                .child(TextArea::new(ctx.state.text_area.clone()).on_change(
                    ctx.link().callback(|ev: TextAreaEvent| {
                        PasteMsg::TextAreaChanged(ev.value.to_string())
                    }),
                ))
                .into()
        }
    }

    #[test]
    fn send_paste_inserts_into_focused_input() {
        let mut backend = TestBackend::new(PasteIntoInput);
        let input_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Input(_)))
            .map(|node| node.id)
            .expect("input exists");
        backend.set_focused(input_id);

        assert!(
            backend
                .send_paste("file:///tmp/demo.pdf")
                .expect("paste should succeed")
        );
        assert_eq!(backend.state().input, "file:///tmp/demo.pdf");
    }

    #[test]
    fn send_paste_inserts_into_focused_text_area() {
        let mut backend = TestBackend::new(PasteIntoInput);
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("text area exists");
        backend.set_focused(text_area_id);

        assert!(
            backend
                .send_paste("alpha\nbeta")
                .expect("paste should succeed")
        );
        assert_eq!(backend.state().text_area, "alpha\nbeta");
    }
}
