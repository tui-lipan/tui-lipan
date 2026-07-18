//! Shared runtime wiring for layered key dispatch.
#![allow(dead_code)]

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::app::context::{FocusPolicy, TextAreaNewlineBinding};
use crate::app::copy_feedback::CopyFeedbackState;
use crate::app::input::command_registry::{
    CommandRegistry, CommandShortcutResult, CommandShortcutRuntime,
};
use crate::app::input::focus;
use crate::app::input::handlers::KeyCtx;
use crate::app::input::hex_history::HexHistory;
use crate::app::input::key_dispatch::{
    ChordMismatchPolicy, CommandConflictPolicy, CommandDispatchState, DispatchOps, DispatchOutcome,
    DispatchRequest, FocusKind, FrameworkDispatch, KeyDispatchPolicy, TerminalKeyPolicy,
    TerminalPreflightDispatch, dispatch_key,
};
use crate::app::input::keyboard;
use crate::app::input::keymap::{Action, Keymap, KeymapRuntime, KeymapRuntimeResult};
use crate::app::input::text_area_vim::TextAreaVimState;
use crate::app::interaction_state::DirtyLevel;
use crate::app::interaction_state::HexPendingEdit;
use crate::clipboard::{ClipboardConfig, ClipboardService};
use crate::core::element::Key;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::tag::Tag;
use crate::runtime::BubbleKeyResult;
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;

#[cfg(feature = "terminal")]
use crate::app::input::handlers::terminal::{TerminalPreflightResult, forward_key, preflight_key};

#[derive(Clone, Copy)]
pub(crate) struct RuntimeKeyDispatchConfig {
    pub focus_policy: FocusPolicy,
    pub key_dispatch_policy: KeyDispatchPolicy,
    pub terminal_key_policy: TerminalKeyPolicy,
    pub command_conflict_policy: CommandConflictPolicy,
    pub chord_mismatch_policy: ChordMismatchPolicy,
}

pub(crate) struct RuntimeKeyDispatchState {
    pub command_runtime: CommandShortcutRuntime,
    pub(crate) pending_command_prefix: Option<KeyEvent>,
}

impl RuntimeKeyDispatchState {
    pub(crate) fn new(registry: &CommandRegistry, conflict_policy: CommandConflictPolicy) -> Self {
        Self {
            command_runtime: CommandShortcutRuntime::new(registry, conflict_policy),
            pending_command_prefix: None,
        }
    }

