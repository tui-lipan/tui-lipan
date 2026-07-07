//! Layered keyboard dispatch for the native event loop.

use crate::app::input::command_registry::{CommandRegistry, CommandShortcutResult};
use crate::app::input::focus;
use crate::app::input::handlers::KeyCtx;
use crate::app::input::key_dispatch::{
    CommandDispatchState, DispatchOps, DispatchOutcome, FocusKind, FrameworkDispatch,
    TerminalPreflightDispatch, dispatch_key,
};
use crate::app::input::keyboard;
use crate::app::input::keymap::{Action, Keymap, KeymapRuntime, KeymapRuntimeResult};
use crate::app::input::runtime_dispatch::{
    FrameworkSideEffect, RuntimeKeyDispatchConfig, RuntimeKeyDispatchOutcome,
    RuntimeKeyDispatchState,
};
use crate::app::interaction_state::DirtyLevel;
use crate::core::component::Component;
use crate::core::element::Key;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, OverlayRoot};
use crate::layout::tag::Tag;

use super::AppRunner;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayeredKeyEventResult {
    pub consumed: bool,
    pub quit: bool,
    pub dirty_override: Option<DirtyLevel>,
    pub mark_layout: bool,
    pub mark_full: bool,
    pub terminal_shift_navigation: bool,
}

impl<C: Component> AppRunner<C> {
    pub(crate) fn dispatch_layered_key(&mut self, key: KeyEvent) -> LayeredKeyEventResult {
        self.framework_effects.clear();

        if matches!(key.code, KeyCode::Esc) {
            self.key_dispatch_state.reset_command_chord();
        }

        let selection = self.dispatch_selection_clipboard_shortcut(key);
        if selection.handled {
            return LayeredKeyEventResult {
                consumed: true,
                dirty_override: selection.dirty_override,
                mark_full: selection.dirty_override.is_none(),
                ..Default::default()
            };
        }

        let focus_kind = if self
            .focus
            .focused
            .is_some_and(|id| self.core.tree.is_valid(id) && self.focused_is_terminal(id))
        {
            FocusKind::Terminal
        } else {
            FocusKind::Widget
        };

        if focus_kind == FocusKind::Widget {
            match self.keymap_runtime.feed(key) {
                KeymapRuntimeResult::Pending => {
                    return LayeredKeyEventResult {
                        consumed: true,
                        ..Default::default()
                    };
                }
                KeymapRuntimeResult::Matched(binding) if binding.is_chord => {
                    if binding.action == Action::Quit {
                        return LayeredKeyEventResult {
                            consumed: true,
                            quit: true,
                            mark_full: true,
                            ..Default::default()
                        };
                    }
                    if binding.action == Action::DismissOverlay && self.handle_overlay_escape() {
                        return LayeredKeyEventResult {
                            consumed: true,
                            mark_full: true,
                            ..Default::default()
                        };
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
            return LayeredKeyEventResult {
                consumed: true,
                mark_full: true,
                ..Default::default()
            };
        }

        if self.top_capturing_overlay_is_empty() {
            if self
                .keymap
                .matches(key)
                .iter()
                .any(|binding| binding.action == Action::Quit)
            {
                return LayeredKeyEventResult {
                    consumed: true,
                    quit: true,
                    mark_full: true,
                    ..Default::default()
                };
            }
            return LayeredKeyEventResult {
                consumed: true,
                ..Default::default()
            };
        }

        if self
            .core
            .tree
            .top_capturing_overlay()
            .is_some_and(|overlay| overlay.captures_focus)
            && self.focus.focused.is_none()
            && !matches!(key.code, KeyCode::Esc)
        {
            return LayeredKeyEventResult {
                consumed: true,
                ..Default::default()
            };
        }

        if focus_kind == FocusKind::Terminal {
            self.keymap_runtime.reset();
        }

        let key_dispatch_config = self.key_dispatch_config;
        let command_registry = self.core.ctx.command_registry();

        let AppRunner {
            core,
            focus: focus_state,
            widgets,
            keymap,
            keymap_runtime,
            text_area_newline_binding,
            clipboard,
            clipboard_config,
            copy_feedback,
            key_dispatch_state,
            framework_effects,
            ..
        } = self;

        let mut key_ctx = KeyCtx {
            read_only_selection: Some(&widgets.read_only_selection),
            input_history: &mut widgets.input_history,
            textarea_history: &mut widgets.textarea_history,
            text_area_vim_state: &mut widgets.text_area_vim_state,
            hex_history: &mut widgets.hex_history,
            hex_pending_edit: &mut widgets.hex_pending_edit,
            keymap,
            text_area_newline_binding: *text_area_newline_binding,
            clipboard,
            clipboard_config,
            copy_feedback,
            dirty_override: None,
        };

        let mut ops = RunnerDispatchOps {
            core,
            focused: &mut focus_state.focused,
            focused_key: &mut focus_state.focused_key,
            focused_tag: &mut focus_state.focused_tag,
            focus_stack: &mut focus_state.focus_stack,
            keymap,
            keymap_runtime,
            key_dispatch_state,
            key_dispatch_config,
            framework_effects,
            command_registry,
            key_ctx: &mut key_ctx,
            clipboard,
            clipboard_config,
        };

        let outcome = dispatch_key(
            crate::app::input::key_dispatch::DispatchRequest::new(key, focus_kind)
                .key_policy(key_dispatch_config.key_dispatch_policy)
                .terminal_policy(key_dispatch_config.terminal_key_policy)
                .chord_mismatch_policy(key_dispatch_config.chord_mismatch_policy),
            &mut ops,
        );
        let dispatch = ops.finish(outcome);

        let terminal_shift_navigation = dispatch.handled
            && focus_kind == FocusKind::Terminal
            && key.mods.shift
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            );

        LayeredKeyEventResult {
            consumed: dispatch.handled,
            quit: dispatch.quit,
            dirty_override: key_ctx.dirty_override,
            mark_layout: dispatch.layout_dirty,
            mark_full: dispatch.dirty && !dispatch.layout_dirty,
            terminal_shift_navigation,
        }
    }

    fn focused_is_terminal(&self, id: NodeId) -> bool {
        #[cfg(feature = "terminal")]
        {
            matches!(self.core.tree.node(id).kind, NodeKind::Terminal(_))
        }
        #[cfg(not(feature = "terminal"))]
        {
            let _ = id;
            false
        }
    }
}

struct RunnerDispatchOps<'a, 'b, C: Component> {
    core: &'a mut crate::runtime::RuntimeCore<C>,
    focused: &'a mut Option<NodeId>,
    focused_key: &'a mut Option<Key>,
    focused_tag: &'a mut Option<Tag>,
    focus_stack: &'a mut Vec<Option<Key>>,
    keymap: &'a Keymap,
    keymap_runtime: &'a mut KeymapRuntime,
    key_dispatch_state: &'a mut RuntimeKeyDispatchState,
    key_dispatch_config: RuntimeKeyDispatchConfig,
    framework_effects: &'a mut Vec<FrameworkSideEffect>,
    command_registry: CommandRegistry,
    key_ctx: &'a mut KeyCtx<'b>,
    clipboard: &'a crate::clipboard::ClipboardService,
    clipboard_config: &'a crate::clipboard::ClipboardConfig,
}

impl<C: Component> RunnerDispatchOps<'_, '_, C> {
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
            && let Some(saved_key) = self.focus_stack.pop()
        {
            *self.focused_key = saved_key;
            *self.focused = None;
            focus::restore_focus(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
            );
        }
        dismissed
    }

    fn focus_overlay_next(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
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
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }
        focusables.sort_by_key(|id| id.index());
        let prev = if let Some(curr) = *self.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + focusables.len() - 1) % focusables.len()]
        } else {
            focusables[focusables.len() - 1]
        };
        *self.focused = Some(prev);
        *self.focused_key = self.core.tree.node(prev).key.clone();
        *self.focused_tag = Some(crate::layout::tag::tag_of_node(self.core.tree.node(prev)));
        true
    }

    fn focused_is_terminal(&self, id: NodeId) -> bool {
        #[cfg(feature = "terminal")]
        {
            matches!(self.core.tree.node(id).kind, NodeKind::Terminal(_))
        }
        #[cfg(not(feature = "terminal"))]
        {
            let _ = id;
            false
        }
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
            result.layout_dirty = matches!(level, DirtyLevel::LayoutOnly | DirtyLevel::Full);
        }

        result
    }
}

