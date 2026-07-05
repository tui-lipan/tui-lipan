//! Terminal output view helpers.

mod buffer;
mod events;
mod layout;
mod mod_private;
mod node;
mod pty;
mod reconcile;
mod screen;

pub use buffer::TerminalBuffer;
pub use events::{
    MouseEncoding, MouseMode, MouseModeState, TerminalInputEvent, TerminalInputKind,
    TerminalSelection, TerminalSelectionEvent, focus_sequences, key_event_to_bytes,
    mouse_event_to_bytes, paste_sequences, terminal_selection_text, wrap_bracketed_paste,
};
pub use mod_private::Terminal;
pub use pty::{TerminalPty, TerminalPtyConfig, TerminalPtyError, TerminalPtyEvent};
pub use screen::{TerminalColorPalette, TerminalRenderSnapshot, TerminalScreen, TerminalViewport};

pub(crate) use layout::{measure_terminal, terminal_content_layout, terminal_mouse_content_rect};
pub(crate) use node::TerminalNode;
pub(crate) use reconcile::reconcile_terminal;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::style::{
    BorderStyle, Length, Padding, ScrollbarConfig, ScrollbarVariant, Span, Style, StyleSlot,
};
use crate::widgets::ScrollEvent;
use std::sync::Arc;

impl Default for Terminal {
    fn default() -> Self {
        Self {
            content: Arc::from(""),
            cursor_row: 0,
            cursor_col: 0,
            show_cursor: true,
            color_lines: None,
            color_cache_key: 0,
            scrollback_offset: 0,
            total_scrollback_rows: 0,
            mouse_mode: MouseModeState::default(),
            selection: None,
            selection_controlled: false,
            selection_style: StyleSlot::Inherit,
            on_selection: None,
            on_resize: None,
            on_mouse_forward: None,
            scroll_wheel: true,
            on_scroll: None,
            on_scroll_to: None,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            focus_content_style: Style::default(),
            border: false,
            border_style: BorderStyle::default(),
            padding: Padding::default(),
            scrollbar: true,
            scrollbar_variant: ScrollbarVariant::default(),
            scrollbar_gap: 0,
            scrollbar_thumb: None,
            scrollbar_thumb_style: None,
            scrollbar_thumb_focus_style: None,
            scrollbar_track_style: None,
            h_scrollbar: true,
            h_scrollbar_variant: ScrollbarVariant::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            focusable: true,
            on_key: None,
            on_input: None,
        }
    }
}

impl Terminal {
    /// Create an empty terminal view.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace visible content.
    pub fn content(mut self, content: impl Into<Arc<str>>) -> Self {
        self.content = content.into();
        self
    }

    /// Set cursor byte position in content.
    pub fn cursor(mut self, cursor: usize) -> Self {
        let (row, col) = byte_to_row_col(self.content.as_ref(), cursor);
        self.cursor_row = row;
        self.cursor_col = col;
        self
    }

    /// Set cursor row/column in the visible viewport.
    pub fn cursor_position(mut self, row: u16, col: u16) -> Self {
        self.cursor_row = row;
        self.cursor_col = col;
        self
    }

    /// Toggle cursor rendering.
    pub fn show_cursor(mut self, show_cursor: bool) -> Self {
        self.show_cursor = show_cursor;
        self
    }

    /// Set precomputed colored lines (must match `content` line lengths).
    pub fn color_lines(mut self, color_lines: Arc<[Vec<Span>]>, cache_key: u64) -> Self {
        self.color_lines = Some(color_lines);
        self.color_cache_key = cache_key;
        self
    }

    /// Apply a full terminal render snapshot.
    pub fn snapshot(mut self, snapshot: TerminalRenderSnapshot) -> Self {
        self.content = snapshot.text;
        self.cursor_row = snapshot.cursor_row;
        self.cursor_col = snapshot.cursor_col;
        self.show_cursor = snapshot.cursor_visible;
        self.color_lines = Some(snapshot.color_lines);
        self.color_cache_key = snapshot.sequence;
        self.scrollback_offset = snapshot.scrollback_offset;
        self.total_scrollback_rows = snapshot.total_scrollback_rows;
        self.mouse_mode = snapshot.mouse_mode;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set focus chrome style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's focus style with additional fields.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set focused content text style.
    pub fn focus_content_style(mut self, style: Style) -> Self {
        self.focus_content_style = style;
        self
    }

    /// Toggle border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set inner padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Toggle vertical scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_variant = config.variant;
        self.scrollbar_gap = config.gap;
        self.scrollbar_thumb = config.thumb;
        self.scrollbar_thumb_style = config.thumb_style;
        self.scrollbar_thumb_focus_style = config.thumb_focus_style;
        self.scrollbar_track_style = config.track_style;
        self
    }

    /// Toggle horizontal scrollbar.
    pub fn h_scrollbar(mut self, h_scrollbar: bool) -> Self {
        self.h_scrollbar = h_scrollbar;
        self
    }

    /// Set horizontal scrollbar style.
    pub fn h_scrollbar_variant(mut self, style: ScrollbarVariant) -> Self {
        self.h_scrollbar_variant = style;
        self
    }

    /// Toggle mouse wheel scrolling through scrollback history.
    pub fn scroll_wheel(mut self, scroll_wheel: bool) -> Self {
        self.scroll_wheel = scroll_wheel;
        self
    }

