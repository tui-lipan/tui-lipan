use crate::utils::text::{SentinelInfo, char_visual_width, is_wrap_break};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PreparedText {
    pub(crate) segments: Vec<Segment>,
    pub(crate) widths: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: SegmentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Text,
    Space,
    PreservedSpace,
    Tab,
    SoftBreak,
    HardBreak,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn prepare_text(
    s: &str,
    sentinel: Option<&SentinelInfo>,
    tab_stop: usize,
) -> PreparedText {
    let mut segments = Vec::new();
    let mut widths = Vec::new();
    let mut chars = s.char_indices().peekable();
    let mut col: usize = 0;

    while let Some((start, ch)) = chars.next() {
        if ch == '\r'
            && let Some((next_start, next_ch)) = chars.peek().copied()
            && next_start == start + ch.len_utf8()
            && next_ch == '\n'
        {
            let (_, consumed) = chars.next().expect("peeked item exists");
            segments.push(Segment {
                start,
                end: start + ch.len_utf8() + consumed.len_utf8(),
                kind: SegmentKind::HardBreak,
            });
            widths.push(0);
            col = 0;
            continue;
        }

        let end = start + ch.len_utf8();
        let kind = if ch == '\n' || ch == '\r' {
            SegmentKind::HardBreak
        } else if ch == '\t' {
            SegmentKind::Tab
        } else if ch == ' ' {
            SegmentKind::Space
        } else if ch.is_whitespace() {
            SegmentKind::PreservedSpace
        } else if is_wrap_break(ch) {
            SegmentKind::SoftBreak
        } else {
            SegmentKind::Text
        };
        let width = match kind {
            SegmentKind::HardBreak => 0,
            SegmentKind::Tab if tab_stop > 0 => tab_stop - (col % tab_stop),
            _ => char_visual_width(ch, sentinel),
        };

        segments.push(Segment { start, end, kind });
        widths.push(width);
        if kind == SegmentKind::HardBreak {
            col = 0;
        } else {
            col += width;
        }
    }

    PreparedText { segments, widths }
}

/// Segment kinds a wrapped row is allowed to end on.
fn is_break_kind(kind: SegmentKind) -> bool {
    matches!(
        kind,
        SegmentKind::Space
            | SegmentKind::PreservedSpace
            | SegmentKind::Tab
            | SegmentKind::SoftBreak
    )
}

/// Whether the break opportunity in front of the segment at `next_idx` is usable.
///
/// A run of separators (`": "`, `"::"`, `"  "`) offers a break after each one,
/// and a greedy scan would take the last that fits. Breaking inside the run
/// strands the rest of it on the next row: `"... dfd:"` keeps the full row and
/// the following `":"` or `" fdfd"` drops down alone. Only the separator that
/// ends the run is a usable break, so the whole word moves down at the previous
/// word break instead.
fn break_is_usable(pt: &PreparedText, next_idx: usize) -> bool {
    pt.segments
        .get(next_idx)
        .is_none_or(|next| !is_break_kind(next.kind))
}

/// Whether the token starting at `start` fits on a row of its own.
///
/// A token is a run of non-whitespace segments, separators included, so
/// `https://host/path` and `some.dotted.name` are single tokens. Splitting one
/// at an interior separator is a fallback for tokens too wide for any row: a
/// token that would fit on a fresh row moves there whole instead.
///
/// Scanning stops as soon as the budget is exceeded, so the extra work per row
/// is bounded by `wrap_width` rather than by the length of the token.
fn token_fits_on_a_row(pt: &PreparedText, start: usize, wrap_width: usize) -> bool {
    let mut width = 0usize;
    for (segment, segment_width) in pt.segments.iter().zip(pt.widths.iter()).skip(start) {
        if !matches!(segment.kind, SegmentKind::Text | SegmentKind::SoftBreak) {
            return true;
        }
        width = width.saturating_add(*segment_width);
        if width > wrap_width {
            return false;
        }
    }
    true
}

/// Counts the wrapped lines for `pt` at `width` without allocating.
///
/// Mirrors the control flow of [`layout_lines`] exactly but only tracks the
/// line count, so it is safe to call from hot measurement loops (e.g. ScrollView
/// child height probing). The `equivalence` tests guard the two paths against
/// drifting apart.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn count_lines(pt: &PreparedText, width: usize) -> usize {
    if pt.segments.is_empty() {
        return 1;
    }

    let wrap_width = width.max(1);
    let ends_with_hard_break = pt
        .segments
        .last()
        .is_some_and(|seg| seg.kind == SegmentKind::HardBreak);

    let mut count = 0usize;
    let mut idx = 0usize;

    while idx < pt.segments.len() {
        let seg = pt.segments[idx];
        if seg.kind == SegmentKind::HardBreak {
            count += 1;
            idx += 1;
            continue;
        }

        let mut used = 0usize;
        let mut cursor = idx;
        let mut token_start = idx;
        let mut last_word_break: Option<usize> = None;
        let mut last_token_break: Option<usize> = None;

        while cursor < pt.segments.len() {
            let cur = pt.segments[cursor];
            if cur.kind == SegmentKind::HardBreak {
                break;
            }

            let cw = pt.widths[cursor];
            if used.saturating_add(cw) > wrap_width {
                break;
            }
            used = used.saturating_add(cw);
            cursor += 1;

            match cur.kind {
                SegmentKind::Space | SegmentKind::PreservedSpace | SegmentKind::Tab => {
                    token_start = cursor;
                    last_token_break = None;
                    if break_is_usable(pt, cursor) {
                        last_word_break = Some(cursor);
                    }
                }
                SegmentKind::SoftBreak if break_is_usable(pt, cursor) => {
                    last_token_break = Some(cursor);
                }
                _ => {}
            }
        }

        if cursor >= pt.segments.len() {
            count += 1;
            break;
        }

        if pt.segments[cursor].kind == SegmentKind::HardBreak {
            count += 1;
            idx = cursor + 1;
            continue;
        }

        let split_token = last_token_break
            .filter(|_| !token_fits_on_a_row(pt, token_start, wrap_width))
            .is_some();
        let chosen = if split_token {
            last_token_break
        } else {
            last_word_break
        };

        if let Some(next_idx) = chosen {
            count += 1;
            idx = next_idx;
            continue;
        }

        count += 1;
        idx = if cursor > idx { cursor } else { idx + 1 };
    }

    if ends_with_hard_break {
        count += 1;
    }

    count
}

