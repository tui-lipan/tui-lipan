//! A second VT parser that tells the observer what the grid engine does not report in-band.
//!
//! Neither engine reports OSC 7/9;9/22/133 with their raw parameters, or XTVERSION, the way the
//! observer needs them. This parser reads the raw bytes ahead of the grid, reports those sequences
//! to the [`EngineObserver`], and stops right after any sequence the observer wants an anchor for,
//! so the grid can be fed exactly up to that byte first. It never touches cell or caret state.

use alacritty_terminal::vte::{Params, Parser, Perform};

use super::{EngineObserver, ModeFlags};

/// Which cursor properties the child explicitly set, for an engine that cannot tell its own
/// defaults from the child's requests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CursorRequests {
    /// `DECSCUSR` with a nonzero style.
    pub(super) shape: bool,
    /// `DECSCUSR` with a nonzero style, or `DECSET`/`DECRST 12`.
    pub(super) blinking: bool,
    /// The shape from the last iTerm2 `OSC 50 ; CursorShape=N` (`0` block, `1` beam,
    /// `2` underline), until a `DECSCUSR` replaces it.
    pub(super) osc_shape: Option<u8>,
    /// The last nonzero `DECSCUSR` style (`1`/`2` block, `3`/`4` underline, `5`/`6` bar).
    pub(super) decscusr: Option<u16>,
}

impl CursorRequests {
    fn csi(&mut self, params: &Params, intermediates: &[u8], action: char) {
        if intermediates == b" " && action == 'q' {
            let style = params
                .iter()
                .next()
                .and_then(|param| param.first().copied())
                .filter(|&style| style != 0);
            *self = Self {
                shape: style.is_some(),
                blinking: style.is_some(),
                osc_shape: None,
                decscusr: style,
            };
        } else if intermediates == b"?"
            && matches!(action, 'h' | 'l')
            && params.iter().any(|param| param == [12])
        {
            self.blinking = true;
        }
    }
}

/// The mouse tracking mode in effect, by xterm's rules: setting `DECSET 9`, `1000`, `1002` or
/// `1003` replaces whichever was on, resetting any of them turns tracking off, and `XTSAVE` and
/// `XTRESTORE` of any of them save and restore the mode in effect.
///
/// Neither engine's mode bits say this. Ghostty keeps each mode's bit apart from the mode in
/// effect, so `?1002h ?1000h` leaves both bits set, and Alacritty keeps `1002` on through a
/// `?1000l`. The screen therefore answers `DECRQM` for these modes itself, from this state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MouseTracking {
    mode: Option<u16>,
    saved: Option<u16>,
}

/// Whether `mode` is one of the mouse tracking modes [`MouseTracking`] owns.
pub(super) fn is_mouse_tracking_mode(mode: u16) -> bool {
    matches!(mode, 9 | 1000 | 1002 | 1003)
}

impl MouseTracking {
    /// Apply a private-mode CSI. Returns the `DECRPM` reply when it is a `DECRQM` for a tracking
    /// mode.
    fn csi(&mut self, params: &Params, intermediates: &[u8], action: char) -> Option<Vec<u8>> {
        let tracking = || {
            params
                .iter()
                .filter_map(|param| match param {
                    &[mode] if is_mouse_tracking_mode(mode) => Some(mode),
                    _ => None,
                })
                .last()
        };
        match (intermediates, action) {
            (b"?", 'h' | 'l') => {
                if let Some(mode) = tracking() {
                    self.mode = (action == 'h').then_some(mode);
                }
            }
            (b"?", 's') if tracking().is_some() => self.saved = self.mode,
            (b"?", 'r') if tracking().is_some() => self.mode = self.saved,
            (b"?$", 'p') => {
                let &[mode] = params.iter().next()? else {
                    return None;
                };
                if is_mouse_tracking_mode(mode) {
                    // DECRPM: 1 is set, 2 is reset.
                    let setting = if self.mode == Some(mode) { 1 } else { 2 };
                    return Some(format!("\x1b[?{mode};{setting}$y").into_bytes());
                }
            }
            _ => {}
        }
        None
    }

    /// The mode in effect as its [`ModeFlags`] bit, or none.
    pub(super) fn flags(self) -> ModeFlags {
        match self.mode {
            Some(9) => ModeFlags::MOUSE_X10,
            Some(1000) => ModeFlags::MOUSE_REPORT_CLICK,
            Some(1002) => ModeFlags::MOUSE_DRAG,
            Some(1003) => ModeFlags::MOUSE_MOTION,
            _ => ModeFlags::empty(),
        }
    }

    /// A distinct small number per mode, for a mode fingerprint.
    pub(super) fn fingerprint(self) -> u64 {
        u64::from(self.mode.unwrap_or(0))
    }
}