    /// Set callback for scroll events with full metrics.
    pub fn on_scroll(mut self, cb: Callback<ScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Set callback emitting the new scrollback offset on scroll.
    ///
    /// The offset is in scrollback rows: 0 = live (bottom), positive
    /// values = scrolled into history. Use this to call
    /// `TerminalScreen::set_scrollback(offset)` in your component.
    pub fn on_scroll_to(mut self, cb: Callback<usize>) -> Self {
        self.on_scroll_to = Some(cb);
        self
    }

    /// Set selection highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's selection style with additional fields.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit selection style from the active theme.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set current selection.
    pub fn selection(mut self, selection: Option<TerminalSelection>) -> Self {
        self.selection = selection;
        self.selection_controlled = true;
        self
    }

    /// Set selection change callback.
    pub fn on_selection(mut self, cb: Callback<TerminalSelectionEvent>) -> Self {
        self.on_selection = Some(cb);
        self
    }

    /// Set callback fired when the terminal viewport size changes.
    pub fn on_resize(mut self, cb: Callback<TerminalViewport>) -> Self {
        self.on_resize = Some(cb);
        self
    }

    /// Set callback to forward mouse bytes to PTY.
    pub fn on_mouse_forward(mut self, cb: Callback<Vec<u8>>) -> Self {
        self.on_mouse_forward = Some(cb);
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Control focusability.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set raw key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set callback for terminal-encoded key input.
    pub fn on_input(mut self, cb: Callback<TerminalInputEvent>) -> Self {
        self.on_input = Some(cb);
        self
    }
}

impl From<Terminal> for Element {
    fn from(mut terminal: Terminal) -> Self {
        let on_input = terminal.on_input.clone();
        let fallback_on_key = terminal.on_key.clone();
        terminal.on_key = if on_input.is_some() || fallback_on_key.is_some() {
            Some(KeyHandler::new(move |key| {
                let mut handled = false;

                if let Some(on_input) = on_input.as_ref()
                    && let Some(bytes) = key_event_to_bytes(key)
                {
                    on_input.emit(TerminalInputEvent {
                        kind: TerminalInputKind::Key,
                        key: Some(key),
                        bytes: bytes.into(),
                    });
                    handled = true;
                }

                if let Some(handler) = fallback_on_key.as_ref() {
                    handled = handler.handle(key) || handled;
                }

                handled
            }))
        } else {
            None
        };

        Element::new(ElementKind::Terminal(terminal))
    }
}

fn byte_to_row_col(value: &str, cursor: usize) -> (u16, u16) {
    let cursor = cursor.min(value.len());
    let mut row = 0u16;
    let mut col = 0u16;
    let mut seen = 0usize;

    for ch in value.chars() {
        if seen >= cursor {
            break;
        }
        seen = seen.saturating_add(ch.len_utf8());
        if ch == '\n' {
            row = row.saturating_add(1);
            col = 0;
        } else {
            col = col.saturating_add(1);
        }
    }

    (row, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};

    #[test]
    fn ctrl_mapping_works() {
        let key = KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };
        assert_eq!(key_event_to_bytes(key), Some(vec![3]));
    }

    #[test]
    fn alt_prefixes_escape() {
        let key = KeyEvent {
            code: KeyCode::Char('x'),
            mods: KeyMods {
                alt: true,
                ..KeyMods::default()
            },
        };
        assert_eq!(key_event_to_bytes(key), Some(vec![0x1b, b'x']));
    }

    #[test]
    fn terminal_buffer_trims_lines() {
        let mut buffer = TerminalBuffer::new(2);
        buffer.push_text("a\nb\nc\n");
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.as_ref(), "b\nc");
    }

    #[test]
    fn terminal_screen_applies_vt_sequences() {
        let mut screen = TerminalScreen::new(4, 20, 128);
        screen.process_bytes(b"\x1b[31mhello\x1b[0m\nworld");
        let snapshot = screen.snapshot();
        let mut lines = snapshot.lines().map(str::trim);
        assert_eq!(lines.next(), Some("hello"));
        assert_eq!(lines.next(), Some("world"));
    }

    #[test]
    fn mouse_event_to_bytes_sgr_encodes_coordinates() {
        let event = MouseEvent {
            x: 2,
            y: 3,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        };
        use super::events::MouseEncoding;

        let bytes = mouse_event_to_bytes(event, MouseEncoding::Sgr, (0, 0)).expect("mouse bytes");
        assert_eq!(String::from_utf8(bytes).unwrap(), "\u{1b}[<0;3;4M");
    }

    #[test]
    fn mouse_event_to_bytes_sgr_encodes_plain_motion() {
        // Any-event tracking (1003) reports motion without a pressed button as
        // code 35 (3 "no button" + 32 motion flag). Dropping these leaves apps
        // in the pane without hover positions.
        let event = MouseEvent {
            x: 26,
            y: 5,
            kind: MouseKind::Moved,
            mods: KeyMods::default(),
        };
        use super::events::MouseEncoding;

        let bytes = mouse_event_to_bytes(event, MouseEncoding::Sgr, (0, 0)).expect("mouse bytes");
        assert_eq!(String::from_utf8(bytes).unwrap(), "\u{1b}[<35;27;6M");
    }

    #[test]
    fn grid_selection_extracts_text() {
        use crate::utils::{GridPos, GridSelection};
        let selection = GridSelection {
            anchor: GridPos { row: 0, col: 1 },
            cursor: GridPos { row: 1, col: 2 },
        };
        let lines = vec!["abcd", "efgh", "ijkl"];
        assert_eq!(selection.extract_text(&lines), "bcd\nef");
    }
}
