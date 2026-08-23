use std::io::Write;

/// How the OSC 52 clipboard escape has to be framed to reach the outer terminal.
///
/// A terminal multiplexer sits between the app and the real terminal emulator,
/// so the escape has to survive one extra hop. tmux and GNU screen disagree
/// about which framing they forward, hence the split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    /// No multiplexer: write the bare escape.
    Plain,
    /// tmux: write the bare escape *and* the DCS passthrough copy.
    ///
    /// tmux has two independent mechanisms and they consume different bytes:
    /// `set-clipboard` (default `external`) forwards a bare OSC 52 outward,
    /// while `allow-passthrough` (default `off`, added in tmux 3.3) forwards
    /// the DCS-wrapped copy verbatim. Sending only the wrapped form means the
    /// copy is silently dropped on a default tmux. Sending both lets whichever
    /// mechanism is enabled do the work; if both are enabled the clipboard is
    /// simply set twice to the same text.
    PlainAndPassthrough,
    /// GNU screen: write only the DCS passthrough copy.
    ///
    /// screen has no `set-clipboard` equivalent that forwards a bare OSC 52,
    /// so the passthrough is the only framing that reaches the outer terminal.
    Passthrough,
}

fn framing_from_env() -> Framing {
    // tmux is checked first: when a tmux session runs inside GNU screen (or the
    // reverse) the innermost multiplexer is the one that has to forward.
    if std::env::var_os("TMUX").is_some() {
        Framing::PlainAndPassthrough
    } else if std::env::var_os("STY").is_some() {
        Framing::Passthrough
    } else {
        Framing::Plain
    }
}

/// Wrap `sequence` in tmux's DCS passthrough so the multiplexer forwards its
/// bytes to the outer terminal untouched.
fn passthrough(sequence: &str) -> String {
    format!("\x1bPtmux;\x1b{sequence}\x1b\\")
}

/// Build the exact bytes to write for an OSC 52 clipboard store of `text`.
fn osc52_payload(text: &str, framing: Framing) -> String {
    use base64::{Engine as _, engine::general_purpose};

    let b64 = general_purpose::STANDARD.encode(text);
    let sequence = format!("\x1b]52;c;{b64}\x07");

    match framing {
        Framing::Plain => sequence,
        Framing::PlainAndPassthrough => {
            let wrapped = passthrough(&sequence);
            sequence + &wrapped
        }
        Framing::Passthrough => passthrough(&sequence),
    }
}

pub(crate) fn write_osc52(text: &str) {
    if cfg!(test) {
        return;
    }

    let payload = osc52_payload(text, framing_from_env());

    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(payload.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_B64: &str = "aGVsbG8=";

    fn plain() -> String {
        format!("\x1b]52;c;{HELLO_B64}\x07")
    }

    #[test]
    fn plain_framing_writes_only_the_bare_escape() {
        assert_eq!(osc52_payload("hello", Framing::Plain), plain());
    }

    #[test]
    fn screen_framing_writes_only_the_passthrough() {
        assert_eq!(
            osc52_payload("hello", Framing::Passthrough),
            format!("\x1bPtmux;\x1b{}\x1b\\", plain())
        );
    }

    #[test]
    fn tmux_framing_writes_the_bare_escape_before_the_passthrough() {
        // `set-clipboard` consumes the bare escape and `allow-passthrough`
        // consumes the wrapped one. Only one is typically enabled, and tmux
        // ships with `allow-passthrough` off, so the bare escape has to be
        // there or a default tmux drops the copy entirely.
        let payload = osc52_payload("hello", Framing::PlainAndPassthrough);
        assert_eq!(
            payload,
            format!("{}\x1bPtmux;\x1b{}\x1b\\", plain(), plain())
        );
        assert!(payload.starts_with(&plain()));
    }

    #[test]
    fn payload_encodes_text_as_base64() {
        assert_eq!(
            osc52_payload("hi there", Framing::Plain),
            "\x1b]52;c;aGkgdGhlcmU=\x07"
        );
    }

    #[test]
    fn empty_text_still_produces_a_well_formed_escape() {
        assert_eq!(osc52_payload("", Framing::Plain), "\x1b]52;c;\x07");
    }
}
