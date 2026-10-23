use std::sync::Arc;

use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};
use crate::style::Span;
use crate::utils::spans::{line_text, line_width, slice_columns};
use crate::utils::{GridSelection, SelectionEnd};

/// Paste content and its entry path, before terminal input encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalPasteEvent {
    /// Local clipboard content, or text emitted by the outer terminal.
    pub content: crate::ClipboardContent,
    /// Distinguishes framework clipboard gestures from terminal-emitted text.
    pub origin: crate::PasteOrigin,
}

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
#[non_exhaustive]
pub struct TerminalInputEvent {
    /// Event source.
    pub kind: TerminalInputKind,
    /// Original key event (if applicable).
    pub key: Option<KeyEvent>,
    /// Encoded bytes suitable for PTY stdin.
    pub bytes: Arc<[u8]>,
}

/// Event emitted when the terminal activates a link under a modified click.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TerminalLinkEvent {
    /// Explicit OSC 8 destination or detected plain-text URL.
    pub uri: Arc<str>,
    /// Zero-based visible viewport row that was activated.
    pub row: usize,
    /// Zero-based display column that was activated.
    pub col: usize,
}

/// Extract a terminal selection from styled rendered lines.
///
/// Terminal selection columns are **display columns**, not character indices. Wide characters and
/// zero-width combining characters therefore use the same column accounting as the renderer.
/// The selection endpoint is exclusive, matching mouse drag coordinates.
pub fn terminal_selection_text(lines: &[Vec<Span>], selection: &GridSelection) -> String {
    terminal_selection_text_with(lines, selection, SelectionEnd::Exclusive, false)
}

/// Extract a terminal selection with an explicit endpoint and row-trimming policy.
pub(crate) fn terminal_selection_text_with(
    lines: &[Vec<Span>],
    selection: &GridSelection,
    endpoint: SelectionEnd,
    trim_row_end: bool,
) -> String {
    if selection.is_empty() && matches!(endpoint, SelectionEnd::Exclusive) {
        return String::new();
    }

    let (start, end) = selection.normalized();
    let mut result = String::new();
    for row in start.row..=end.row {
        let Some(line) = lines.get(row) else { continue };
        let width = line_width(line);
        let col_start = if row == start.row { start.col } else { 0 };
        let col_end = if row == end.row {
            end.col
                .saturating_add(matches!(endpoint, SelectionEnd::Inclusive) as usize)
        } else {
            width
        };
        let mut text = line_text(&slice_columns(line, col_start, col_end));
        if trim_row_end {
            text.truncate(text.trim_end().len());
        }
        result.push_str(&text);
        if row < end.row {
            result.push('\n');
        }
    }
    result
}

/// The [Kitty keyboard protocol] enhancement flags a child program has pushed with `CSI > <flags> u`.
///
/// A terminal must not send the protocol's `CSI u` encodings until the child has asked for them,
/// so these gate [`key_event_to_bytes`]. Only `disambiguate_escape_codes` changes what this encoder
/// emits today; the rest are surfaced so hosts can see what the child negotiated.
///
/// [Kitty keyboard protocol]: https://sw.kovidgoyal.net/kitty/keyboard-protocol/
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboardFlags {
    /// Bit 1. Keys with no unambiguous legacy encoding are reported as `CSI <codepoint>;<mod> u`,
    /// and a lone `Esc` becomes `CSI 27 u` so it cannot be confused with an escape sequence.
    pub disambiguate_escape_codes: bool,
    /// Bit 2. Key release and repeat events are reported. Not emitted: `KeyEvent` carries no kind.
    pub report_event_types: bool,
    /// Bit 4. Shifted and base-layout key codes accompany each report. Not emitted.
    pub report_alternate_keys: bool,
    /// Bit 8. Every key, including plain text, is reported as an escape code. Not emitted.
    pub report_all_keys_as_escape_codes: bool,
    /// Bit 16. The text a key would produce accompanies each report. Not emitted.
    pub report_associated_text: bool,
}

impl KittyKeyboardFlags {
    /// Whether the child has negotiated any part of the protocol.
    pub fn any(&self) -> bool {
        self.disambiguate_escape_codes
            || self.report_event_types
            || self.report_alternate_keys
            || self.report_all_keys_as_escape_codes
            || self.report_associated_text
    }
}

