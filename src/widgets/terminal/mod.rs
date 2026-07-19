//! Terminal output view helpers.

mod buffer;
mod events;
mod layout;
mod mod_private;
mod node;
mod osc;
mod pty;
mod reconcile;
mod screen;

pub use buffer::TerminalBuffer;
pub use events::{
    KittyKeyboardFlags, MouseEncoding, MouseMode, MouseModeState, TerminalInputEvent,
    TerminalInputKind, TerminalKeyModes, TerminalSelection, TerminalSelectionEvent, encode_paste,
    focus_sequences, key_event_to_bytes, mouse_event_to_bytes, paste_sequences,
    terminal_selection_text,
};
pub use mod_private::Terminal;
pub use osc::{
    TerminalCommandPhase, TerminalSemanticEvent, TerminalSemanticState, TerminalWorkingDirectory,
    TerminalWorkingDirectorySource,
};
#[cfg(unix)]
pub use pty::TerminalPtyHandoff;
pub use pty::{TerminalPty, TerminalPtyConfig, TerminalPtyError, TerminalPtyEvent};
pub use screen::{TerminalColorPalette, TerminalRenderSnapshot, TerminalScreen, TerminalViewport};

pub(crate) use layout::{measure_terminal, terminal_content_layout, terminal_mouse_content_rect};
pub(crate) use node::TerminalNode;
pub(crate) use reconcile::reconcile_terminal;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::style::{
    BorderStyle, CaretShape, Length, Padding, ScrollbarConfig, ScrollbarVariant, Span, Style,
    StyleSlot,
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
            cursor_shape: CaretShape::Block,
            cursor_blinking: true,
            color_lines: None,
            color_cache_key: 0,
            scrollback_offset: 0,
            total_scrollback_rows: 0,
            mouse_mode: MouseModeState::default(),
            key_modes: TerminalKeyModes::default(),
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
            tab_stop: true,
            on_focus: None,
            on_blur: None,
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

    /// Set the cursor shape (block, bar, or underline).
    pub fn cursor_shape(mut self, shape: CaretShape) -> Self {
        self.cursor_shape = shape;
        self
    }

    /// Set whether the cursor should blink.
    pub fn cursor_blinking(mut self, blinking: bool) -> Self {
        self.cursor_blinking = blinking;
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
        self.cursor_shape = snapshot.cursor_shape;
        self.cursor_blinking = snapshot.cursor_blinking;
        self.color_lines = Some(snapshot.color_lines);
        self.color_cache_key = snapshot.sequence;
        self.scrollback_offset = snapshot.scrollback_offset;
        self.total_scrollback_rows = snapshot.total_scrollback_rows;
        self.mouse_mode = snapshot.mouse_mode;
        self.key_modes = snapshot.key_modes;
        self
    }

    /// Set the child's input-affecting DEC modes directly.
    ///
    /// [`snapshot`](Self::snapshot) already carries these. Use this only when driving the widget
    /// from something other than a `TerminalRenderSnapshot`.
    pub fn key_modes(mut self, key_modes: TerminalKeyModes) -> Self {
        self.key_modes = key_modes;
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

    /// Control whether the terminal participates in tab focus traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the terminal gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the terminal loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
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
        let key_modes = terminal.key_modes;
        terminal.on_key = if on_input.is_some() || fallback_on_key.is_some() {
            Some(KeyHandler::new(move |key| {
                let mut handled = false;

                if let Some(on_input) = on_input.as_ref()
                    && let Some(bytes) = key_event_to_bytes(key, key_modes)
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

    /// Encode a chord for a child that negotiated nothing (a shell, an editor).
    fn enc(code: KeyCode, mods: KeyMods) -> Option<Vec<u8>> {
        key_event_to_bytes(KeyEvent { code, mods }, TerminalKeyModes::default())
    }

    /// Encode a chord for a child that pushed the Kitty protocol's disambiguate flag, as
    /// tui-lipan's own backend does on startup.
    fn kitty(code: KeyCode, mods: KeyMods) -> Option<Vec<u8>> {
        let modes = TerminalKeyModes {
            kitty_keyboard: KittyKeyboardFlags {
                disambiguate_escape_codes: true,
                ..KittyKeyboardFlags::default()
            },
            ..TerminalKeyModes::default()
        };
        key_event_to_bytes(KeyEvent { code, mods }, modes)
    }

    const CTRL_SHIFT: KeyMods = KeyMods {
        ctrl: true,
        shift: true,
        alt: false,
        super_key: false,
    };
    const CTRL_ALT: KeyMods = KeyMods {
        ctrl: true,
        alt: true,
        shift: false,
        super_key: false,
    };
    const ALT_SHIFT: KeyMods = KeyMods {
        alt: true,
        shift: true,
        ctrl: false,
        super_key: false,
    };

    #[test]
    fn ctrl_mapping_works() {
        assert_eq!(enc(KeyCode::Char('c'), KeyMods::CTRL), Some(vec![3]));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(
            enc(KeyCode::Char('x'), KeyMods::ALT),
            Some(vec![0x1b, b'x'])
        );
    }

    #[test]
    fn ctrl_arrows_encode_xterm_modifier_param() {
        // Ctrl+Left/Right drive word-wise motion in TUIs and must not collapse to a bare arrow.
        for (code, expected) in [
            (KeyCode::Left, "\x1b[1;5D"),
            (KeyCode::Right, "\x1b[1;5C"),
            (KeyCode::Up, "\x1b[1;5A"),
            (KeyCode::Down, "\x1b[1;5B"),
            (KeyCode::Home, "\x1b[1;5H"),
            (KeyCode::End, "\x1b[1;5F"),
        ] {
            assert_eq!(
                enc(code, KeyMods::CTRL),
                Some(expected.as_bytes().to_vec()),
                "ctrl {code:?}"
            );
        }
    }

    #[test]
    fn shift_and_combined_modifiers_encode_params() {
        assert_eq!(
            enc(KeyCode::Left, KeyMods::SHIFT),
            Some(b"\x1b[1;2D".to_vec())
        );
        // Ctrl+Shift => param 6; Ctrl+Alt => param 7 (no separate ESC prefix).
        assert_eq!(enc(KeyCode::Right, CTRL_SHIFT), Some(b"\x1b[1;6C".to_vec()));
        assert_eq!(enc(KeyCode::Up, CTRL_ALT), Some(b"\x1b[1;7A".to_vec()));
        // Alt+Shift => param 4. Alt only suppresses the parameterized form when it stands alone,
        // so this takes the CSI path rather than the ESC-prefixed one.
        assert_eq!(enc(KeyCode::Left, ALT_SHIFT), Some(b"\x1b[1;4D".to_vec()));
    }

    #[test]
    fn shift_only_emulator_reserved_keys_keep_plain_form() {
        // Shift+Insert pastes and Shift+PageUp/PageDown page the scrollback by convention. The
        // widget forwards them instead of consuming them, and children do not understand the
        // parameterized form, so the plain bytes must survive or the key becomes a no-op.
        for (code, expected) in [
            (KeyCode::Insert, "\x1b[2~"),
            (KeyCode::PageUp, "\x1b[5~"),
            (KeyCode::PageDown, "\x1b[6~"),
        ] {
            assert_eq!(
                enc(code, KeyMods::SHIFT),
                Some(expected.as_bytes().to_vec()),
                "shift {code:?}"
            );
        }

        // Delete is not emulator-reserved, and adding Ctrl lifts the exemption entirely.
        assert_eq!(
            enc(KeyCode::Delete, KeyMods::SHIFT),
            Some(b"\x1b[3;2~".to_vec())
        );
        assert_eq!(
            enc(KeyCode::PageUp, CTRL_SHIFT),
            Some(b"\x1b[5;6~".to_vec())
        );
    }

    #[test]
    fn ctrl_tilde_keys_encode_params() {
        assert_eq!(
            enc(KeyCode::Delete, KeyMods::CTRL),
            Some(b"\x1b[3;5~".to_vec())
        );
        assert_eq!(
            enc(KeyCode::PageUp, KeyMods::CTRL),
            Some(b"\x1b[5;5~".to_vec())
        );
        // F5 uses param 15 in the tilde scheme; Shift keeps that number.
        assert_eq!(
            enc(KeyCode::F(5), KeyMods::SHIFT),
            Some(b"\x1b[15;2~".to_vec())
        );
    }

    #[test]
    fn high_function_keys_encode_through_f20() {
        // The tilde scheme skips 16, 22, 27 and 30, so F15 is 28 rather than 27.
        assert_eq!(
            enc(KeyCode::F(13), KeyMods::NONE),
            Some(b"\x1b[25~".to_vec())
        );
        assert_eq!(
            enc(KeyCode::F(15), KeyMods::NONE),
            Some(b"\x1b[28~".to_vec())
        );
        assert_eq!(
            enc(KeyCode::F(20), KeyMods::CTRL),
            Some(b"\x1b[34;5~".to_vec())
        );
        // Past F20 there is no sequence to send, so the key is left for the app.
        assert_eq!(enc(KeyCode::F(21), KeyMods::NONE), None);
    }

    #[test]
    fn ctrl_backspace_sends_backward_kill_word() {
        // Ctrl+Backspace has no native PTY encoding; emit ESC DEL (readline backward-kill-word,
        // the same bytes as Alt+Backspace) so it deletes the previous word, not one character.
        assert_eq!(
            enc(KeyCode::Backspace, KeyMods::CTRL),
            Some(vec![0x1b, 0x7f])
        );
        // Plain Backspace stays a bare DEL.
        assert_eq!(enc(KeyCode::Backspace, KeyMods::NONE), Some(vec![0x7f]));
        // Alt+Backspace keeps its historical ESC DEL form (the same word-delete chord).
        assert_eq!(
            enc(KeyCode::Backspace, KeyMods::ALT),
            Some(vec![0x1b, 0x7f])
        );
    }

    #[test]
    fn legacy_children_keep_legacy_enter_tab_and_lose_ctrl_digits() {
        // A child that never negotiated the Kitty protocol gets exactly the bytes it did before.
        // Ctrl+Enter is indistinguishable from Enter, and Ctrl+1 has no encoding to send at all --
        // inventing one (modifyOtherKeys) would hand crossterm children a sequence their parser
        // rejects and silently discards, which is strictly worse than dropping the key here.
        assert_eq!(enc(KeyCode::Enter, KeyMods::NONE), Some(vec![b'\r']));
        assert_eq!(enc(KeyCode::Enter, KeyMods::CTRL), Some(vec![b'\r']));
        assert_eq!(enc(KeyCode::Enter, CTRL_SHIFT), Some(vec![b'\r']));
        assert_eq!(enc(KeyCode::Tab, KeyMods::NONE), Some(vec![b'\t']));
        assert_eq!(enc(KeyCode::Tab, KeyMods::CTRL), Some(vec![b'\t']));
        assert_eq!(
            enc(KeyCode::BackTab, KeyMods::SHIFT),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(enc(KeyCode::Esc, KeyMods::NONE), Some(vec![0x1b]));
        // Alt alone keeps the ESC prefix, matching the arrows.
        assert_eq!(enc(KeyCode::Enter, KeyMods::ALT), Some(vec![0x1b, b'\r']));
        assert_eq!(enc(KeyCode::Char('1'), KeyMods::CTRL), None);
    }

    #[test]
    fn kitty_children_get_csi_u_for_chords_with_no_legacy_encoding() {
        // tui-lipan's own backend pushes DISAMBIGUATE_ESCAPE_CODES on startup, so a tui-lipan app
        // running inside a Terminal widget lands here. Ctrl+1..Ctrl+9 are the motivating case:
        // they have no legacy bytes, so before the protocol they simply never reached the child.
        for (ch, expected) in [
            ('1', "\x1b[49;5u"),
            ('2', "\x1b[50;5u"),
            ('9', "\x1b[57;5u"),
        ] {
            assert_eq!(
                kitty(KeyCode::Char(ch), KeyMods::CTRL),
                Some(expected.as_bytes().to_vec())
            );
        }

        // Enter, Tab and Backspace become distinguishable from their unmodified selves.
        assert_eq!(
            kitty(KeyCode::Enter, KeyMods::CTRL),
            Some(b"\x1b[13;5u".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Enter, KeyMods::SHIFT),
            Some(b"\x1b[13;2u".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Tab, KeyMods::CTRL),
            Some(b"\x1b[9;5u".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Backspace, KeyMods::CTRL),
            Some(b"\x1b[127;5u".to_vec())
        );
        // BackTab is Shift+Tab; the shift belongs in the parameter.
        assert_eq!(
            kitty(KeyCode::BackTab, KeyMods::NONE),
            Some(b"\x1b[9;2u".to_vec())
        );
        // A lone Esc is escaped -- that is what "disambiguate escape codes" buys.
        assert_eq!(
            kitty(KeyCode::Esc, KeyMods::NONE),
            Some(b"\x1b[27u".to_vec())
        );

        // The codepoint is the key as engraved, so Ctrl+Shift+C reports `c` with shift in the
        // parameter and is finally distinct from Ctrl+C.
        assert_eq!(
            kitty(KeyCode::Char('c'), KeyMods::CTRL),
            Some(b"\x1b[99;5u".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Char('C'), CTRL_SHIFT),
            Some(b"\x1b[99;6u".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Char('x'), KeyMods::ALT),
            Some(b"\x1b[120;3u".to_vec())
        );
    }

    #[test]
    fn kitty_children_keep_legacy_bytes_for_text_and_functional_keys() {
        // Text still arrives as text: Shift alone does not promote a key to the escape form.
        assert_eq!(
            kitty(KeyCode::Char('a'), KeyMods::NONE),
            Some(b"a".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Char('A'), KeyMods::SHIFT),
            Some(b"A".to_vec())
        );
        assert_eq!(kitty(KeyCode::Enter, KeyMods::NONE), Some(vec![b'\r']));
        assert_eq!(kitty(KeyCode::Tab, KeyMods::NONE), Some(vec![b'\t']));
        assert_eq!(kitty(KeyCode::Backspace, KeyMods::NONE), Some(vec![0x7f]));

        // At this protocol level the arrows, tilde keys and function keys keep their unambiguous
        // legacy sequences; Kitty only escapes them under `report_all_keys_as_escape_codes`.
        assert_eq!(
            kitty(KeyCode::Left, KeyMods::NONE),
            Some(b"\x1b[D".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::Left, KeyMods::CTRL),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::PageUp, KeyMods::CTRL),
            Some(b"\x1b[5;5~".to_vec())
        );
        assert_eq!(
            kitty(KeyCode::F(5), KeyMods::NONE),
            Some(b"\x1b[15~".to_vec())
        );

        // Super is still dropped: the protocol has a bit for it, but the chord belongs to the app.
        assert_eq!(
            kitty(
                KeyCode::Char('1'),
                KeyMods {
                    super_key: true,
                    ..KeyMods::default()
                }
            ),
            None
        );
    }

    #[test]
    fn ctrl_punctuation_maps_to_control_codes() {
        // Ctrl+/ and Ctrl+_ are both US (0x1f), which readline binds to `undo`. Before this these
        // returned None and the key was dropped before it ever reached the child.
        assert_eq!(enc(KeyCode::Char('/'), KeyMods::CTRL), Some(vec![0x1f]));
        assert_eq!(enc(KeyCode::Char('_'), KeyMods::CTRL), Some(vec![0x1f]));
        assert_eq!(enc(KeyCode::Char('?'), KeyMods::CTRL), Some(vec![0x7f]));
        assert_eq!(enc(KeyCode::Char('@'), KeyMods::CTRL), Some(vec![0x00]));
        assert_eq!(enc(KeyCode::Char(' '), KeyMods::CTRL), Some(vec![0x00]));
        // xterm's digit aliases.
        assert_eq!(enc(KeyCode::Char('2'), KeyMods::CTRL), Some(vec![0x00]));
        assert_eq!(enc(KeyCode::Char('8'), KeyMods::CTRL), Some(vec![0x7f]));
        // Ctrl+1 has no control code, so the app keeps the chord.
        assert_eq!(enc(KeyCode::Char('1'), KeyMods::CTRL), None);
    }

    #[test]
    fn super_modified_keys_are_dropped() {
        // Super has no encoding. Sending the bare key would type a character the user never asked
        // for, so the chord is left for the app to bind.
        let sup = KeyMods {
            super_key: true,
            ..KeyMods::default()
        };
        assert_eq!(enc(KeyCode::Char('c'), sup), None);
        assert_eq!(enc(KeyCode::Left, sup), None);
        assert_eq!(
            enc(
                KeyCode::Char('v'),
                KeyMods {
                    super_key: true,
                    ctrl: true,
                    ..KeyMods::default()
                }
            ),
            None
        );
    }

    #[test]
    fn plain_arrows_are_unmodified() {
        assert_eq!(enc(KeyCode::Left, KeyMods::NONE), Some(b"\x1b[D".to_vec()));
        // Alt alone keeps the historical ESC-prefix form.
        assert_eq!(
            enc(KeyCode::Left, KeyMods::ALT),
            Some(b"\x1b\x1b[D".to_vec())
        );
    }

    #[test]
    fn app_cursor_mode_switches_unmodified_arrows_to_ss3() {
        // DECCKM: ncurses emits `smkx` on startup and then matches arrows against terminfo's
        // `kcuu1=\EOA`, so a child in application mode expects SS3 rather than CSI.
        let modes = TerminalKeyModes {
            app_cursor: true,
            ..TerminalKeyModes::default()
        };
        let app = |code| {
            key_event_to_bytes(
                KeyEvent {
                    code,
                    mods: KeyMods::NONE,
                },
                modes,
            )
        };
        assert_eq!(app(KeyCode::Up), Some(b"\x1bOA".to_vec()));
        assert_eq!(app(KeyCode::Down), Some(b"\x1bOB".to_vec()));
        assert_eq!(app(KeyCode::Right), Some(b"\x1bOC".to_vec()));
        assert_eq!(app(KeyCode::Left), Some(b"\x1bOD".to_vec()));
        assert_eq!(app(KeyCode::Home), Some(b"\x1bOH".to_vec()));
        assert_eq!(app(KeyCode::End), Some(b"\x1bOF".to_vec()));

        // Modified cursor keys stay on the CSI parameterized form regardless of DECCKM.
        assert_eq!(
            key_event_to_bytes(
                KeyEvent {
                    code: KeyCode::Left,
                    mods: KeyMods::CTRL
                },
                modes
            ),
            Some(b"\x1b[1;5D".to_vec())
        );
        // The tilde keys are unaffected too.
        assert_eq!(
            key_event_to_bytes(
                KeyEvent {
                    code: KeyCode::PageUp,
                    mods: KeyMods::NONE
                },
                modes
            ),
            Some(b"\x1b[5~".to_vec())
        );
    }

    #[test]
    fn paste_is_bracketed_only_when_the_child_asked_for_it() {
        let off = TerminalKeyModes::default();
        assert_eq!(encode_paste("hi", off), b"hi".to_vec());

        let on = TerminalKeyModes {
            bracketed_paste: true,
            ..TerminalKeyModes::default()
        };
        assert_eq!(encode_paste("hi", on), b"\x1b[200~hi\x1b[201~".to_vec());
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
