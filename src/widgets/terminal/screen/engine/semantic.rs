//! A second VT parser that tells the observer what the grid engine does not report in-band.
//!
//! Neither engine reports OSC 7/9;9/22/133 with their raw parameters, or XTVERSION, the way the
//! observer needs them. This parser reads the raw bytes ahead of the grid, reports those sequences
//! to the [`EngineObserver`], and stops right after any sequence the observer wants an anchor for,
//! so the grid can be fed exactly up to that byte first. It never touches cell or caret state.

use alacritty_terminal::vte::{Params, Parser, Perform};

use super::EngineObserver;

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

/// Parse `bytes` until the observer wants an anchor. Returns how many bytes were consumed.
pub(super) fn advance_until_anchor<O: EngineObserver>(
    parser: &mut Parser,
    observer: &mut O,
    cursor: Option<&mut CursorRequests>,
    bytes: &[u8],
) -> usize {
    parser.advance_until_terminated(&mut SemanticPerform(observer, cursor), bytes)
}

/// Every callback besides OSC, CSI, and ESC dispatch keeps the trait's no-op default.
struct SemanticPerform<'o, O>(&'o mut O, Option<&'o mut CursorRequests>);

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
        self.0
            .advance(&mut SemanticPerform(&mut Forward(observer), None), bytes);
    }
}