/// Input-affecting modes the child program has turned on.
///
/// The child requests these with `CSI ? <n> h` / `CSI ? <n> l` (DEC private modes) or `CSI > <n> u`
/// (the Kitty keyboard protocol), and they change what bytes a key press or a paste must produce.
/// `TerminalScreen` tracks them and publishes them on
/// [`TerminalRenderSnapshot`](crate::widgets::TerminalRenderSnapshot), the same way it publishes
/// [`MouseModeState`]. Pass [`TerminalKeyModes::default()`] when no child has spoken yet.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalKeyModes {
    /// DECCKM (`CSI ? 1 h`): unmodified cursor keys are introduced by `SS3` (`ESC O`) instead of
    /// `CSI` (`ESC [`). Modified cursor keys always stay on the `CSI` form.
    pub app_cursor: bool,
    /// Bracketed paste (`CSI ? 2004 h`): pasted text is wrapped in `CSI 200~` / `CSI 201~` so the
    /// child can tell it apart from typing. When off, pasting the wrapper would insert its literal
    /// bytes into the child's input.
    pub bracketed_paste: bool,
    /// Kitty keyboard protocol flags the child pushed with `CSI > <flags> u`.
    pub kitty_keyboard: KittyKeyboardFlags,
}

/// How a focused terminal handles a direct `Ctrl+V` shortcut before app commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalPasteShortcutBehavior {
    /// Forward `Ctrl+V` to the child unchanged.
    #[default]
    Forward,
    /// Paste plain text locally, but forward `Ctrl+V` when the clipboard contains files, an image,
    /// or another non-text format so the child can inspect the system clipboard itself.
    Performable,
}

/// Encode a framework `KeyEvent` into terminal bytes.
///
/// This covers common printable keys and ANSI control sequences. `modes` carries the modes the
/// child has negotiated; see [`TerminalKeyModes`]. Returns `None` when the key has no encoding the
/// child would understand, which leaves the caller free to route it elsewhere.
///
/// Chords like `Ctrl+1` have no legacy encoding at all and can only be delivered once the child has
/// negotiated the Kitty keyboard protocol; until then they return `None` rather than being
/// flattened onto some other key's bytes.
///
/// Note that in the legacy encoding `Ctrl+Shift+C` produces `0x03` (SIGINT) exactly like `Ctrl+C`,
/// because a control code has no shift bit. The `Terminal` widget never reaches this path for that
/// chord: the clipboard preflight consumes it first. Direct callers must do the same. Under the
/// Kitty protocol the two are distinct (`CSI 99;6u` versus `CSI 99;5u`).
pub fn key_event_to_bytes(key: KeyEvent, modes: TerminalKeyModes) -> Option<Vec<u8>> {
    // Super has no representation in any encoding we speak. Forwarding the unmodified key would
    // type a character the user never asked for (Super+C inserting a literal `c`), so drop it and
    // let the chord bubble to the app instead.
    if key.mods.super_key {
        return None;
    }

    // Only once the child has negotiated the protocol. Sending `CSI u` unsolicited would hand a
    // legacy child a sequence it cannot parse.
    if modes.kitty_keyboard.disambiguate_escape_codes
        && let Some(bytes) = kitty_csi_u_bytes(key.code, key.mods)
    {
        return Some(bytes);
    }

    if let Some(bytes) = modified_special_key_bytes(key.code, key.mods) {
        return Some(bytes);
    }

    // Ctrl+Backspace has no native PTY encoding, so a terminal has to pick a sequence for it.
    // Emit `ESC DEL` - readline's `backward-kill-word` and the same bytes as Alt+Backspace - so
    // "delete the previous word" works out of the box in shells and line editors, instead of
    // collapsing to a bare Backspace (`DEL`) that only deletes a single character. A child that
    // negotiated the Kitty protocol gets `CSI 127;5 u` above instead.
    if key.mods.ctrl && key.code == KeyCode::Backspace {
        return Some(vec![0x1b, 0x7f]);
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
        KeyCode::Up => cursor_key_bytes(b'A', modes),
        KeyCode::Down => cursor_key_bytes(b'B', modes),
        KeyCode::Right => cursor_key_bytes(b'C', modes),
        KeyCode::Left => cursor_key_bytes(b'D', modes),
        KeyCode::Home => cursor_key_bytes(b'H', modes),
        KeyCode::End => cursor_key_bytes(b'F', modes),
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

/// An unmodified cursor key, introduced by `SS3` when the child has set DECCKM and by `CSI`
/// otherwise. ncurses emits `smkx` (`ESC [ ? 1 h ESC =`) on startup and then matches arrows
/// against terminfo's `kcuu1=\EOA`, so a child in application mode expects `ESC O A`.
fn cursor_key_bytes(final_byte: u8, modes: TerminalKeyModes) -> Vec<u8> {
    let introducer: &[u8] = if modes.app_cursor { b"\x1bO" } else { b"\x1b[" };
    let mut bytes = Vec::with_capacity(3);
    bytes.extend_from_slice(introducer);
    bytes.push(final_byte);
    bytes
}

/// The xterm modifier parameter for a modified special key: `1 + shift + 2·alt + 4·ctrl`, so
/// Shift=2, Alt=3, Ctrl=5, Ctrl+Shift=6, and so on. Super has no bit here; `key_event_to_bytes`
/// drops Super-modified keys before reaching this point.
fn xterm_modifier_param(mods: KeyMods) -> u8 {
    1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl)
}

/// The parameter number for a function key in the `CSI <num> ~` scheme (F1→11 … F20→34),
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
        13 => 25,
        14 => 26,
        15 => 28,
        16 => 29,
        17 => 31,
        18 => 32,
        19 => 33,
        20 => 34,
        _ => return None,
    })
}