pub(crate) fn layout_lines(pt: &PreparedText, width: usize) -> Vec<LineRange> {
    if pt.segments.is_empty() {
        return vec![LineRange { start: 0, end: 0 }];
    }

    let wrap_width = width.max(1);
    let text_len = pt.segments.last().map_or(0, |seg| seg.end);
    let ends_with_hard_break = pt
        .segments
        .last()
        .is_some_and(|seg| seg.kind == SegmentKind::HardBreak);

    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut line_start = 0usize;

    while idx < pt.segments.len() {
        let seg = pt.segments[idx];
        if seg.kind == SegmentKind::HardBreak {
            out.push(LineRange {
                start: line_start,
                end: seg.start,
            });
            line_start = seg.end;
            idx += 1;
            continue;
        }

        let mut used = 0usize;
        let mut cursor = idx;
        let mut token_start = idx;
        let mut last_word_break: Option<(usize, usize)> = None;
        let mut last_token_break: Option<(usize, usize)> = None;

        while cursor < pt.segments.len() {
            let cur = pt.segments[cursor];
            if cur.kind == SegmentKind::HardBreak {
                break;
            }

            let cw = pt.widths[cursor];
            if used.saturating_add(cw) > wrap_width {
                break;
            }
            used = used.saturating_add(cw);
            cursor += 1;

            match cur.kind {
                SegmentKind::Space | SegmentKind::PreservedSpace | SegmentKind::Tab => {
                    token_start = cursor;
                    last_token_break = None;
                    if break_is_usable(pt, cursor) {
                        last_word_break = Some((cursor, cur.end));
                    }
                }
                SegmentKind::SoftBreak if break_is_usable(pt, cursor) => {
                    last_token_break = Some((cursor, cur.end));
                }
                _ => {}
            }
        }

        if cursor >= pt.segments.len() {
            out.push(LineRange {
                start: line_start,
                end: text_len,
            });
            break;
        }

        let cur = pt.segments[cursor];
        if cur.kind == SegmentKind::HardBreak {
            out.push(LineRange {
                start: line_start,
                end: cur.start,
            });
            line_start = cur.end;
            idx = cursor + 1;
            continue;
        }

        // Split the token at an interior separator only when it cannot fit a row
        // of its own; otherwise fall back to the last word break so the whole
        // token moves down intact.
        let split_token = last_token_break
            .filter(|_| !token_fits_on_a_row(pt, token_start, wrap_width))
            .is_some();
        let chosen = if split_token {
            last_token_break
        } else {
            last_word_break
        };

        if let Some((next_idx, end)) = chosen {
            out.push(LineRange {
                start: line_start,
                end,
            });
            line_start = end;
            idx = next_idx;
            continue;
        }

        let forced_end = if cursor > idx {
            pt.segments[cursor - 1].end
        } else {
            pt.segments[idx].end
        };
        out.push(LineRange {
            start: line_start,
            end: forced_end,
        });
        line_start = forced_end;
        idx = if cursor > idx { cursor } else { idx + 1 };
    }

    if ends_with_hard_break {
        out.push(LineRange {
            start: text_len,
            end: text_len,
        });
    }

    out
}

