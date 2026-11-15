//! Diff color strategy for TextArea.

use super::render::*;
use crate::style::DiffPalette;
use crate::style::{Span, Style, StyleSlot};
use crate::widgets::{TextAreaColorInput, TextAreaColorLines, TextAreaColorStrategy};
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

pub(crate) const DIFF_FULL_WIDTH_PAD_CELLS: usize = 512;

#[derive(Clone)]
pub(crate) struct DiffColorStrategy {
    pub(crate) lines: Arc<Vec<DiffRenderLine>>,
    pub(crate) raw_text: Arc<str>,
    pub(crate) diff_hash: u64,
    pub(crate) base: Option<Rc<dyn TextAreaColorStrategy>>,
    pub(crate) style: DiffPalette,
    pub(crate) highlight_full_width: bool,
    pub(crate) pad_full_width: bool,
    pub(crate) selected_lines: Arc<[bool]>,
    pub(crate) selection_slot: StyleSlot,
    pub(crate) selection_overlay: Style,
    /// Cached [`TextAreaColorStrategy::cache_key`] (palette + base); recomputed after theme patch.
    strategy_cache_key: u64,
    /// Cached hash of [`Self::style`]; recomputed alongside `strategy_cache_key`.
    pub(crate) style_hash: u64,
}

impl DiffColorStrategy {
    pub fn new(
        render: DiffRender,
        base: Option<Rc<dyn TextAreaColorStrategy>>,
        style: DiffPalette,
        highlight_full_width: bool,
        pad_full_width: bool,
    ) -> Self {
        let mut s = Self {
            lines: render.lines,
            raw_text: render.raw_text,
            diff_hash: render.diff_hash,
            base,
            style,
            highlight_full_width,
            pad_full_width,
            selected_lines: Arc::from([]),
            selection_slot: StyleSlot::Inherit,
            selection_overlay: Style::default(),
            strategy_cache_key: 0,
            style_hash: 0,
        };
        s.recompute_strategy_cache_key();
        s
    }

    pub(crate) fn with_line_selection(
        mut self,
        selected_lines: Arc<[bool]>,
        selection_slot: StyleSlot,
    ) -> Self {
        self.selected_lines = selected_lines;
        self.selection_slot = selection_slot;
        self.selection_overlay = selection_slot.explicit_style().unwrap_or_default();
        self.recompute_strategy_cache_key();
        self
    }

    pub(crate) fn recompute_strategy_cache_key(&mut self) {
        self.style_hash = diff_style_hash(&self.style);
        self.strategy_cache_key = diff_color_strategy_cache_key(
            self.diff_hash,
            self.style_hash,
            self.highlight_full_width,
            self.pad_full_width,
            self.base.as_ref().map(|b| b.cache_key()),
            self.selected_lines.as_ref(),
            self.selection_overlay,
        );
    }

    /// Geometry-only key: excludes palette and syntax-theme since those are
    /// purely cosmetic for diffs (they change colors, not text content or wrapping).
    pub(crate) fn measure_cache_key(&self) -> u64 {
        let mut hasher = FxHasher::default();
        self.diff_hash.hash(&mut hasher);
        self.highlight_full_width.hash(&mut hasher);
        self.pad_full_width.hash(&mut hasher);
        if self.highlight_full_width && self.pad_full_width {
            self.style_hash.hash(&mut hasher);
        }
        hasher.finish()
    }
}

pub(crate) fn diff_style_hash(style: &DiffPalette) -> u64 {
    let mut h = FxHasher::default();
    style.hash(&mut h);
    h.finish()
}

fn diff_color_strategy_cache_key(
    diff_hash: u64,
    style_hash: u64,
    highlight_full_width: bool,
    pad_full_width: bool,
    base_key: Option<u64>,
    selected_lines: &[bool],
    selection_style: Style,
) -> u64 {
    let mut hasher = FxHasher::default();
    diff_hash.hash(&mut hasher);
    style_hash.hash(&mut hasher);
    highlight_full_width.hash(&mut hasher);
    pad_full_width.hash(&mut hasher);
    base_key.hash(&mut hasher);
    selected_lines.hash(&mut hasher);
    selection_style.hash(&mut hasher);
    hasher.finish()
}