/// Encode a chord in the Kitty keyboard protocol's `CSI <codepoint> ; <mod> u` form.
///
/// Only reached once the child has set `disambiguate_escape_codes`. This is what lets a chord like
/// `Ctrl+1` reach the child at all: it has no legacy encoding, so without the protocol it can only
/// be dropped. `Ctrl+Enter` and `Shift+Enter` likewise become distinguishable from a bare `Enter`.
///
/// Returns `None` for the keys that keep their legacy encoding at this protocol level: plain text,
/// the arrows, `Home`/`End`, the tilde keys, and the function keys. Those already have unambiguous
/// sequences, and Kitty only escapes them under `report_all_keys_as_escape_codes`.
fn kitty_csi_u_bytes(code: KeyCode, mods: KeyMods) -> Option<Vec<u8>> {
    let (codepoint, mods) = match code {
        // A text-producing key still produces text under Shift alone. Ctrl and Alt are what
        // promote it to the escape form, because a control code cannot express which key it was.
        KeyCode::Char(ch) if mods.ctrl || mods.alt => (kitty_char_codepoint(ch), mods),
        // The whole point of `disambiguate_escape_codes`: a lone Esc must not look like the start
        // of an escape sequence.
        KeyCode::Esc => (27, mods),
        KeyCode::Enter if !mods.is_empty() => (13, mods),
        KeyCode::Tab if mods.ctrl || mods.alt => (9, mods),
        // BackTab *is* Shift+Tab, so put the shift back into the parameter even when the backend
        // reported the chord without it.
        KeyCode::BackTab => (
            9,
            KeyMods {
                shift: true,
                ..mods
            },
        ),
        KeyCode::Backspace if mods.ctrl || mods.alt => (127, mods),
        _ => return None,
    };

    let m = xterm_modifier_param(mods);
    let seq = if m == 1 {
        format!("\x1b[{codepoint}u")
    } else {
        format!("\x1b[{codepoint};{m}u")
    };
    Some(seq.into_bytes())
}