/// Lays out text while reserving the final cell of an exactly full caret row.
///
/// The regular width still applies to every other visual row. Only the row
/// ending at `caret` is reflowed, avoiding both an off-screen caret and a
/// synthetic empty continuation row. A trailing separator already occupying
/// the final cell stays in place and gives the caret a real continuation row.
pub(crate) fn layout_lines_with_caret(
    pt: &PreparedText,
    width: usize,
    caret: usize,
) -> (Vec<LineRange>, bool) {
    let mut lines = layout_lines(pt, width);
    let Some(idx) = lines.iter().enumerate().find_map(|(idx, line)| {
        let continues_at_caret = lines.get(idx + 1).is_some_and(|next| next.start == caret);
        (line.end == caret && !continues_at_caret).then_some(idx)
    }) else {
        return (lines, false);
    };

    let line = lines[idx];
    let row_width = pt
        .segments
        .iter()
        .zip(pt.widths.iter())
        .filter(|(segment, _)| {
            segment.kind != SegmentKind::HardBreak
                && segment.start >= line.start
                && segment.end <= line.end
        })
        .map(|(_, width)| *width)
        .fold(0usize, usize::saturating_add);
    if row_width != width.max(1) {
        return (lines, false);
    }

    let ends_with_separator = pt.segments.iter().rev().find_map(|segment| {
        (segment.kind != SegmentKind::HardBreak && segment.end <= line.end).then_some(segment.kind)
    });
    if width <= 1
        || matches!(
            ends_with_separator,
            Some(SegmentKind::Space | SegmentKind::PreservedSpace | SegmentKind::Tab)
        )
    {
        lines.insert(
            idx + 1,
            LineRange {
                start: caret,
                end: caret,
            },
        );
        return (lines, true);
    }

    let mut row = PreparedText::default();
    for (segment, segment_width) in pt.segments.iter().zip(pt.widths.iter()) {
        if segment.kind == SegmentKind::HardBreak
            || segment.start < line.start
            || segment.end > line.end
        {
            continue;
        }
        row.segments.push(Segment {
            start: segment.start - line.start,
            end: segment.end - line.start,
            kind: segment.kind,
        });
        row.widths.push(*segment_width);
    }

    let replacement = layout_lines(&row, width - 1)
        .into_iter()
        .map(|range| LineRange {
            start: line.start + range.start,
            end: line.start + range.end,
        })
        .collect::<Vec<_>>();
    lines.splice(idx..=idx, replacement);
    (lines, true)
}

#[cfg(test)]
mod tests {
    use super::{LineRange, SegmentKind, count_lines, layout_lines, prepare_text};
    use crate::utils::text::SentinelInfo;

    #[test]
    fn tokenizes_spaces_tabs_soft_and_hard_breaks() {
        let s = "a b\t-\n\u{2003}z";
        let pt = prepare_text(s, None, 4);

        assert_eq!(pt.segments.len(), 8);
        assert_eq!(pt.widths.len(), 8);
        assert_eq!(pt.segments[0].kind, SegmentKind::Text);
        assert_eq!(pt.segments[1].kind, SegmentKind::Space);
        assert_eq!(pt.segments[2].kind, SegmentKind::Text);
        assert_eq!(pt.segments[3].kind, SegmentKind::Tab);
        assert_eq!(pt.segments[4].kind, SegmentKind::SoftBreak);
        assert_eq!(pt.segments[5].kind, SegmentKind::HardBreak);
        assert_eq!(pt.segments[6].kind, SegmentKind::PreservedSpace);
        assert_eq!(pt.segments[7].kind, SegmentKind::Text);
    }

    #[test]
    fn applies_sentinel_width_when_preparing() {
        let sentinel = SentinelInfo {
            image: Some((0xE000, 0xE001, 5)),
            custom: None,
        };
        let s = "x\u{E000}y";
        let pt = prepare_text(s, Some(&sentinel), 4);

        assert_eq!(pt.widths, vec![1, 5, 1]);
        assert_eq!(pt.segments[1].kind, SegmentKind::Text);
    }