impl DiffColorStrategy {
    /// Syntax-highlight each patch hunk on its own.
    ///
    /// A patch shows fragments of a file. Highlighting them as one text lets
    /// lexer state leak across the gap between hunks: a hunk that ends inside
    /// what looks like an open string or block comment would paint every later
    /// hunk as that string. Each hunk starts from a fresh state instead. Diffs
    /// without hunks (built from full before/after content) stay one pass.
    fn highlight_by_hunk(
        &self,
        base: &dyn TextAreaColorStrategy,
        input: TextAreaColorInput<'_>,
    ) -> TextAreaColorLines {
        let hunk_of = |line: &DiffRenderLine| line.hunk.map(|hunk| hunk.index);
        let single_segment = self
            .lines
            .windows(2)
            .all(|pair| hunk_of(&pair[0]) == hunk_of(&pair[1]));
        if single_segment {
            let lines = base.highlight(TextAreaColorInput {
                value: self.raw_text.as_ref(),
                language: input.language,
                theme: input.theme,
            });
            return normalize_lines(lines, self.lines.len());
        }

        let mut out = Vec::with_capacity(self.lines.len());
        let mut start = 0;
        while start < self.lines.len() {
            let hunk = hunk_of(&self.lines[start]);
            let end = self.lines[start..]
                .iter()
                .position(|line| hunk_of(line) != hunk)
                .map_or(self.lines.len(), |offset| start + offset);
            let segment = &self.lines[start..end];
            let mut text = segment
                .iter()
                .map(|line| line.text.as_ref())
                .collect::<Vec<_>>()
                .join("\n");
            if end < self.lines.len() {
                text.push('\n');
            }
            let lines = base.highlight(TextAreaColorInput {
                value: &text,
                language: input.language,
                theme: input.theme,
            });
            out.extend(normalize_lines(lines, segment.len()));
            start = end;
        }
        out
    }
}

impl TextAreaColorStrategy for DiffColorStrategy {
    fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines {
        let raw_value = self.raw_text.as_ref();
        let base_lines = if let Some(base) = &self.base {
            self.highlight_by_hunk(base.as_ref(), input)
        } else {
            normalize_lines(plain_lines(raw_value), self.lines.len())
        };

        let mut out = Vec::with_capacity(self.lines.len());
        for (idx, line) in self.lines.iter().enumerate() {
            let mut base = base_lines
                .get(idx)
                .cloned()
                .unwrap_or_else(|| vec![Span::new("")]);
            // Separators and patch metadata are not source code: syntax
            // highlighting would color e.g. the count in "7 hidden lines".
            if matches!(
                line.kind,
                DiffLineKind::Separator | DiffLineKind::PatchHeader
            ) {
                base = vec![Span::new(
                    base.iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>(),
                )];
            }
            let line_style = self.style.line_style(line.kind);
            let content_style = line_overlay_style(line_style);
            let mut spans = apply_line_style(base, content_style);

            if let Some(word_style) = self.style.word_style(line.kind) {
                spans = apply_word_ranges(spans, &line.word_ranges, word_style);
            }

            if self.selected_lines.get(idx).copied().unwrap_or(false) {
                spans = apply_line_style(spans, self.selection_overlay);
                if self.selection_overlay.bg.is_some() {
                    spans.insert(0, Span::new("").style(self.selection_overlay));
                }
            }

            if self.highlight_full_width && content_style.bg.is_some() {
                spans.insert(0, Span::new("").style(content_style));
            }

            if self.highlight_full_width && self.pad_full_width && content_style.bg.is_some() {
                spans.push(Span::new(" ".repeat(DIFF_FULL_WIDTH_PAD_CELLS)).style(content_style));
            }

            if spans.is_empty() {
                spans.push(Span::new(""));
            }

            out.push(spans);
        }

        out
    }

