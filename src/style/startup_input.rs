//! What the user types while startup is still asking the host questions.
//!
//! Before the input reader takes the terminal, startup reads it to collect the host's answers: the
//! capability probe, the color query, the image query, and a final sweep for answers that arrived
//! late. Anything typed or clicked meanwhile lands in those same reads. Discarding it all loses the
//! user's first keys, and passing it all on hands the app raw replies - a kitty graphics answer
//! decodes as an Alt chord followed by typed text. So each read is split: the host's replies go to
//! whoever asked, everything else is kept here, and the input reader starts by replaying it.

use std::sync::Mutex;

static KEPT: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Drop anything an earlier run kept and never handed on.
pub(crate) fn forget() {
    if let Ok(mut kept) = KEPT.lock() {
        kept.clear();
    }
}

/// Keep input read during startup, in order, for the input reader.
pub(crate) fn keep(input: &[u8]) {
    if input.is_empty() {
        return;
    }
    if let Ok(mut kept) = KEPT.lock() {
        kept.extend_from_slice(input);
    }
}

/// Everything kept so far, for the input reader to decode before anything it reads itself.
pub(crate) fn take() -> Vec<u8> {
    KEPT.lock()
        .map(|mut kept| std::mem::take(&mut *kept))
        .unwrap_or_default()
}

/// The parts of what startup read from the terminal.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Split {
    /// Answers to questions put to the host, in arrival order.
    pub(crate) replies: Vec<u8>,
    /// Everything else: keys, clicks, pastes, focus changes.
    pub(crate) input: Vec<u8>,
}

/// Separate the host's replies from the user's input in bytes read during startup.
///
/// A reply is recognised by a shape no key or pointer report has:
/// - `CSI ?` and `CSI >` sequences: device attributes, mode and keyboard-flag reports;
/// - CSI sequences ending in `t` (window and cell sizes), `n` (status) or `R` (cursor position) -
///   startup asks for the cursor position, and the one key that shares the shape, F3 with a
///   modifier, is reported differently once the keyboard protocol is on;
/// - OSC, DCS, APC, PM and SOS strings: colors, capabilities and graphics answers.
///
/// A sequence cut off at the end is kept as input. Its remainder is still on its way, and the input
/// reader puts the two halves back together - and drops the whole if it was a reply after all.
pub(crate) fn split(bytes: &[u8]) -> Split {
    let mut split = Split::default();
    let mut rest = bytes;
    while let Some(&first) = rest.first() {
        if first != 0x1b {
            let text = rest
                .iter()
                .position(|&byte| byte == 0x1b)
                .unwrap_or(rest.len());
            split.input.extend_from_slice(&rest[..text]);
            rest = &rest[text..];
            continue;
        }
        let (length, reply) = match rest.get(1) {
            Some(b'[') => csi(rest),
            Some(b']' | b'P' | b'_' | b'^' | b'X') => string(rest),
            // A lone escape, or an Alt chord.
            _ => (Some(1), false),
        };
        let Some(length) = length else {
            split.input.extend_from_slice(rest);
            break;
        };
        let target = if reply {
            &mut split.replies
        } else {
            &mut split.input
        };
        target.extend_from_slice(&rest[..length]);
        rest = &rest[length..];
    }
    split
}

/// The length of the CSI sequence `bytes` starts with, or `None` if it is cut off, and whether it
/// is a reply.
fn csi(bytes: &[u8]) -> (Option<usize>, bool) {
    let body = &bytes[2..];
    let Some(end) = body.iter().position(|byte| (0x40..=0x7e).contains(byte)) else {
        return (None, false);
    };
    // Bytes other than parameters and intermediates mean this was never a CSI sequence: an Alt+[
    // chord, say. Its two bytes are input, and the rest is read on its own.
    if !body[..end].iter().all(|byte| (0x20..=0x3f).contains(byte)) {
        return (Some(2), false);
    }
    let reply =
        matches!(body.first(), Some(b'?' | b'>')) || matches!(body[end], b't' | b'n' | b'R');
    (Some(2 + end + 1), reply)
}

/// The length of the OSC, DCS, APC, PM or SOS string `bytes` starts with, ended by BEL (OSC only)
/// or ST, or `None` if it is cut off. These are always replies.
fn string(bytes: &[u8]) -> (Option<usize>, bool) {
    let bell = bytes[1] == b']';
    let body = &bytes[2..];
    let end = body
        .iter()
        .enumerate()
        .find_map(|(index, &byte)| match byte {
            0x07 if bell => Some(index + 1),
            0x1b if body.get(index + 1) == Some(&b'\\') => Some(index + 2),
            _ => None,
        });
    (end.map(|end| 2 + end), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_go_one_way_and_input_the_other() {
        let read = b"a\x1b[?62;22c\x1b[<0;11;6M\x1b[12;1R\x1b[6;20;10t\r\
            \x1b]11;rgb:0000/0000/0000\x07\x1b_Gi=31;OK\x1b\\\x1b[A\x1b[?1016;2$y\x1b[0n\x1bx";
        let split = split(read);
        assert_eq!(
            split.input, b"a\x1b[<0;11;6M\r\x1b[A\x1bx",
            "keys, a click, Enter and an Alt chord are input"
        );
        assert_eq!(
            split.replies,
            b"\x1b[?62;22c\x1b[12;1R\x1b[6;20;10t\x1b]11;rgb:0000/0000/0000\x07\x1b_Gi=31;OK\x1b\\\
            \x1b[?1016;2$y\x1b[0n"
        );
    }

    #[test]
    fn a_sequence_cut_off_is_kept_for_the_reader_to_finish() {
        assert_eq!(
            split(b"x\x1b[?62;2"),
            Split {
                replies: Vec::new(),
                input: b"x\x1b[?62;2".to_vec(),
            }
        );
        assert_eq!(split(b"\x1b").input, b"\x1b", "a lone escape is a key");
        assert_eq!(split(b"\x1b_Gi=31").input, b"\x1b_Gi=31");
    }

    #[test]
    fn an_alt_bracket_chord_is_not_taken_for_a_sequence() {
        let split = split(b"\x1b[\x1b[?1c");
        assert_eq!(split.input, b"\x1b[");
        assert_eq!(split.replies, b"\x1b[?1c");
    }

    #[test]
    fn kept_input_is_handed_on_once() {
        forget();
        keep(b"ab");
        keep(b"");
        keep(b"\x1b[A");
        assert_eq!(take(), b"ab\x1b[A");
        assert!(take().is_empty());
        keep(b"stale");
        forget();
        assert!(take().is_empty());
    }
}