/// Parse `bytes` until the observer wants an anchor. Returns how many bytes were consumed.
pub(super) fn advance_until_anchor<O: EngineObserver>(
    parser: &mut Parser,
    observer: &mut O,
    cursor: Option<&mut CursorRequests>,
    mouse: &mut MouseTracking,
    bytes: &[u8],
) -> usize {
    parser.advance_until_terminated(&mut SemanticPerform(observer, cursor, mouse), bytes)
}

/// Every callback besides OSC, CSI, and ESC dispatch keeps the trait's no-op default.
struct SemanticPerform<'o, O>(
    &'o mut O,
    Option<&'o mut CursorRequests>,
    &'o mut MouseTracking,
);

impl<O: EngineObserver> SemanticPerform<'_, O> {
    fn pointer_csi(&mut self, params: &Params, intermediates: &[u8], action: char) {
        if intermediates == b"!" && action == 'p' {
            self.0.pointer_reset();
        }
        if intermediates == b"?" && matches!(action, 'h' | 'l') {
            for param in params.iter() {
                if matches!(param, [47] | [1047] | [1049]) {
                    self.0.alternate_screen(action == 'h');
                }
            }
        }
    }
}

impl<O: EngineObserver> Perform for SemanticPerform<'_, O> {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }
        if let Some(cursor) = self.1.as_deref_mut() {
            cursor.csi(params, intermediates, action);
        }
        if let Some(reply) = self.2.csi(params, intermediates, action) {
            self.0.ordered_reply(reply);
        }
        self.pointer_csi(params, intermediates, action);
        if action != 'q' || intermediates != b">" {
            return;
        }

        let mut params = params.iter();
        let supported_parameter = match params.next() {
            None => true,
            Some(&[0]) => params.next().is_none(),
            Some(_) => false,
        };
        if supported_parameter {
            self.0.xtversion_query();
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore && byte == b'c' && intermediates.is_empty() {
            self.0.pointer_reset();
            if let Some(cursor) = self.1.as_deref_mut() {
                *cursor = CursorRequests::default();
            }
            *self.2 = MouseTracking::default();
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(cursor) = self.1.as_deref_mut()
            && let [b"50", value] = params
            && let Some(shape) = value.strip_prefix(b"CursorShape=")
            && let [digit @ b'0'..=b'2'] = shape
        {
            cursor.shape = true;
            cursor.osc_shape = Some(digit - b'0');
        }
        if matches!(params.first(), Some(&(b"7" | b"9" | b"22" | b"133"))) {
            self.0.osc(params);
        }
    }

    fn terminated(&self) -> bool {
        self.0.wants_anchor()
    }
}

/// Drives a [`SemanticObserver`](crate::widgets::terminal::osc::SemanticObserver) directly, for
/// tests of the observer itself.
#[cfg(test)]
pub(in crate::widgets::terminal) struct SemanticTestDriver(Parser);

#[cfg(test)]
impl SemanticTestDriver {
    pub(in crate::widgets::terminal) fn new() -> Self {
        Self(Parser::new())
    }

    pub(in crate::widgets::terminal) fn advance(
        &mut self,
        observer: &mut crate::widgets::terminal::osc::SemanticObserver,
        bytes: &[u8],
    ) {
        struct Forward<'a>(&'a mut crate::widgets::terminal::osc::SemanticObserver);
        impl EngineObserver for Forward<'_> {
            type Anchor = NoAnchor;
            fn respond(&mut self, _reply: Vec<u8>) {}
            fn grid_step(&mut self, _step: super::GridStep) {}
            fn osc(&mut self, params: &[&[u8]]) {
                self.0.osc(params);
            }
            fn xtversion_query(&mut self) {
                self.0.xtversion_query();
            }
            fn pointer_reset(&mut self) {
                self.0.pointer_reset();
            }
            fn alternate_screen(&mut self, active: bool) {
                self.0.alternate_screen(active);
            }
            fn ordered_reply(&mut self, _reply: Vec<u8>) {}
            fn wants_anchor(&self) -> bool {
                false
            }
            fn take_anchor(&mut self) -> Option<NoAnchor> {
                None
            }
            fn anchor(&mut self, _anchor: NoAnchor, _cursor: Option<super::CursorAnchor>) {}
        }
        enum NoAnchor {}
        impl super::EngineAnchor for NoAnchor {
            fn kind(&self) -> super::AnchorKind {
                match *self {}
            }
        }
        self.0.advance(
            &mut SemanticPerform(&mut Forward(observer), None, &mut MouseTracking::default()),
            bytes,
        );
    }
}
