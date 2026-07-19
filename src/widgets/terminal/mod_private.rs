use crate::callback::{Callback, KeyHandler};
use crate::style::{
    BorderStyle, CaretShape, Length, Padding, ScrollbarVariant, Span, Style, StyleSlot,
};
use crate::widgets::ScrollEvent;
use std::sync::Arc;

use super::events::{
    MouseModeState, TerminalInputEvent, TerminalKeyModes, TerminalSelection, TerminalSelectionEvent,
};
use super::screen::TerminalViewport;

/// Terminal-like widget backed by a read-only `TextArea`.
#[derive(Clone)]
pub struct Terminal {
    pub(crate) content: Arc<str>,
    pub(crate) cursor_row: u16,
    pub(crate) cursor_col: u16,
    pub(crate) show_cursor: bool,
    pub(crate) cursor_shape: CaretShape,
    pub(crate) cursor_blinking: bool,
    pub(crate) color_lines: Option<Arc<[Vec<Span>]>>,
    pub(crate) color_cache_key: u64,
    pub(crate) scrollback_offset: usize,
    pub(crate) total_scrollback_rows: usize,
    pub(crate) mouse_mode: MouseModeState,
    pub(crate) key_modes: TerminalKeyModes,
    pub(crate) selection: Option<TerminalSelection>,
    pub(crate) selection_controlled: bool,
    pub(crate) selection_style: StyleSlot,
    pub(crate) on_selection: Option<Callback<TerminalSelectionEvent>>,
    pub(crate) on_resize: Option<Callback<TerminalViewport>>,
    pub(crate) on_mouse_forward: Option<Callback<Vec<u8>>>,
    pub(crate) scroll_wheel: bool,
    pub(crate) on_scroll: Option<Callback<ScrollEvent>>,
    pub(crate) on_scroll_to: Option<Callback<usize>>,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) focus_style: StyleSlot,
    pub(crate) focus_content_style: Style,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) scrollbar: bool,
    pub(crate) scrollbar_variant: ScrollbarVariant,
    pub(crate) scrollbar_gap: u16,
    pub(crate) scrollbar_thumb: Option<char>,
    pub(crate) scrollbar_thumb_style: Option<Style>,
    pub(crate) scrollbar_thumb_focus_style: Option<Style>,
    pub(crate) scrollbar_track_style: Option<Style>,
    pub(crate) h_scrollbar: bool,
    pub(crate) h_scrollbar_variant: ScrollbarVariant,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) on_input: Option<Callback<TerminalInputEvent>>,
}
