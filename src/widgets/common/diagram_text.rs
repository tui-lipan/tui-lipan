use std::sync::Arc;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Wrap text to a maximum terminal-cell width for diagram labels.
pub(crate) fn wrap_label(text: &str, max_width: u16) -> Arc<[Arc<str>]> {
    let max_width = max_width.max(1) as usize;
    let mut lines = Vec::new();
    for source_line in text.lines() {
        wrap_source_line(source_line, max_width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(Arc::<str>::from(""));
    }
    lines.into()
}

fn wrap_source_line(source: &str, max_width: usize, out: &mut Vec<Arc<str>>) {
    if source.is_empty() {
        out.push(Arc::<str>::from(""));
        return;
    }

    let mut current = String::new();
    let mut current_width = 0usize;
    for word in source.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current_width > 0
            && current_width.saturating_add(1).saturating_add(word_width) <= max_width
        {
            current.push(' ');
            current.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
        } else if current_width == 0 && word_width <= max_width {
            current.push_str(word);
            current_width = word_width;
        } else {
            if !current.is_empty() {
                out.push(Arc::<str>::from(std::mem::take(&mut current)));
                current_width = 0;
            }
            push_wrapped_word(word, max_width, out, &mut current, &mut current_width);
        }
    }

    if !current.is_empty() {
        out.push(Arc::<str>::from(current));
    }
}

fn push_wrapped_word(
    word: &str,
    max_width: usize,
    out: &mut Vec<Arc<str>>,
    current: &mut String,
    current_width: &mut usize,
) {
    for ch in word.chars() {
        let width = ch.width().unwrap_or(0);
        if *current_width > 0 && current_width.saturating_add(width) > max_width {
            out.push(Arc::<str>::from(std::mem::take(current)));
            *current_width = 0;
        }
        current.push(ch);
        *current_width = current_width.saturating_add(width);
    }
}