    #[test]
    fn layout_respects_hard_breaks_and_trailing_newline() {
        let s = "ab\ncd\n";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 80);
        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 2 },
                LineRange { start: 3, end: 5 },
                LineRange { start: 6, end: 6 },
            ]
        );
        assert_eq!(count_lines(&pt, 80), 3);
    }

    #[test]
    fn layout_uses_fallback_for_unbreakable_runs() {
        let s = "abcdef";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 3);
        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 3 },
                LineRange { start: 3, end: 6 },
            ]
        );
        assert_eq!(count_lines(&pt, 3), 2);
    }

    #[test]
    fn overflowing_trailing_separator_reuses_previous_word_break() {
        let s = "hello word ";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 10);

        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 6 },
                LineRange { start: 6, end: 11 },
            ]
        );
        assert_eq!(count_lines(&pt, 10), lines.len());
    }

    #[test]
    fn overflowing_trailing_separator_reuses_unicode_word_break() {
        let s = "ab \u{4F60} ";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 5);

        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 3 },
                LineRange { start: 3, end: 7 },
            ]
        );
        assert_eq!(count_lines(&pt, 5), lines.len());
    }

    #[test]
    fn token_that_fits_a_fresh_row_moves_down_whole() {
        // The URL does not fit in what is left of row 1, but does fit a row of
        // its own, so it moves down intact instead of splitting at a separator.
        let s = "testing https://chatgpt.com/c/6a8828df-6d0a9f7dfe5b";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 45)
            .into_iter()
            .map(|line| &s[line.start..line.end])
            .collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec!["testing ", "https://chatgpt.com/c/6a8828df-6d0a9f7dfe5b"]
        );
        assert_eq!(count_lines(&pt, 45), lines.len());
    }

    #[test]
    fn token_wider_than_a_row_still_splits_at_separators() {
        let s = "testing https://chatgpt.com/c/6a8828df-6d0a9f7dfe5b";
        let pt = prepare_text(s, None, 40);
        let lines = layout_lines(&pt, 40)
            .into_iter()
            .map(|line| &s[line.start..line.end])
            .collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec!["testing https://chatgpt.com/c/6a8828df-", "6d0a9f7dfe5b"]
        );
        assert_eq!(count_lines(&pt, 40), lines.len());
    }

    #[test]
    fn separator_run_at_row_end_moves_the_whole_word_down() {
        // "hello word:" fills the row exactly, so the greedy scan used to break
        // after the ':' and strand the rest of the run on its own row.
        for (s, expected) in [
            ("hello word::", vec![(0usize, 6usize), (6, 12)]),
            ("hello word: x", vec![(0, 6), (6, 13)]),
            ("hello word.. ", vec![(0, 6), (6, 13)]),
        ] {
            let pt = prepare_text(s, None, 4);
            let lines = layout_lines(&pt, 11)
                .into_iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>();
            assert_eq!(lines, expected, "unexpected layout for {s:?}");
            assert_eq!(count_lines(&pt, 11), lines.len(), "count for {s:?}");
        }
    }

    #[test]
    fn separator_still_breaks_unbreakable_runs() {
        for (s, width, expected) in [
            ("aaa/bbb/ccc", 8usize, vec![(0usize, 8usize), (8, 11)]),
            ("a.b.c.d.e.f", 6, vec![(0, 6), (6, 11)]),
            ("hello aaa/bbb/ccc", 10, vec![(0, 10), (10, 17)]),
        ] {
            let pt = prepare_text(s, None, 4);
            let lines = layout_lines(&pt, width)
                .into_iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>();
            assert_eq!(lines, expected, "unexpected layout for {s:?}");
            assert_eq!(count_lines(&pt, width), lines.len(), "count for {s:?}");
        }
    }

    #[test]
    fn count_lines_matches_layout_lines_across_widths() {
        let samples = [
            "",
            "a",
            "hello world",
            "ab\ncd\n",
            "\n\n\n",
            "abcdef",
            "the quick brown fox jumps over the lazy dog",
            "one two\tthree\nfour five six seven\n\n",
            "\u{4F60}\u{597D}a b\u{4F60}",
            "trailing  spaces   ",
            "word-with-hyphens-that-are-long break here",
            "hello word:: more",
            "path/to/some/file.rs and more text",
            "colon: dot. slash/ pipe| dash- under_ run",
            "testing https://chatgpt.com/c/6a8828df-6d0a9f7dfe5b",
            "a b/c d well-known some.dotted.name end",
        ];
        for s in samples {
            let pt = prepare_text(s, None, 4);
            for width in [1usize, 2, 3, 5, 8, 13, 80, 200] {
                assert_eq!(
                    count_lines(&pt, width),
                    layout_lines(&pt, width).len(),
                    "mismatch for {s:?} at width {width}",
                );
            }
        }
    }

    #[test]
    fn layout_forces_single_wide_glyph_when_width_too_small() {
        let s = "\u{4F60}a";
        let pt = prepare_text(s, None, 4);
        let lines = layout_lines(&pt, 1);
        assert_eq!(
            lines,
            vec![
                LineRange { start: 0, end: 3 },
                LineRange { start: 3, end: 4 },
            ]
        );
    }
}