impl<C: Component> DispatchOps for RunnerDispatchOps<'_, '_, C> {
    fn continue_command_chord(&mut self, key: KeyEvent) -> CommandDispatchState {
        let was_pending = self.key_dispatch_state.command_runtime.is_pending();
        if was_pending {
            self.key_dispatch_state.pending_command_prefix = Some(key);
        }
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
                CommandDispatchState::None
            }
            CommandShortcutResult::Pending => CommandDispatchState::Pending,
            CommandShortcutResult::Matched(id) if !was_pending => {
                self.key_dispatch_state.command_runtime.reset();
                CommandDispatchState::None
            }
            CommandShortcutResult::Matched(id) => {
                self.key_dispatch_state.pending_command_prefix = None;
                self.command_registry.execute(id.clone());
                CommandDispatchState::Matched(id.as_str().into())
            }
            CommandShortcutResult::Mismatch => {
                self.key_dispatch_state.pending_command_prefix = None;
                CommandDispatchState::Mismatch
            }
            CommandShortcutResult::None => CommandDispatchState::None,
        }
    }

    fn dispatch_widget(&mut self, key: KeyEvent) -> bool {
        use crate::app::input::runtime_dispatch::dispatch_widget_with_policy;
        dispatch_widget_with_policy(
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
                self.key_ctx.dirty_override = Some(DirtyLevel::Full);
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
            focus::focus_next(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
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
            focus::focus_prev(
                &self.core.tree,
                self.focused,
                self.focused_key,
                self.focused_tag,
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
        #[cfg(feature = "terminal")]
        if let Some(id) = self.focused.filter(|id| self.core.tree.is_valid(*id))
            && self.focused_is_terminal(id)
        {
            use crate::app::input::handlers::terminal::forward_key;
            return forward_key(&mut self.core.tree, id, key);
        }
        keyboard::dispatch_key(&mut self.core.tree, *self.focused, key, self.key_ctx)
    }

    fn dispatch_ambient_scroll(&mut self, key: KeyEvent) -> bool {
        keyboard::dispatch_ambient_page_scroll(&mut self.core.tree, key)
    }
}