    fn cache_key(&self) -> u64 {
        self.strategy_cache_key
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub(crate) fn build_prefix_spans(
    prefix: &str,
    kind: DiffLineKind,
    style: DiffPalette,
    line_style: Style,
) -> Vec<Span> {
    if let Some(marker_style) = style.marker_style(kind, line_style)
        && let Some((idx, ch)) = prefix.char_indices().find(|(_, c)| *c == '+' || *c == '-')
    {
        let marker_end = idx + ch.len_utf8();

        // All three gutter segments (line-number, marker char, trailing space)
        // must share the same background so the prefix column looks uniform.
        //
        // `line_style` is already the output of `prefix_overlay_style`, which
        // dims the content bg.  If the palette carries an explicit bg on either
        // `added_marker` or `added_line_number` that explicit colour wins (in
        // that priority order); otherwise we keep the already-dimmed bg.
        let (palette_marker_bg, palette_num_bg) = match kind {
            DiffLineKind::Added => (style.added_marker.bg, style.added_line_number.bg),
            DiffLineKind::Removed => (style.removed_marker.bg, style.removed_line_number.bg),
            _ => (None, None),
        };
        let unified_bg = palette_marker_bg.or(palette_num_bg).or(line_style.bg);

        let marker_style = Style {
            bg: unified_bg,
            ..marker_style
        };

        let mut out = Vec::with_capacity(3);
        if idx > 0 {
            let num_style = style
                .line_number_style(kind, line_style)
                .map(|s| Style {
                    bg: unified_bg,
                    ..s
                })
                .unwrap_or(line_style);
            out.push(Span::new(prefix[..idx].to_owned()).style(num_style));
        }
        out.push(Span::new(prefix[idx..marker_end].to_owned()).style(marker_style));
        if marker_end < prefix.len() {
            out.push(Span::new(prefix[marker_end..].to_owned()).style(marker_style));
        }
        return out;
    }

    if let Some(split_idx) = split_line_number_prefix(prefix)
        && let Some(num_style) = style.line_number_style(kind, line_style)
    {
        let mut out = Vec::with_capacity(2);
        out.push(Span::new(prefix[..split_idx].to_owned()).style(num_style));
        if split_idx < prefix.len() {
            out.push(Span::new(prefix[split_idx..].to_owned()).style(line_style));
        }
        return out;
    }

    vec![Span::new(prefix.to_owned()).style(line_style)]
}

fn split_line_number_prefix(prefix: &str) -> Option<usize> {
    let first_digit = prefix.find(|ch: char| ch.is_ascii_digit())?;
    if !prefix[..first_digit].chars().all(|ch| ch == ' ') {
        return None;
    }

    let mut end = first_digit;
    for (idx, ch) in prefix[first_digit..].char_indices() {
        if ch.is_ascii_digit() {
            end = first_digit + idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if end < prefix.len() && prefix[end..].starts_with(' ') {
        end += ' '.len_utf8();
    }

    Some(end)
}

pub(crate) fn line_overlay_style(style: Style) -> Style {
    Style {
        fg: None,
        bg: style.bg,
        fg_transform: None,
        bg_transform: style.bg_transform,
        contrast_policy: style.contrast_policy,
        bold: None,
        dim: style.dim,
        italic: None,
        underline: None,
        reverse: None,
        strikethrough: None,
        underline_color: None,
        dim_amount: style.dim_amount,
        tint: None,
    }
}

pub(crate) fn prefix_overlay_style(style: Style, kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Added | DiffLineKind::Removed => {
            let mut s = style;
            if s.bg.is_some() {
                s = s.dim_by(0.25);
            }
            s
        }
        DiffLineKind::Context
        | DiffLineKind::PatchHeader
        | DiffLineKind::Empty
        | DiffLineKind::Separator => style,
    }
}

fn normalize_lines(mut lines: TextAreaColorLines, expected: usize) -> TextAreaColorLines {
    if lines.len() > expected {
        lines.truncate(expected);
    }
    while lines.len() < expected {
        lines.push(vec![Span::new("")]);
    }
    lines
}

fn plain_lines(value: &str) -> TextAreaColorLines {
    if value.is_empty() {
        return vec![vec![Span::new("")]];
    }
    value
        .split('\n')
        .map(|line| vec![Span::new(line)])
        .collect()
}

fn apply_line_style(spans: Vec<Span>, style: Style) -> Vec<Span> {
    spans
        .into_iter()
        .map(|mut span| {
            span.style = span.style.patch(style);
            span
        })
        .collect()
}

fn apply_word_ranges(spans: Vec<Span>, ranges: &[WordRange], word_style: Style) -> Vec<Span> {
    if ranges.is_empty() {
        return spans;
    }

    let mut out = Vec::new();
    let mut range_idx = 0usize;
    let mut offset = 0usize;

    for span in spans {
        let text = span.content.as_ref();
        let span_start = offset;
        let span_end = span_start.saturating_add(text.len());
        offset = span_end;

        let mut cursor = span_start;
        while range_idx < ranges.len() && ranges[range_idx].end <= span_start {
            range_idx += 1;
        }

        while range_idx < ranges.len() && ranges[range_idx].start < span_end {
            let range = ranges[range_idx];
            let seg_start = range.start.max(span_start);
            let seg_end = range.end.min(span_end);

            if seg_start > cursor {
                let local_start = cursor.saturating_sub(span_start);
                let local_end = seg_start.saturating_sub(span_start);
                let slice = &text[local_start..local_end];
                if !slice.is_empty() {
                    out.push(Span::new(slice).style(span.style));
                }
            }

            if seg_end > seg_start {
                let local_start = seg_start.saturating_sub(span_start);
                let local_end = seg_end.saturating_sub(span_start);
                let slice = &text[local_start..local_end];
                if !slice.is_empty() {
                    out.push(Span::new(slice).style(span.style.patch(word_style)));
                }
            }

            cursor = seg_end;
            if range.end <= span_end {
                range_idx += 1;
            } else {
                break;
            }
        }

        if cursor < span_end {
            let local_start = cursor.saturating_sub(span_start);
            let slice = &text[local_start..];
            if !slice.is_empty() {
                out.push(Span::new(slice).style(span.style));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, DiffPalette, Style};

    /// Colors every character, as a syntax highlighter would color a number.
    struct PaintEverything;

    impl TextAreaColorStrategy for PaintEverything {
        fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines {
            input
                .value
                .lines()
                .map(|line| {
                    line.chars()
                        .map(|ch| Span::new(ch.to_string()).style(Style::new().fg(Color::Red)))
                        .collect()
                })
                .collect()
        }

        fn cache_key(&self) -> u64 {
            1
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// Marks lines after an odd number of `'''` as string, like a lexer would.
    struct TripleQuoteLexer;

    impl TextAreaColorStrategy for TripleQuoteLexer {
        fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines {
            let mut in_string = false;
            input
                .value
                .lines()
                .map(|line| {
                    let style = if in_string {
                        Style::new().fg(Color::Green)
                    } else {
                        Style::default()
                    };
                    if line.contains("'''") {
                        in_string = !in_string;
                    }
                    vec![Span::new(line.to_string()).style(style)]
                })
                .collect()
        }

        fn cache_key(&self) -> u64 {
            2
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn separators_are_not_syntax_highlighted() {
        let line = |text: &str, kind| DiffRenderLine {
            prefix: Arc::from(""),
            text: Arc::from(text),
            kind,
            old_line: None,
            new_line: None,
            word_ranges: Vec::new(),
            context_separator: None,
            hunk: None,
        };
        let render = DiffRender::new(vec![
            line("x = 7", DiffLineKind::Context),
            line("↑↓ 7 hidden lines between ↑↓", DiffLineKind::Separator),
        ]);
        let palette = DiffPalette::default();
        let base: Rc<dyn TextAreaColorStrategy> = Rc::new(PaintEverything);
        let strategy = DiffColorStrategy::new(render, Some(base), palette, false, false);
        let lines = strategy.highlight(TextAreaColorInput {
            value: "",
            language: None,
            theme: None,
        });

        assert!(
            lines[0]
                .iter()
                .any(|s| s.style.fg == Some(Color::Red.into()))
        );
        let text: String = lines[1].iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "↑↓ 7 hidden lines between ↑↓");
        assert!(
            lines[1]
                .iter()
                .all(|s| s.style.fg != Some(Color::Red.into())),
            "{:?}",
            lines[1]
        );
    }

    #[test]
    fn each_patch_hunk_starts_from_a_fresh_syntax_state() {
        let patch = concat!(
            "@@ -40,2 +40,2 @@\n",
            " x = 1\n",
            " ''')\n",
            "@@ -97,2 +97,2 @@\n",
            "-print(a)\n",
            "+print(b)\n",
        );
        let data = super::super::DiffData::from_patch(patch);
        let base: Rc<dyn TextAreaColorStrategy> = Rc::new(TripleQuoteLexer);
        let strategy = DiffColorStrategy::new(
            data.unified,
            Some(base),
            DiffPalette::default(),
            false,
            false,
        );
        let lines = strategy.highlight(TextAreaColorInput {
            value: "",
            language: None,
            theme: None,
        });
        let in_string =
            |row: &Vec<Span>| row.iter().any(|s| s.style.fg == Some(Color::Green.into()));
        let texts: Vec<String> = lines
            .iter()
            .map(|row| row.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let print_b = texts
            .iter()
            .position(|t| t == "print(b)")
            .expect("{texts:?}");
        assert!(!in_string(&lines[print_b]), "{texts:?}");
        assert!(!in_string(&lines[print_b - 1]), "{texts:?}");
    }

    /// Distinguishes newline-terminated lines from an EOF-terminated line and
    /// emits a trailing blank line for a trailing `\n`, like `SyntectStrategy`.
    struct NewlineTerminatedLexer;

    impl TextAreaColorStrategy for NewlineTerminatedLexer {
        fn highlight(&self, input: TextAreaColorInput<'_>) -> TextAreaColorLines {
            let mut out: TextAreaColorLines = input
                .value
                .split_inclusive('\n')
                .map(|line| {
                    let style = if line.ends_with('\n') {
                        Style::new().fg(Color::Cyan)
                    } else {
                        Style::new().fg(Color::Yellow)
                    };
                    let text = line.strip_suffix('\n').unwrap_or(line);
                    vec![Span::new(text.to_string()).style(style)]
                })
                .collect();
            if input.value.ends_with('\n') {
                out.push(vec![Span::new("")]);
            }
            out
        }

        fn cache_key(&self) -> u64 {
            3
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn non_final_hunk_preserves_trailing_newline() {
        let patch = concat!(
            "@@ -10,2 +10,2 @@\n",
            " first_hunk_line_1\n",
            " first_hunk_last\n",
            "@@ -40,2 +40,2 @@\n",
            "-second_hunk_old\n",
            "+second_hunk_new\n",
        );
        let data = super::super::DiffData::from_patch(patch);
        let base: Rc<dyn TextAreaColorStrategy> = Rc::new(NewlineTerminatedLexer);
        let strategy = DiffColorStrategy::new(
            data.unified,
            Some(base),
            DiffPalette::default(),
            false,
            false,
        );
        let lines = strategy.highlight(TextAreaColorInput {
            value: "",
            language: None,
            theme: None,
        });
        let texts: Vec<String> = lines
            .iter()
            .map(|row| row.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let fg_of = |row: &[Span]| row.first().and_then(|s| s.style.fg);

        assert_eq!(
            texts,
            vec![
                "first_hunk_line_1",
                "first_hunk_last",
                "↑↓ 28 hidden lines between ↑↓",
                "second_hunk_old",
                "second_hunk_new",
            ]
        );
        assert_eq!(fg_of(&lines[0]), Some(Color::Cyan.into()));
        assert_eq!(fg_of(&lines[1]), Some(Color::Cyan.into()));
        assert_eq!(fg_of(&lines[2]), None);
        assert_eq!(fg_of(&lines[3]), Some(Color::Cyan.into()));
        assert_eq!(fg_of(&lines[4]), Some(Color::Yellow.into()));
    }

    #[test]
    fn full_width_anchor_uses_line_background_without_padding() {
        let render = DiffRender::new(vec![DiffRenderLine {
            prefix: Arc::from("1 + "),
            text: Arc::from("abc"),
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(1),
            word_ranges: vec![WordRange { start: 2, end: 3 }],
            context_separator: None,
            hunk: None,
        }]);

        let palette = DiffPalette {
            added: Style::new().bg(Color::rgb(0x10, 0x2A, 0x1E)),
            added_word: Style::new().bg(Color::rgb(0x16, 0x4E, 0x32)),
            ..DiffPalette::default()
        };
        let strategy = DiffColorStrategy::new(render, None, palette, true, false);
        let lines = strategy.highlight(TextAreaColorInput {
            value: "abc",
            language: None,
            theme: None,
        });

        let spans = &lines[0];
        assert!(spans.iter().any(|s| {
            s.content.is_empty() && s.style.bg == Some(Color::rgb(0x10, 0x2A, 0x1E).into())
        }));
        assert!(spans.iter().all(|s| s.content.len() < 200));
    }

    #[test]
    fn full_width_padding_is_present_when_enabled() {
        let render = DiffRender::new(vec![DiffRenderLine {
            prefix: Arc::from("1 + "),
            text: Arc::from("abc"),
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(1),
            word_ranges: Vec::new(),
            context_separator: None,
            hunk: None,
        }]);

        let palette = DiffPalette {
            added: Style::new().bg(Color::rgb(0x10, 0x2A, 0x1E)),
            ..DiffPalette::default()
        };
        let strategy = DiffColorStrategy::new(render, None, palette, true, true);
        let lines = strategy.highlight(TextAreaColorInput {
            value: "abc",
            language: None,
            theme: None,
        });

        assert!(
            lines[0]
                .iter()
                .any(|s| s.content.len() >= 512 && s.style.bg.is_some())
        );
    }
}
