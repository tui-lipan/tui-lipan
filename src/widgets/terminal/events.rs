use std::sync::Arc;

use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};
use crate::style::Span;
use crate::utils::{GridSelection, GridSelectionEvent};

/// Terminal input event source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalInputKind {
    /// Keyboard input encoded for the PTY.
    Key,
    /// Clipboard paste input encoded for the PTY.
    Paste,
    /// Focus-in notification.
    FocusIn,
    /// Focus-out notification.
    FocusOut,
}

/// Terminal input event emitted by the framework.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalInputEvent {
    /// Event source.
    pub kind: TerminalInputKind,
    /// Original key event (if applicable).
    pub key: Option<KeyEvent>,
    /// Encoded bytes suitable for PTY stdin.
    pub bytes: Arc<[u8]>,
}

/// Terminal selection in grid coordinates.
pub type TerminalSelection = GridSelection;

/// Terminal selection event payload.
pub type TerminalSelectionEvent = GridSelectionEvent;

pub fn terminal_selection_text(lines: &[Vec<Span>], selection: &GridSelection) -> String {
    if selection.is_empty() {
        return String::new();
    }

    let mut row_strings = Vec::with_capacity(lines.len());
    for line in lines {
        let mut row = String::new();
        for span in line {
            row.push_str(span.content.as_ref());
        }
        row_strings.push(row);
    }

    selection.extract_text(&row_strings)
}

/// Encode a framework `KeyEvent` into terminal bytes.
///
/// This covers common printable keys and ANSI control sequences.
pub fn key_event_to_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    if let Some(bytes) = modified_special_key_bytes(key.code, key.mods) {
        return Some(bytes);
    }

    let mut bytes = match key.code {
        KeyCode::Char(ch) => {
            if key.mods.ctrl {
                vec![ctrl_char(ch)?]
            } else {
                ch.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n) => format!("\x1b[{}~", f_key_number(n)?).into_bytes(),
    };

    if key.mods.alt {
        let mut alt_prefixed = Vec::with_capacity(bytes.len() + 1);
        alt_prefixed.push(0x1b);
        alt_prefixed.extend(bytes);
        bytes = alt_prefixed;
    }

    Some(bytes)
}

/// The xterm modifier parameter for a modified special key: `1 + shift + 2·alt + 4·ctrl`, so
/// Shift=2, Alt=3, Ctrl=5, Ctrl+Shift=6, and so on (Super is not representable and is ignored).
fn xterm_modifier_param(mods: KeyMods) -> u8 {
    1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl)
}

/// The parameter number for a function key in the `CSI <num> ~` scheme (F1→11 … F12→24),
/// matching the unmodified encoding. `None` for out-of-range function keys.
fn f_key_number(n: u8) -> Option<u8> {
    Some(match n {
        1 => 11,
        2 => 12,
        3 => 13,
        4 => 14,
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    })
}

/// Whether Shift is the only modifier on a key that a terminal emulator conventionally handles
/// itself: Shift+Insert pastes, Shift+PageUp/PageDown page the scrollback.
///
/// This widget forwards those keys to the child rather than consuming them (its scrollback is
/// driven by the wheel and `on_scroll_to`), so encoding them as `CSI <num> ; 2 ~` would hand the
/// child a sequence it does not recognize and turn the key into a no-op. Keep the unmodified form
/// so the child still pages and pastes.
fn shift_reserved_by_emulator(code: KeyCode, mods: KeyMods) -> bool {
    mods.shift
        && !mods.ctrl
        && !mods.alt
        && matches!(code, KeyCode::Insert | KeyCode::PageUp | KeyCode::PageDown)
}

/// Encode a cursor, navigation, or function key that carries a modifier into its xterm
/// parameterized CSI form: `CSI 1 ; <mod> <letter>` for the arrows and Home/End, `CSI <num> ;
/// <mod> ~` for the tilde keys. Without this a modified key like Ctrl+Left would collapse to a
/// bare Left and lose word-wise motion in TUIs (readline, editors).
///
/// Returns `None`, leaving the caller to fall back on the plain encoding, when the key has no
/// parameterized form (`Char`, `Enter`, …), when Alt is the only modifier (that keeps its
/// historical ESC-prefix encoding), and for the Shift-only keys an emulator normally reserves.
fn modified_special_key_bytes(code: KeyCode, mods: KeyMods) -> Option<Vec<u8>> {
    if (!mods.ctrl && !mods.shift) || shift_reserved_by_emulator(code, mods) {
        return None;
    }

    let m = xterm_modifier_param(mods);
    let seq = match code {
        KeyCode::Up => format!("\x1b[1;{m}A"),
        KeyCode::Down => format!("\x1b[1;{m}B"),
        KeyCode::Right => format!("\x1b[1;{m}C"),
        KeyCode::Left => format!("\x1b[1;{m}D"),
        KeyCode::Home => format!("\x1b[1;{m}H"),
        KeyCode::End => format!("\x1b[1;{m}F"),
        KeyCode::Insert => format!("\x1b[2;{m}~"),
        KeyCode::Delete => format!("\x1b[3;{m}~"),
        KeyCode::PageUp => format!("\x1b[5;{m}~"),
        KeyCode::PageDown => format!("\x1b[6;{m}~"),
        KeyCode::F(n) => format!("\x1b[{};{m}~", f_key_number(n)?),
        _ => return None,
    };
    Some(seq.into_bytes())
}

