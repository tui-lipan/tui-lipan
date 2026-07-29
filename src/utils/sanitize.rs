//! Sanitization helpers for untrusted display text.

use std::borrow::Cow;
use std::sync::Arc;

use crate::style::Span;

/// Remove terminal escape sequences and Unicode control characters.
///
/// Display text is otherwise preserved byte-for-byte: in particular, leading
/// and trailing spaces are not trimmed. Printable input takes a borrowed fast
/// path based on its bytes and does not allocate.
pub fn sanitize_display_text(input: &str) -> Cow<'_, str> {
    // ASCII printable bytes are the overwhelmingly common case. The character
    // check keeps UTF-8 encoded C1 controls out of the borrowed path while
    // retaining the byte-level fast path for ordinary Unicode text.
    if input.bytes().all(|byte| byte >= 0x20 && byte != 0x7f)
        && !input.chars().any(char::is_control)
    {
        return Cow::Borrowed(input);
    }

    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        EscapeIntermediate,
        Csi,
        Osc,
        OscEscape,
        String,
        StringEscape,
    }

    let mut state = State::Text;
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        state = match state {
            State::Text => match ch {
                '\u{1b}' => State::Escape,
                '\u{9b}' => State::Csi,
                '\u{9d}' => State::Osc,
                '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => State::String,
                _ if ch.is_control() => State::Text,
                _ => {
                    output.push(ch);
                    State::Text
                }
            },
            State::Escape => match ch {
                '[' => State::Csi,
                ']' => State::Osc,
                'P' | 'X' | '^' | '_' => State::String,
                ' '..='/' => State::EscapeIntermediate,
                _ => State::Text,
            },
            State::EscapeIntermediate => {
                if ('0'..='~').contains(&ch) {
                    State::Text
                } else {
                    State::EscapeIntermediate
                }
            }
            State::Csi => {
                if ('@'..='~').contains(&ch) {
                    State::Text
                } else {
                    State::Csi
                }
            }
            State::Osc => match ch {
                '\u{7}' | '\u{9c}' => State::Text,
                '\u{1b}' => State::OscEscape,
                _ => State::Osc,
            },
            State::OscEscape => {
                if ch == '\\' {
                    State::Text
                } else {
                    State::Osc
                }
            }
            State::String => match ch {
                '\u{9c}' => State::Text,
                '\u{1b}' => State::StringEscape,
                _ => State::String,
            },
            State::StringEscape => {
                if ch == '\\' {
                    State::Text
                } else {
                    State::String
                }
            }
        };
    }
    Cow::Owned(output)
}

/// Sanitize one styled span, preserving its style and row policy.
pub fn sanitize_display_span(span: &Span) -> Option<Span> {
    let content = sanitize_display_text(&span.content);
    if content.is_empty() {
        return None;
    }
    Some(match content {
        Cow::Borrowed(_) => span.clone(),
        Cow::Owned(content) => Span {
            content: Arc::from(content),
            style: span.style,
            row_style_policy: span.row_style_policy,
        },
    })
}

/// Sanitize styled spans, dropping spans whose content becomes empty.
pub fn sanitize_display_spans(spans: &[Span]) -> Vec<Span> {
    spans.iter().filter_map(sanitize_display_span).collect()
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{sanitize_display_span, sanitize_display_spans, sanitize_display_text};
    use crate::style::{Color, RowStylePolicy, Span};

    #[test]
    fn clean_text_uses_the_byte_fast_path_without_trimming() {
        assert!(matches!(
            sanitize_display_text("  plain text  "),
            Cow::Borrowed("  plain text  ")
        ));
    }

    #[test]
    fn strips_escape_sequences_and_controls_but_keeps_content_spacing() {
        assert_eq!(
            sanitize_display_text("  \u{1b}[31mred\u{1b}[0m\r\n\u{1b}]0;title\u{7}ok  "),
            "  redok  "
        );
        assert_eq!(
            sanitize_display_text("a\u{9b}31mb\u{9d}title\u{9c}c"),
            "abc"
        );
    }

    #[test]
    fn span_sanitization_preserves_style_and_row_policy() {
        let span = Span::new("  a\n")
            .fg(Color::Red)
            .row_style_policy(RowStylePolicy::Disabled);
        let sanitized = sanitize_display_span(&span).expect("visible content remains");
        assert_eq!(sanitized.content.as_ref(), "  a");
        assert_eq!(sanitized.style, span.style);
        assert_eq!(sanitized.row_style_policy, RowStylePolicy::Disabled);
    }

    #[test]
    fn span_lists_drop_only_empty_results() {
        let spans = [Span::new("\u{1b}[31m"), Span::new(" x ")];
        let sanitized = sanitize_display_spans(&spans);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].content.as_ref(), " x ");
    }
}
