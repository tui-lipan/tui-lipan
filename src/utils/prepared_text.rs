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
        let mut last_break: Option<usize> = None;

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

            if matches!(
                cur.kind,
                SegmentKind::Space
                    | SegmentKind::PreservedSpace
                    | SegmentKind::Tab
                    | SegmentKind::SoftBreak
            ) {
                last_break = Some(cursor);
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

        if let Some(next_idx) = last_break {
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
        let mut last_break: Option<(usize, usize)> = None;

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

            if matches!(
                cur.kind,
                SegmentKind::Space
                    | SegmentKind::PreservedSpace
                    | SegmentKind::Tab
                    | SegmentKind::SoftBreak
            ) {
                last_break = Some((cursor, cur.end));
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

        if let Some((next_idx, end)) = last_break {
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
