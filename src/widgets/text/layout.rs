use super::{Overflow, Text};
use crate::style::Span;
use unicode_width::UnicodeWidthStr;

/// Split `&[Span]` on embedded `\n` characters into logical lines.
pub(crate) fn split_spans_on_newlines(spans: &[Span]) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut current_line: Vec<Span> = Vec::new();

    for span in spans {
        for (i, part) in span.content.split('\n').enumerate() {
            if i > 0 {
                lines.push(std::mem::take(&mut current_line));
            }
            if !part.is_empty() {
                current_line.push(Span::new(part).style(span.style));
            }
        }
    }
    lines.push(current_line);
    lines
}

pub fn measure_text_constrained(text: &Text, max_w: Option<u16>) -> (u16, u16) {
    let wrap_width = if matches!(text.overflow, Overflow::Wrap | Overflow::Auto) {
        max_w
    } else {
        None
    };

    let logical_lines = split_spans_on_newlines(&text.spans);
    let mut max_line_w = 0usize;
    let mut total_h = 0usize;

    for logical_line in &logical_lines {
        let logical_w: usize = logical_line
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        max_line_w = max_line_w.max(logical_w);

        match wrap_width {
            Some(ww) if ww > 0 => {
                let wrapped = crate::utils::text::wrap_spans_for_budgets(logical_line, ww, ww);
                total_h += wrapped.len();
            }
            Some(_) => {
                // Zero wrap width: contributes 0 height.
            }
            None => {
                total_h += 1;
            }
        }
    }

    let width = max_w
        .map(|w| max_line_w.min(w as usize))
        .unwrap_or(max_line_w);
    let h = total_h.max(1).min(u16::MAX as usize) as u16;
    let w = width.min(u16::MAX as usize) as u16;
    (w, h)
}

#[cfg(test)]
mod tests {
    use crate::style::Span;
    use crate::widgets::{Overflow, Text};

    use super::measure_text_constrained;

    #[test]
    fn measures_newlines_across_span_boundaries() {
        let text = Text::from_spans([Span::from("ab"), Span::from("\ncd")]);
        assert_eq!(measure_text_constrained(&text, None), (2, 2));
    }

    #[test]
    fn wraps_across_span_boundaries_without_flattening() {
        let text =
            Text::from_spans([Span::from("hello "), Span::from("world")]).overflow(Overflow::Wrap);
        assert_eq!(measure_text_constrained(&text, Some(8)), (8, 2));
    }

    #[test]
    fn wraps_at_slash_break_char() {
        let text = Text::new("foo/bar/baz").overflow(Overflow::Wrap);
        // "foo/" (4) on line 1, "bar/" (4) on line 2, "baz" (3) on line 3
        assert_eq!(measure_text_constrained(&text, Some(5)), (5, 3));
    }

    #[test]
    fn wraps_at_dot_break_char() {
        let text = Text::new("src.widgets.text").overflow(Overflow::Wrap);
        // "src." (4), "widgets." (8 > 6 → new line), "text" (4 > 6-8? → new line)
        // width=6: "src." (4) fits, "widget" would be 4+8=12>6 → wrap
        //   line 1: "src." (4)
        //   line 2: "widgets." (8 > 6 → force-break: "widget" (6), then "s." (2))
        //   line 3: "s." wait...
        // Actually: segments are "src." (4), "widgets." (8), "text" (4)
        // line 1: "src." (4), try "widgets." → 4+8=12 > 6, wrap
        // line 2: "widgets." (8 > 6) → force break: 8-6=2 remainder, count+=1
        //   so line 2 takes 6 chars, line 3 takes 2 chars
        // line 3: "s." (2), try "text" → 2+4=6 ≤ 6
        // line 3: "s.text" (6)
        // Total: 3 lines
        assert_eq!(measure_text_constrained(&text, Some(6)), (6, 3));
    }

    #[test]
    fn long_word_force_breaks() {
        let text = Text::new("abcdefghij").overflow(Overflow::Wrap);
        // 10 chars, width 4 → 3 lines (4+4+2)
        assert_eq!(measure_text_constrained(&text, Some(4)), (4, 3));
    }

    #[test]
    fn wraps_wide_characters_correctly() {
        // Each CJK char is 2 columns wide
        // "你好世界" = 4 chars × 2 cols = 8 columns
        let text = Text::new("你好世界").overflow(Overflow::Wrap);
        // At width 5: "你好" (4 cols) fits, "世界" (4 cols) wraps → 2 lines
        assert_eq!(measure_text_constrained(&text, Some(5)), (5, 2));
        // At width 3: "你" (2 cols) fits, "好" (2 cols) wraps, "世" (2) wraps, "界" (2) wraps → 4 lines
        assert_eq!(measure_text_constrained(&text, Some(3)), (3, 4));
    }
}