fn ctrl_char(ch: char) -> Option<u8> {
    if ch.is_ascii_alphabetic() {
        return Some((ch.to_ascii_uppercase() as u8) - b'@');
    }

    match ch {
        ' ' => Some(0),
        '[' => Some(27),
        '\\' => Some(28),
        ']' => Some(29),
        '^' => Some(30),
        '_' => Some(31),
        _ => None,
    }
}

/// Mouse reporting mode requested by PTY application.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    /// No mouse reporting.
    #[default]
    None,
    /// X10 compatibility mode (1000) - button press only.
    X10,
    /// Normal tracking (1002) - button press/release.
    Normal,
    /// Any-event tracking (1003) - all motion.
    AnyEvent,
}

/// Mouse protocol encoding.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseEncoding {
    /// Default X10 encoding (coordinates limited to 223).
    #[default]
    X10,
    /// SGR extended encoding (1006) - no coordinate limits.
    Sgr,
    /// UTF-8 extended encoding (1005) - no coordinate limits.
    Utf8,
}

/// Combined mouse mode state.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseModeState {
    /// Mouse reporting mode to enable in the PTY.
    pub mode: MouseMode,
    /// Wire encoding used for mouse reports.
    pub encoding: MouseEncoding,
    /// Whether focus reporting is enabled (CSI ? 1004 h).
    pub focus_events_enabled: bool,
}

/// Encode a MouseEvent to bytes for PTY (SGR 1006 format).
pub fn mouse_event_to_bytes(
    event: MouseEvent,
    encoding: MouseEncoding,
    viewport_offset: (u16, u16),
) -> Option<Vec<u8>> {
    let (button_code, is_release) = match event.kind {
        MouseKind::Down(btn) => (button_to_code(btn), false),
        MouseKind::Up(btn) => (button_to_code(btn), true),
        MouseKind::Drag(btn) => (button_to_code(btn).saturating_add(32), false),
        MouseKind::ScrollUp => (64, false),
        MouseKind::ScrollDown => (65, false),
        // Motion without a pressed button: code 3 ("no button") + 32 (motion
        // flag). Callers gate this on any-event tracking (1003) being active.
        MouseKind::Moved => (35, false),
    };

    let mut cb = button_code;
    if event.mods.shift {
        cb = cb.saturating_add(4);
    }
    if event.mods.alt {
        cb = cb.saturating_add(8);
    }
    if event.mods.ctrl {
        cb = cb.saturating_add(16);
    }

    let cx = event.x.saturating_sub(viewport_offset.0).saturating_add(1);
    let cy = event.y.saturating_sub(viewport_offset.1).saturating_add(1);

    match encoding {
        MouseEncoding::Sgr => {
            let suffix = if is_release { 'm' } else { 'M' };
            Some(format!("\x1b[<{};{};{}{}", cb, cx, cy, suffix).into_bytes())
        }
        MouseEncoding::X10 => {
            if cx > 223 || cy > 223 {
                return None;
            }
            let cb = cb.saturating_add(32);
            let cx = cx.saturating_add(32) as u8;
            let cy = cy.saturating_add(32) as u8;
            Some(vec![0x1b, b'[', b'M', cb, cx, cy])
        }
        MouseEncoding::Utf8 => {
            let mut out = Vec::with_capacity(6);
            out.extend_from_slice(b"\x1b[M");
            out.push(cb.saturating_add(32));
            push_utf8_coord(&mut out, cx.saturating_add(32))?;
            push_utf8_coord(&mut out, cy.saturating_add(32))?;
            Some(out)
        }
    }
}

#[cfg(all(test, feature = "terminal-serde"))]
mod terminal_serde_tests {
    use super::*;

    #[test]
    fn mouse_mode_state_round_trips() {
        let state = MouseModeState {
            mode: MouseMode::AnyEvent,
            encoding: MouseEncoding::Sgr,
            focus_events_enabled: true,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            serde_json::from_str::<MouseModeState>(&json).unwrap(),
            state
        );
    }
}

fn push_utf8_coord(out: &mut Vec<u8>, value: u16) -> Option<()> {
    let mut buffer = [0u8; 4];
    let ch = char::from_u32(u32::from(value))?;
    let encoded = ch.encode_utf8(&mut buffer);
    out.extend_from_slice(encoded.as_bytes());
    Some(())
}

fn button_to_code(btn: MouseButton) -> u8 {
    match btn {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Focus-in escape sequence.
pub fn focus_in_sequence() -> &'static [u8] {
    b"\x1b[I"
}

/// Focus-out escape sequence.
pub fn focus_out_sequence() -> &'static [u8] {
    b"\x1b[O"
}

/// Focus-related sequences (focus-in, focus-out).
pub fn focus_sequences() -> (&'static [u8], &'static [u8]) {
    (focus_in_sequence(), focus_out_sequence())
}

/// Wrap text in bracketed paste mode sequences.
pub fn wrap_bracketed_paste(text: &str) -> Vec<u8> {
    let (start, end) = paste_sequences();
    let mut out = Vec::with_capacity(text.len() + start.len() + end.len());
    out.extend_from_slice(start);
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(end);
    out
}

/// All paste-related sequences.
pub fn paste_sequences() -> (&'static [u8], &'static [u8]) {
    (b"\x1b[200~", b"\x1b[201~")
}
