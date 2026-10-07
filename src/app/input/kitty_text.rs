//! Preserve Kitty's associated text before Termina discards the third CSI-u parameter.
//!
//! Only the explicitly requested all-keys mode needs this adapter. Strings and bracketed
//! paste are opaque; partial sequences remain buffered across reads.
use std::sync::atomic::{AtomicU64, Ordering};

static MODE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn set_enabled(enabled: bool) {
    let previous = MODE.load(Ordering::Relaxed);
    MODE.store(
        ((previous >> 1) + 1) * 2 + u64::from(enabled),
        Ordering::Relaxed,
    );
}

#[derive(Default)]
pub(crate) struct KittyText {
    pending: Vec<u8>,
    string: bool,
    string_escape: bool,
    paste: bool,
    right_alt: bool,
    mode: u64,
}

impl KittyText {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mode = MODE.load(Ordering::Relaxed);
        if self.mode != mode {
            self.right_alt = false;
            self.mode = mode;
        }
        self.push_with_mode(bytes, mode & 1 != 0)
    }

    fn push_with_mode(&mut self, bytes: &[u8], enabled: bool) -> Vec<u8> {
        let mut output = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            if self.string {
                output.push(byte);
                if byte == 7 || (self.string_escape && byte == b'\\') {
                    self.string = false;
                }
                self.string_escape = byte == 27;
                continue;
            }
            if self.pending.is_empty() {
                if byte == 27 {
                    self.pending.push(byte);
                } else {
                    output.push(byte);
                }
                continue;
            }
            self.pending.push(byte);
            if self.pending.len() == 2 {
                if matches!(byte, b']' | b'P' | b'_' | b'^' | b'X') && !self.paste {
                    self.string = true;
                } else if byte == b'[' {
                    continue;
                }
            } else if !(0x40..=0x7e).contains(&byte) && self.pending.len() < 16384 {
                continue;
            }
            if self.pending == b"\x1b[O" {
                self.right_alt = false;
            }
            if self.pending == b"\x1b[200~" {
                self.paste = true;
            } else if self.pending == b"\x1b[201~" {
                self.paste = false;
            } else if !self.paste
                && enabled
                && byte == b'u'
                && let Some(replacement) = self.text_sequence()
            {
                output.extend(replacement);
                self.pending.clear();
                continue;
            }
            output.append(&mut self.pending);
        }
        output
    }

    pub(crate) fn settle(&mut self) -> Vec<u8> {
        if self.pending == [27] {
            std::mem::take(&mut self.pending)
        } else {
            Vec::new()
        }
    }

    fn text_sequence(&mut self) -> Option<Vec<u8>> {
        let body =
            std::str::from_utf8(self.pending.strip_prefix(b"\x1b[")?.strip_suffix(b"u")?).ok()?;
        let mut fields = body.split(';');
        let code = fields.next()?.split(':').next()?.parse::<u32>().ok()?;
        let mut modifier_fields = fields.next().unwrap_or("1").split(':');
        let modifiers = modifier_fields
            .next()?
            .parse::<u16>()
            .ok()?
            .checked_sub(1)?;
        let kind = modifier_fields.next().unwrap_or("1").parse::<u8>().ok()?;
        if matches!(code, 57449 | 57453) {
            self.right_alt = kind != 3;
            return None;
        }
        // Focus loss invalidates physical state; the next report without Alt also clears it.
        if modifiers & 2 == 0 {
            self.right_alt = false;
        }
        if modifiers & (8 | 16 | 32) != 0 || (modifiers & (2 | 4) != 0 && !self.right_alt) {
            return None;
        }
        // Functional keys retain their normal dispatch, including Shift+Tab and Enter.
        if code != 0 && (code < 32 || (57344..=63743).contains(&code) || code == 127) {
            return None;
        }
        let text_field = fields.next().unwrap_or("");
        let mut text = String::new();
        if !text_field.is_empty() {
            for point in text_field.split(':') {
                let ch = char::from_u32(point.parse().ok()?)?;
                if ch.is_control() {
                    return None;
                }
                text.push(ch);
            }
        }
        if text.is_empty() && modifiers & (2 | 4) != 0 {
            // A right-Alt shortcut without generated text remains a shortcut.
            return None;
        }
        if kind == 3 || text.is_empty() {
            return Some(Vec::new());
        }
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_none() {
            // Preserve Alt for physical-side tracking; consume AltGr's synthetic Ctrl.
            Some(format!("\x1b[{};{}u", u32::from(first), (modifiers & (1 | 2)) + 1).into_bytes())
        } else {
            Some(format!("\x1b[200~{text}\x1b[201~").into_bytes())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_text_survives_fragmentation_and_releases() {
        let mut decoder = KittyText::default();
        assert!(decoder.push_with_mode(b"\x1b[97;2;", true).is_empty());
        assert_eq!(decoder.push_with_mode(b"65u", true), b"\x1b[65;2u");
        assert!(decoder.push_with_mode(b"\x1b[97;2:3u", true).is_empty());
        assert!(decoder.push_with_mode(b"\x1b[39;1u", true).is_empty());
        assert_eq!(
            decoder.push_with_mode(b"\x1b[101;1;233u", true),
            b"\x1b[233;1u"
        );
        assert_eq!(
            decoder.push_with_mode(b"\x1b[0;1;20320:22909u", true),
            "\x1b[200~你好\x1b[201~".as_bytes()
        );
    }

    #[test]
    fn altgr_text_and_shortcuts_remain_distinct() {
        let mut decoder = KittyText::default();
        assert_eq!(decoder.push_with_mode(b"\x1b[106;3u", true), b"\x1b[106;3u");
        decoder.push_with_mode(b"\x1b[57449;3u", true);
        assert_eq!(
            decoder.push_with_mode(b"\x1b[108;3;322u", true),
            b"\x1b[322;3u"
        );
        assert_eq!(decoder.push_with_mode(b"\x1b[97;5u", true), b"\x1b[97;5u");
    }

    #[test]
    fn paste_strings_and_legacy_input_are_opaque() {
        let mut decoder = KittyText::default();
        for bytes in [
            b"\x1b[200~\x1b[97;1u\x1b[201~".as_slice(),
            b"\x1b]0;\x1b[97;1u\x07",
            b"\x1bP\x1b[97;1u\x1b\\",
        ] {
            assert_eq!(decoder.push_with_mode(bytes, true), bytes);
        }
        assert_eq!(decoder.push_with_mode(b"\x1b[97;1u", false), b"\x1b[97;1u");
    }
}
