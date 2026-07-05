//! Per-widget event handler functions.
//!
//! This module breaks up the monolithic `dispatch_key` and `handle_scroll_wheel_n`
//! functions into per-widget handler modules, each responsible for a single widget
//! type's keyboard and scroll behaviour.

use std::collections::HashMap;

use crate::app::context::TextAreaNewlineBinding;
use crate::app::copy_feedback::CopyFeedbackState;
use crate::app::input::hex_history::HexHistory;
use crate::app::input::keymap::Keymap;
use crate::app::input::text_area_vim::TextAreaVimState;
use crate::app::interaction_state::DirtyLevel;
use crate::app::interaction_state::HexPendingEdit;
use crate::callback::KeyHandler;
use crate::clipboard::{ClipboardConfig, ClipboardService};
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind};
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;

pub mod button;
pub mod checkbox;
pub mod document_view;
pub mod graph;
pub mod hex_area;
pub mod input_widget;
pub mod list_table;
pub mod pan_view;
pub mod scroll_view;
pub mod tabs;
#[cfg(feature = "terminal")]
pub mod terminal;
pub mod text_area;

// ── Context structs ─────────────────────────────────────────────────────────

/// Shared services for keyboard dispatch, bundling the many parameters that were
/// previously passed individually to `dispatch_key`.
pub(crate) struct KeyCtx<'a> {
    pub read_only_selection: Option<&'a HashMap<NodeId, (usize, Option<usize>)>>,
    pub input_history: &'a mut HashMap<NodeId, TextInput>,
    pub textarea_history: &'a mut HashMap<NodeId, TextEditor>,
    pub text_area_vim_state: &'a mut HashMap<NodeId, TextAreaVimState>,
    pub hex_history: &'a mut HashMap<NodeId, HexHistory>,
    pub hex_pending_edit: &'a mut HashMap<NodeId, HexPendingEdit>,
    pub keymap: &'a Keymap,
    pub text_area_newline_binding: TextAreaNewlineBinding,
    pub clipboard: &'a ClipboardService,
    pub clipboard_config: &'a ClipboardConfig,
    pub copy_feedback: &'a mut CopyFeedbackState,
    /// When a handler consumes a key without mutating layout/component state,
    /// it can override the event loop's generic handled-key dirty level.
    pub dirty_override: Option<DirtyLevel>,
}

impl KeyCtx<'_> {
    pub(crate) fn record_copy_feedback_dispatch(
        &mut self,
        dispatch: crate::app::copy_feedback::CopyFeedbackDispatch,
    ) -> bool {
        if dispatch.dirty_override.is_some() {
            self.dirty_override = dispatch.dirty_override;
        }
        dispatch.handled
    }
}

pub(crate) fn handle_key_interceptor(interceptor: Option<&KeyHandler>, key: KeyEvent) -> bool {
    interceptor.is_some_and(|interceptor| interceptor.handle(key))
}

// ── Dispatch tags ───────────────────────────────────────────────────────────

/// Lightweight discriminant for interactive `NodeKind` variants so that
/// `dispatch_key` can release the immutable tree borrow before calling into
/// per-widget handler functions that require `&mut NodeTree`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InteractiveTag {
    Button,
    Checkbox,
    Graph,
    Input,
    HexArea,
    List,
    Table,
    TextArea,
    #[cfg(feature = "terminal")]
    Terminal,
    PanView,
    ScrollView,
    Tabs,
    DraggableTabBar,
    DocumentView,
    /// Not an interactive widget - dispatch should return `false`.
    NonInteractive,
}

/// Classify a `NodeKind` into its `InteractiveTag`.
pub(crate) fn classify_interactive(kind: &NodeKind) -> InteractiveTag {
    match kind {
        NodeKind::Button(_) => InteractiveTag::Button,
        NodeKind::Checkbox(_) => InteractiveTag::Checkbox,
        NodeKind::Graph(_) => InteractiveTag::Graph,
        NodeKind::Input(_) => InteractiveTag::Input,
        NodeKind::HexArea(_) => InteractiveTag::HexArea,
        NodeKind::List(_) => InteractiveTag::List,
        NodeKind::Table(_) => InteractiveTag::Table,
        NodeKind::TextArea(_) => InteractiveTag::TextArea,
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(_) => InteractiveTag::Terminal,
        NodeKind::PanView(_) => InteractiveTag::PanView,
        NodeKind::ScrollView(_) => InteractiveTag::ScrollView,
        NodeKind::Tabs(_) => InteractiveTag::Tabs,
        NodeKind::DraggableTabBar(_) => InteractiveTag::DraggableTabBar,
        NodeKind::DocumentView(_) => InteractiveTag::DocumentView,
        _ => InteractiveTag::NonInteractive,
    }
}

/// Lightweight discriminant for `NodeKind` variants that handle scroll-wheel
/// events, used in `handle_scroll_wheel_n` to avoid borrow conflicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScrollableTag {
    List,
    Table,
    ScrollView,
    TextArea,
    HexArea,
    #[cfg(feature = "terminal")]
    Terminal,
    DraggableTabBar,
    DocumentView,
    /// Not scrollable - bubble to parent.
    NonScrollable,
}

/// Classify a `NodeKind` into its `ScrollableTag`.
pub(crate) fn classify_scrollable(kind: &NodeKind) -> ScrollableTag {
    match kind {
        NodeKind::List(_) => ScrollableTag::List,
        NodeKind::Table(_) => ScrollableTag::Table,
        NodeKind::ScrollView(_) => ScrollableTag::ScrollView,
        NodeKind::TextArea(_) => ScrollableTag::TextArea,
        NodeKind::HexArea(_) => ScrollableTag::HexArea,
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(_) => ScrollableTag::Terminal,
        NodeKind::DraggableTabBar(_) => ScrollableTag::DraggableTabBar,
        NodeKind::DocumentView(_) => ScrollableTag::DocumentView,
        _ => ScrollableTag::NonScrollable,
    }
}