    pub(crate) fn reset_command_chord(&mut self) {
        self.command_runtime.reset();
        self.pending_command_prefix = None;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuntimeKeyDispatchOutcome {
    pub handled: bool,
    pub quit: bool,
    pub dirty: bool,
    pub layout_dirty: bool,
}

pub(crate) enum FrameworkSideEffect {
    ToggleDevtools,
}

pub(crate) struct RuntimeKeyDispatchEnv<'a> {
    pub tree: &'a mut NodeTree,
    pub focused: &'a mut Option<NodeId>,
    pub focused_key: &'a mut Option<Key>,
    pub focused_tag: &'a mut Option<Tag>,
    pub key_ctx: &'a mut KeyCtx<'a>,
    pub keymap: &'a Keymap,
    pub keymap_runtime: &'a mut KeymapRuntime,
    pub command_registry: CommandRegistry,
    pub dispatch_state: &'a mut RuntimeKeyDispatchState,
    pub config: RuntimeKeyDispatchConfig,
    pub clipboard: &'a ClipboardService,
    pub clipboard_config: &'a ClipboardConfig,
    pub framework_effects: &'a mut Vec<FrameworkSideEffect>,
}

pub(crate) trait RuntimeOverlayDispatch {
    fn dismiss_overlay(&mut self) -> bool;
    fn focus_next(&mut self) -> bool;
    fn focus_prev(&mut self) -> bool;
    fn top_overlay_blocks_unfocused(&self) -> bool;
}

pub(crate) fn dispatch_runtime_key(
    env: &mut RuntimeKeyDispatchEnv<'_>,
    bubble: &mut impl FnMut(KeyEvent) -> BubbleKeyResult,
    overlay: &mut impl RuntimeOverlayDispatch,
    key: KeyEvent,
) -> RuntimeKeyDispatchOutcome {
    let focus = if env
        .focused
        .is_some_and(|id| env.tree.is_valid(id) && is_terminal(env.tree, id))
    {
        FocusKind::Terminal
    } else {
        FocusKind::Widget
    };

    if focus == FocusKind::Terminal {
        env.keymap_runtime.reset();
    }

    let key_policy = env.config.key_dispatch_policy;
    let terminal_policy = env.config.terminal_key_policy;
    let chord_mismatch_policy = env.config.chord_mismatch_policy;

    let mut ops = RuntimeDispatchOps {
        env,
        bubble,
        overlay,
    };
    let outcome = dispatch_key(
        DispatchRequest::new(key, focus)
            .key_policy(key_policy)
            .terminal_policy(terminal_policy)
            .chord_mismatch_policy(chord_mismatch_policy),
        &mut ops,
    );
    ops.finish(outcome)
}

struct RuntimeDispatchOps<'a, 'b> {
    env: &'a mut RuntimeKeyDispatchEnv<'b>,
    bubble: &'a mut dyn FnMut(KeyEvent) -> BubbleKeyResult,
    overlay: &'a mut dyn RuntimeOverlayDispatch,
}

impl RuntimeDispatchOps<'_, '_> {
    fn forward_terminal_key(&mut self, key: KeyEvent) -> bool {
        #[cfg(feature = "terminal")]
        if let Some(id) = self.env.focused.filter(|id| self.env.tree.is_valid(*id))
            && is_terminal(self.env.tree, id)
        {
            return forward_key(self.env.tree, id, key);
        }
        keyboard::dispatch_key(self.env.tree, *self.env.focused, key, self.env.key_ctx)
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

        if let Some(level) = self.env.key_ctx.dirty_override {
            result.dirty = true;
            result.layout_dirty = matches!(level, DirtyLevel::LayoutOnly | DirtyLevel::Full);
        }

        result
    }
}

impl DispatchOps for RuntimeDispatchOps<'_, '_> {
    fn continue_command_chord(&mut self, key: KeyEvent) -> CommandDispatchState {
        let was_pending = self.env.dispatch_state.command_runtime.is_pending();
        let registry = self.env.command_registry.clone();
        match self.env.dispatch_state.command_runtime.feed(key, &registry) {
            CommandShortcutResult::Pending
                if !was_pending
                    && self.env.config.key_dispatch_policy == KeyDispatchPolicy::WidgetFirst =>
            {
                self.env.dispatch_state.command_runtime.reset();
                self.env.dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
            CommandShortcutResult::Pending => {
                // Remember the first key that starts an accepted chord so a later
                // mismatch can replay it under `ForwardPrefixAndCurrent`.
                if !was_pending {
                    self.env.dispatch_state.pending_command_prefix = Some(key);
                }
                CommandDispatchState::Pending
            }
            CommandShortcutResult::Matched(_id) if !was_pending => {
                self.env.dispatch_state.command_runtime.reset();
                self.env.dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
            CommandShortcutResult::Matched(id) => {
                self.env.dispatch_state.pending_command_prefix = None;
                registry.execute(id.clone());
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
                self.env.dispatch_state.pending_command_prefix = None;
                CommandDispatchState::None
            }
        }
    }

    fn dispatch_widget(&mut self, key: KeyEvent) -> bool {
        if should_dispatch_focus_key_to_widget_first(self.env.tree, *self.env.focused, key)
            && keyboard::dispatch_key(self.env.tree, *self.env.focused, key, self.env.key_ctx)
        {
            return true;
        }

        keyboard::dispatch_key(self.env.tree, *self.env.focused, key, self.env.key_ctx)
    }

    fn dispatch_bubble(&mut self, key: KeyEvent) -> bool {
        if matches!(
            self.env.config.chord_mismatch_policy,
            ChordMismatchPolicy::ForwardPrefixAndCurrent
        ) && let Some(prefix) = self.env.dispatch_state.pending_command_prefix.take()
        {
            let prefix_bubble = (self.bubble)(prefix);
            if prefix_bubble.handled || prefix_bubble.dirty {
                self.env.key_ctx.dirty_override = Some(DirtyLevel::Full);
            }
        }
        (self.bubble)(key).handled
    }

    fn dispatch_command(&mut self, key: KeyEvent) -> bool {
        let registry = self.env.command_registry.clone();
        let matches =
            registry.matching_enabled_shortcuts(key, self.env.config.command_conflict_policy);
        if let Some(id) = matches.into_iter().next() {
            registry.execute(id);
            return true;
        }
        false
    }

    fn dispatch_framework(&mut self, key: KeyEvent) -> FrameworkDispatch {
        let runtime_match = match self.env.keymap_runtime.feed(key) {
            KeymapRuntimeResult::Pending => return FrameworkDispatch::Handled,
            KeymapRuntimeResult::Matched(binding) => Some(binding),
            KeymapRuntimeResult::None => None,
        };
        let action_from_chord = runtime_match.is_some_and(|binding| binding.is_chord);
        let matches = runtime_match
            .map(|binding| vec![binding.binding_match()])
            .unwrap_or_else(|| self.env.keymap.matches(key));

        if self.overlay.top_overlay_blocks_unfocused()
            && matches.iter().any(|binding| binding.action == Action::Quit)
        {
            return FrameworkDispatch::Quit;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::DismissOverlay)
            && self.overlay.dismiss_overlay()
        {
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::FocusNext)
        {
            if !action_from_chord
                && should_dispatch_focus_key_to_widget_first(self.env.tree, *self.env.focused, key)
                && keyboard::dispatch_key(self.env.tree, *self.env.focused, key, self.env.key_ctx)
            {
                return FrameworkDispatch::Handled;
            }
            if self.overlay.focus_next() {
                return FrameworkDispatch::Handled;
            }
            if self.env.config.focus_policy == FocusPolicy::Manual {
                return FrameworkDispatch::None;
            }
            focus::focus_next(
                self.env.tree,
                self.env.focused,
                self.env.focused_key,
                self.env.focused_tag,
                self.env.config.focus_policy,
            );
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::FocusPrev)
        {
            if !action_from_chord
                && should_dispatch_focus_key_to_widget_first(self.env.tree, *self.env.focused, key)
                && keyboard::dispatch_key(self.env.tree, *self.env.focused, key, self.env.key_ctx)
            {
                return FrameworkDispatch::Handled;
            }
            if self.overlay.focus_prev() {
                return FrameworkDispatch::Handled;
            }
            if self.env.config.focus_policy == FocusPolicy::Manual {
                return FrameworkDispatch::None;
            }
            focus::focus_prev(
                self.env.tree,
                self.env.focused,
                self.env.focused_key,
                self.env.focused_tag,
                self.env.config.focus_policy,
            );
            return FrameworkDispatch::Handled;
        }

        if matches
            .iter()
            .any(|binding| binding.action == Action::ToggleDevTools)
        {
            self.env
                .framework_effects
                .push(FrameworkSideEffect::ToggleDevtools);
            return FrameworkDispatch::Handled;
        }

        if matches.iter().any(|binding| binding.action == Action::Quit) {
            if action_from_chord || !self.overlay.top_overlay_blocks_unfocused() {
                return FrameworkDispatch::Quit;
            }
            let bubble = (self.bubble)(key);
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
            let Some(id) = self.env.focused.filter(|id| self.env.tree.is_valid(*id)) else {
                return TerminalPreflightDispatch::NotApplicable;
            };
            match preflight_key(
                self.env.tree,
                id,
                key,
                self.env.clipboard,
                self.env.clipboard_config,
            ) {
                TerminalPreflightResult::Consumed => TerminalPreflightDispatch::Consumed,
                TerminalPreflightResult::NotConsumed => TerminalPreflightDispatch::NotConsumed,
                TerminalPreflightResult::NotApplicable => TerminalPreflightDispatch::NotApplicable,
            }
        }
    }

    fn dispatch_terminal(&mut self, key: KeyEvent) -> bool {
        if matches!(
            self.env.config.chord_mismatch_policy,
            ChordMismatchPolicy::ForwardPrefixAndCurrent
        ) && let Some(prefix) = self.env.dispatch_state.pending_command_prefix.take()
        {
            self.forward_terminal_key(prefix);
        }
        self.forward_terminal_key(key)
    }

    fn dispatch_ambient_scroll(&mut self, key: KeyEvent) -> bool {
        keyboard::dispatch_ambient_page_scroll(self.env.tree, key)
    }
}

fn is_terminal(tree: &NodeTree, id: NodeId) -> bool {
    #[cfg(feature = "terminal")]
    {
        matches!(tree.node(id).kind, NodeKind::Terminal(_))
    }
    #[cfg(not(feature = "terminal"))]
    {
        let _ = (tree, id);
        false
    }
}

fn should_dispatch_focus_key_to_widget_first(
    tree: &NodeTree,
    focused: Option<NodeId>,
    key: KeyEvent,
) -> bool {
    if !matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        return false;
    }

    let Some(id) = focused else {
        return false;
    };

    tree.is_valid(id) && matches!(tree.node(id).kind, NodeKind::TextArea(_))
}

pub(crate) fn should_dispatch_text_area_tab_first(
    tree: &NodeTree,
    focused: Option<NodeId>,
    key: KeyEvent,
) -> bool {
    should_dispatch_focus_key_to_widget_first(tree, focused, key)
}

pub(crate) fn dispatch_widget_with_policy(
    tree: &mut NodeTree,
    focused: Option<NodeId>,
    key: KeyEvent,
    key_ctx: &mut KeyCtx<'_>,
    _policy: KeyDispatchPolicy,
) -> bool {
    if should_dispatch_focus_key_to_widget_first(tree, focused, key)
        && keyboard::dispatch_key(tree, focused, key, key_ctx)
    {
        return true;
    }

    keyboard::dispatch_key(tree, focused, key, key_ctx)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn make_key_ctx<'a>(
    read_only_selection: Option<&'a HashMap<NodeId, (usize, Option<usize>)>>,
    input_history: &'a mut HashMap<NodeId, TextInput>,
    textarea_history: &'a mut HashMap<NodeId, TextEditor>,
    text_area_vim_state: &'a mut HashMap<NodeId, TextAreaVimState>,
    hex_history: &'a mut HashMap<NodeId, HexHistory>,
    hex_pending_edit: &'a mut HashMap<NodeId, HexPendingEdit>,
    keymap: &'a Keymap,
    text_area_newline_binding: TextAreaNewlineBinding,
    clipboard: &'a ClipboardService,
    clipboard_config: &'a ClipboardConfig,
    copy_feedback: &'a mut CopyFeedbackState,
) -> KeyCtx<'a> {
    KeyCtx {
        read_only_selection,
        input_history,
        textarea_history,
        text_area_vim_state,
        hex_history,
        hex_pending_edit,
        keymap,
        text_area_newline_binding,
        clipboard,
        clipboard_config,
        copy_feedback,
        dirty_override: None,
    }
}

pub(crate) fn selection_clipboard_shortcut(
    tree: &mut NodeTree,
    key_ctx: &mut KeyCtx<'_>,
    key: KeyEvent,
) -> bool {
    keyboard::dispatch_selection_clipboard_shortcut(tree, key, key_ctx)
}

pub(crate) fn devtools_toggle_cell() -> Rc<Cell<bool>> {
    Rc::new(Cell::new(false))
}