/// The codepoint Kitty reports for a character key: the key as engraved, without Shift applied.
/// `Ctrl+Shift+C` therefore reports `c` (99) with the shift bit set in the modifier parameter.
fn kitty_char_codepoint(ch: char) -> u32 {
    ch.to_lowercase().next().unwrap_or(ch) as u32
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

/// The C0 control code a `Ctrl+<char>` chord produces, or `None` when the chord has no control
/// code (`Ctrl+1`, `Ctrl+;`, …) and should be left for the app to handle.
fn ctrl_char(ch: char) -> Option<u8> {
    if ch.is_ascii_alphabetic() {
        return Some((ch.to_ascii_uppercase() as u8) - b'@');
    }

    Some(match ch {
        ' ' | '@' => 0x00,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        // US. Readline binds it to `undo`, and `/` reaches it without Shift on most layouts.
        '_' | '/' => 0x1f,
        '?' => 0x7f,
        // xterm's digit aliases, for the control codes whose named key needs Shift.
        '2' => 0x00,
        '3' => 0x1b,
        '4' => 0x1c,
        '5' => 0x1d,
        '6' => 0x1e,
        '7' => 0x1f,
        '8' => 0x7f,
        _ => return None,
    })
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
    /// SGR-pixels encoding (1016) - SGR's shape, reporting pixels instead of cells.
    ///
    /// A program that asks for this is drawing something a cell is too coarse for: dragging a
    /// scrollbar, a canvas, a map. It only means anything when the host reports the pointer in
    /// pixels too, so [`TerminalScreen`](crate::widgets::TerminalScreen) answers the mode's
    /// `DECRQM` probe with what is actually available rather than with what was asked for.
    SgrPixels,
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

/// Where a pane sits, and how finely the host can say where the pointer is inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseReportGeometry {
    /// Cell position of the pane's content area on screen, subtracted to make the report relative.
    pub content_origin: (u16, u16),
    /// Where in its cell the pointer is, in pixels, when the host reports that finely.
    ///
    /// `None` leaves [`MouseEncoding::SgrPixels`] reporting cell corners, which is the honest
    /// answer when all the host said was which cell.
    pub sub_cell: Option<(u16, u16)>,
    /// Host cell size in pixels, used to turn a cell position into a pixel one.
    pub cell: super::TerminalCellSize,
}

impl MouseReportGeometry {
    /// Geometry for a pane at `content_origin` with no sub-cell information.
    pub fn at(content_origin: (u16, u16)) -> Self {
        Self {
            content_origin,
            sub_cell: None,
            cell: super::TerminalCellSize::default(),
        }
    }
}

/// A mouse report forwarded to a child terminal, with semantics retained before wire encoding.
/// Coordinates are zero-based and relative to the terminal's content area.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwardedMouseEvent {
    /// Press, release, drag, motion, or wheel direction, including the button when applicable.
    pub kind: MouseKind,
    /// Modifiers received from the host. Shift normally selects locally instead of forwarding.
    pub mods: crate::core::event::KeyMods,
    /// Cell position inside the terminal content area.
    pub cell: (u16, u16),
    /// Pixel position when the host supplied sub-cell precision, clamped to the reported cell.
    pub pixel: Option<(u16, u16)>,
    /// Tracking mode and encoding requested by the child at the time of forwarding.
    pub mouse_mode: MouseModeState,
    /// Number of reports represented by this callback. Coalesced wheel ticks retain their count.
    pub repeat_count: u16,
    /// Authoritative encoded reports, including repetitions. Forward these bytes unchanged.
    pub bytes: Vec<u8>,
}

impl ForwardedMouseEvent {
    pub(crate) fn from_mouse(
        event: MouseEvent,
        mouse_mode: MouseModeState,
        geometry: MouseReportGeometry,
    ) -> Option<Self> {
        let cell = (
            event.x.saturating_sub(geometry.content_origin.0),
            event.y.saturating_sub(geometry.content_origin.1),
        );
        let pixel = geometry.sub_cell.map(|_| {
            let (x, y) = mouse_pixel_position(cell, geometry);
            (x.saturating_sub(1), y.saturating_sub(1))
        });
        let mut forwarded = Self {
            kind: event.kind,
            mods: event.mods,
            cell,
            pixel,
            mouse_mode,
            repeat_count: 1,
            bytes: Vec::new(),
        };
        forwarded.bytes = mouse_event_to_bytes(event, mouse_mode.encoding, geometry)?;
        Some(forwarded)
    }
}

/// One-based pixel coordinates used by SGR-pixels, including its saturation rules.
fn mouse_pixel_position(cell: (u16, u16), geometry: MouseReportGeometry) -> (u16, u16) {
    let (sub_x, sub_y) = geometry.sub_cell.unwrap_or((0, 0));
    let coordinate = |cell: u16, size: u16, sub: u16| {
        cell.saturating_mul(size)
            .saturating_add(sub.min(size.saturating_sub(1)))
            .saturating_add(1)
    };
    (
        coordinate(cell.0, geometry.cell.width, sub_x),
        coordinate(cell.1, geometry.cell.height, sub_y),
    )
}

fn mouse_kind_code(kind: MouseKind) -> (u8, bool) {
    match kind {
        MouseKind::Down(btn) => (button_to_code(btn), false),
        MouseKind::Up(btn) => (button_to_code(btn), true),
        MouseKind::Drag(btn) => (button_to_code(btn).saturating_add(32), false),
        MouseKind::ScrollUp => (64, false),
        MouseKind::ScrollDown => (65, false),
        // Motion without a pressed button: code 3 ("no button") + 32 (motion
        // flag). Callers gate this on any-event tracking (1003) being active.
        MouseKind::Moved => (35, false),
    }
}

fn mouse_report_code(event: MouseEvent, encoding: MouseEncoding) -> (u8, bool) {
    let (button_code, is_release) = mouse_kind_code(event.kind);
    let mut cb = if is_release && matches!(encoding, MouseEncoding::X10 | MouseEncoding::Utf8) {
        3 // Legacy releases identify no button, unlike SGR which retains the released button.
    } else {
        button_code
    };
    if event.mods.shift {
        cb = cb.saturating_add(4);
    }
    if event.mods.alt {
        cb = cb.saturating_add(8);
    }
    if event.mods.ctrl {
        cb = cb.saturating_add(16);
    }

    (cb, is_release)
}

/// Encode a mouse event using the child terminal's selected wire protocol.
pub fn mouse_event_to_bytes(
    event: MouseEvent,
    encoding: MouseEncoding,
    geometry: MouseReportGeometry,
) -> Option<Vec<u8>> {
    let viewport_offset = geometry.content_origin;
    let (cb, is_release) = mouse_report_code(event, encoding);

    let cx = event.x.saturating_sub(viewport_offset.0).saturating_add(1);
    let cy = event.y.saturating_sub(viewport_offset.1).saturating_add(1);

    match encoding {
        MouseEncoding::Sgr => {
            let suffix = if is_release { 'm' } else { 'M' };
            Some(format!("\x1b[<{};{};{}{}", cb, cx, cy, suffix).into_bytes())
        }
        MouseEncoding::SgrPixels => {
            let (px, py) =
                mouse_pixel_position((cx.saturating_sub(1), cy.saturating_sub(1)), geometry);
            let suffix = if is_release { 'm' } else { 'M' };
            Some(format!("\x1b[<{};{};{}{}", cb, px, py, suffix).into_bytes())
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

/// Encode pasted text for the child's stdin.
///
/// Wraps the text in the bracketed-paste sequences only when the child has enabled the mode
/// (`CSI ? 2004 h`). A child that has not asked for bracketed paste does not strip the wrapper, so
/// sending it unconditionally would insert the literal bytes `ESC [ 200 ~` into its input.
pub fn encode_paste(text: &str, modes: TerminalKeyModes) -> Vec<u8> {
    if !modes.bracketed_paste {
        return text.as_bytes().to_vec();
    }

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

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn terminal_selection_text_uses_display_columns_for_wide_characters() {
        use crate::utils::GridPos;

        let lines = vec![vec![Span::new("a界🙂b")]];
        let mut cjk = GridSelection::new(GridPos { row: 0, col: 1 });
        cjk.extend_to(GridPos { row: 0, col: 3 });
        assert_eq!(terminal_selection_text(&lines, &cjk), "界");

        let mut emoji = GridSelection::new(GridPos { row: 0, col: 3 });
        emoji.extend_to(GridPos { row: 0, col: 5 });
        assert_eq!(terminal_selection_text(&lines, &emoji), "🙂");
    }
}

#[cfg(test)]
mod forwarded_mouse_tests {
    use super::*;
    use crate::core::event::KeyMods;

    #[test]
    fn structured_reports_retain_semantics_and_exact_protocol_bytes() {
        let cases = [
            (MouseKind::Down(MouseButton::Left), 0, false),
            (MouseKind::Down(MouseButton::Middle), 1, false),
            (MouseKind::Down(MouseButton::Right), 2, false),
            (MouseKind::Up(MouseButton::Left), 0, true),
            (MouseKind::Up(MouseButton::Middle), 1, true),
            (MouseKind::Up(MouseButton::Right), 2, true),
            (MouseKind::Drag(MouseButton::Left), 32, false),
            (MouseKind::Drag(MouseButton::Middle), 33, false),
            (MouseKind::Drag(MouseButton::Right), 34, false),
            (MouseKind::Moved, 35, false),
            (MouseKind::ScrollUp, 64, false),
            (MouseKind::ScrollDown, 65, false),
        ];
        for encoding in [
            MouseEncoding::Sgr,
            MouseEncoding::SgrPixels,
            MouseEncoding::X10,
            MouseEncoding::Utf8,
        ] {
            for (kind, base, release) in cases {
                for bits in 0..16 {
                    let mods = KeyMods {
                        shift: bits & 1 != 0,
                        alt: bits & 2 != 0,
                        ctrl: bits & 4 != 0,
                        super_key: bits & 8 != 0,
                        ..KeyMods::default()
                    };
                    let geometry = MouseReportGeometry {
                        content_origin: (10, 20),
                        sub_cell: Some((3, 5)),
                        cell: super::super::TerminalCellSize {
                            width: 8,
                            height: 16,
                        },
                    };
                    let mode = MouseModeState {
                        mode: MouseMode::AnyEvent,
                        encoding,
                        focus_events_enabled: true,
                    };
                    let event = MouseEvent {
                        kind,
                        x: 12,
                        y: 24,
                        mods,
                    };
                    let forwarded = ForwardedMouseEvent::from_mouse(event, mode, geometry).unwrap();
                    assert_eq!(forwarded.kind, kind);
                    assert_eq!(forwarded.mods, mods);
                    assert_eq!(forwarded.cell, (2, 4));
                    assert_eq!(forwarded.pixel, Some((19, 69)));
                    assert_eq!(forwarded.mouse_mode, mode);
                    assert_eq!(forwarded.repeat_count, 1);
                    let modifier_code =
                        (bits & 1) * 4 + ((bits >> 1) & 1) * 8 + ((bits >> 2) & 1) * 16;
                    let legacy = matches!(encoding, MouseEncoding::X10 | MouseEncoding::Utf8);
                    let code = if release && legacy { 3 } else { base } + modifier_code;
                    let suffix = if release { 'm' } else { 'M' };
                    let expected = match encoding {
                        MouseEncoding::Sgr => format!("\x1b[<{code};3;5{suffix}").into_bytes(),
                        MouseEncoding::SgrPixels => {
                            format!("\x1b[<{code};20;70{suffix}").into_bytes()
                        }
                        MouseEncoding::X10 | MouseEncoding::Utf8 => {
                            vec![27, b'[', b'M', code + 32, 35, 37]
                        }
                    };
                    assert_eq!(forwarded.bytes, expected, "{kind:?} {encoding:?} {mods:?}");
                }
            }
        }
    }

    #[test]
    fn pixel_precision_is_optional_and_clamped_before_encoding() {
        let event = MouseEvent {
            kind: MouseKind::Moved,
            x: 12,
            y: 24,
            mods: KeyMods::default(),
        };
        let mode = MouseModeState {
            mode: MouseMode::AnyEvent,
            encoding: MouseEncoding::SgrPixels,
            ..MouseModeState::default()
        };
        let geometry = MouseReportGeometry {
            content_origin: (10, 20),
            sub_cell: None,
            cell: super::super::TerminalCellSize {
                width: 8,
                height: 16,
            },
        };
        let coarse = ForwardedMouseEvent::from_mouse(event, mode, geometry).unwrap();
        assert_eq!(coarse.pixel, None);
        assert_eq!(coarse.bytes, b"\x1b[<35;17;65M");
        let precise = ForwardedMouseEvent::from_mouse(
            event,
            mode,
            MouseReportGeometry {
                sub_cell: Some((500, 500)),
                ..geometry
            },
        )
        .unwrap();
        assert_eq!(precise.pixel, Some((23, 79)));
        assert_eq!(precise.bytes, b"\x1b[<35;24;80M");
    }

    #[test]
    fn protocol_coordinate_limits_do_not_invent_forwarded_events() {
        let event = MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            x: 300,
            y: 2,
            mods: KeyMods::default(),
        };
        let mode = MouseModeState {
            mode: MouseMode::Normal,
            encoding: MouseEncoding::X10,
            ..MouseModeState::default()
        };
        let geometry = MouseReportGeometry::at((0, 0));
        assert!(ForwardedMouseEvent::from_mouse(event, mode, geometry).is_none());
        let utf8 = ForwardedMouseEvent::from_mouse(
            event,
            MouseModeState {
                encoding: MouseEncoding::Utf8,
                ..mode
            },
            geometry,
        )
        .unwrap();
        assert_eq!(utf8.cell, (300, 2));
        assert_eq!(utf8.bytes, "\x1b[M ō#".as_bytes());
    }
}
