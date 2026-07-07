use smallvec::{SmallVec, smallvec};

use crate::style::{Span, Style};

use super::{TextAreaDecoration, TextAreaDecorationKind};

/// Default resolver priority for normal Vim search matches.
pub(crate) const TEXT_AREA_LAYER_PRIORITY_SEARCH: u32 = 1_000;
/// Default resolver priority for the current Vim search match.
pub(crate) const TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH: u32 = 2_000;
/// Base resolver priority for public `TextAreaDecoration` overlays.
pub(crate) const TEXT_AREA_LAYER_PRIORITY_PUBLIC_DECORATION: u32 = 3_000;
/// Default resolver priority for selection overlays.
pub(crate) const TEXT_AREA_LAYER_PRIORITY_SELECTION: u32 = 4_000;

/// Internal source marker for a ranged text style layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TextAreaLayerKind {
    Search,
    CurrentSearch,
    PublicDecoration,
    Selection,
}

/// One ranged styling source in the TextArea resolver.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextAreaRangeLayer {
    pub(crate) ranges: SmallVec<[(usize, usize); 4]>,
    pub(crate) style: Style,
    pub(crate) priority: u32,
    pub(crate) kind: TextAreaLayerKind,
}

impl TextAreaRangeLayer {
    pub(crate) fn new(
        ranges: impl Into<SmallVec<[(usize, usize); 4]>>,
        style: Style,
        priority: u32,
        kind: TextAreaLayerKind,
    ) -> Self {
        Self {
            ranges: ranges.into(),
            style,
            priority,
            kind,
        }
    }

    pub(crate) fn single(
        range: (usize, usize),
        style: Style,
        priority: u32,
        kind: TextAreaLayerKind,
    ) -> Self {
        Self::new(smallvec![range], style, priority, kind)
    }
}

/// A byte-addressed styled slice of TextArea content.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextAreaStyledSegment<'a> {
    pub(crate) text: &'a str,
    pub(crate) style: Style,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn segments_from_plain<'a>(
    text: &'a str,
    style: Style,
    start: usize,
    end: usize,
) -> Vec<TextAreaStyledSegment<'a>> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![TextAreaStyledSegment {
        text,
        style,
        start,
        end,
    }]
}

pub(crate) fn segments_from_spans<'a>(
    spans: &'a [Span],
    line_start: usize,
    line_len: usize,
    range_start: usize,
    range_end: usize,
    base_style: Style,
) -> Option<Vec<TextAreaStyledSegment<'a>>> {
    let mut total_len = 0usize;
    for span in spans {
        total_len = total_len.saturating_add(span.content.len());
    }
    if total_len < line_len {
        return None;
    }

    let rel_start = range_start.saturating_sub(line_start);
    let rel_end = range_end.saturating_sub(line_start).min(total_len);

    if rel_start >= rel_end {
        return Some(Vec::new());
    }

    let mut segments = Vec::new();
    let mut cursor = 0usize;
    for span in spans {
        let span_start = cursor;
        let span_end = cursor.saturating_add(span.content.len());
        cursor = span_end;

        if rel_end <= span_start || rel_start >= span_end {
            continue;
        }

        let local_start = rel_start.max(span_start).saturating_sub(span_start);
        let local_end = rel_end.min(span_end).saturating_sub(span_start);
        let content = span.content.as_ref();
        let slice = &content[local_start..local_end];

        if slice.is_empty() {
            continue;
        }

        let segment_start = line_start
            .saturating_add(span_start)
            .saturating_add(local_start)
            .min(line_start.saturating_add(line_len));
        let segment_end = line_start
            .saturating_add(span_start)
            .saturating_add(local_end)
            .min(line_start.saturating_add(line_len));
        segments.push(TextAreaStyledSegment {
            text: slice,
            style: base_style.patch(span.style),
            start: segment_start,
            end: segment_end,
        });
    }

    Some(segments)
}

pub(crate) fn public_decoration_layers_for_visible_range(
    decorations: &[TextAreaDecoration],
    visible_start: usize,
    visible_end: usize,
    logical_line_start: usize,
    logical_line_end: usize,
) -> Vec<TextAreaRangeLayer> {
    if decorations.is_empty() || visible_start >= visible_end {
        return Vec::new();
    }
    let mut out = Vec::new();
    for decoration in decorations {
        let priority = TEXT_AREA_LAYER_PRIORITY_PUBLIC_DECORATION
            .saturating_add(u32::from(decoration.priority));
        match decoration.kind {
            TextAreaDecorationKind::Range | TextAreaDecorationKind::Underline => {
                let start = decoration.range.start.max(visible_start);
                let end = decoration.range.end.min(visible_end);
                if start < end {
                    let mut style = decoration.style;
                    if matches!(decoration.kind, TextAreaDecorationKind::Underline) {
                        style.underline = Some(true);
                    }
                    out.push(TextAreaRangeLayer::single(
                        (start, end),
                        style,
                        priority,
                        TextAreaLayerKind::PublicDecoration,
                    ));
                }
            }
            TextAreaDecorationKind::WholeLine => {
                if decoration.range.end > logical_line_start
                    && decoration.range.start < logical_line_end
                {
                    out.push(TextAreaRangeLayer::single(
                        (visible_start, visible_end),
                        decoration.style,
                        priority,
                        TextAreaLayerKind::PublicDecoration,
                    ));
                }
            }
        }
    }
    out
}

/// Resolve all ranged TextArea style sources using one priority-ordered fold.
///
/// Syntax/plain styles are represented by `base`. Search matches, current
/// search, public decorations, and selection are all peers in `layers`; current
/// behavior is reproduced by the default priority constants above.
pub(crate) fn resolve_text_area_spans<'a>(
    base: Vec<TextAreaStyledSegment<'a>>,
    layers: &[TextAreaRangeLayer],
) -> Vec<TextAreaStyledSegment<'a>> {
    if layers.is_empty() {
        return base;
    }

    let mut ordered: Vec<&TextAreaRangeLayer> = layers.iter().collect();
    ordered.sort_by_key(|layer| layer.priority);

    let mut segments = base;
    for layer in ordered {
        segments = overlay_segments(segments, &layer.ranges, layer.style);
    }
    segments
}

fn overlay_segments<'a>(
    segments: Vec<TextAreaStyledSegment<'a>>,
    overlay_ranges: &[(usize, usize)],
    overlay_style: Style,
) -> Vec<TextAreaStyledSegment<'a>> {
    if overlay_ranges.is_empty() {
        return segments;
    }

    let mut current = segments;
    for &(overlay_start, overlay_end) in overlay_ranges {
        if overlay_start >= overlay_end {
            continue;
        }
        let mut next = Vec::with_capacity(current.len().saturating_mul(2));
        for segment in current {
            if overlay_end <= segment.start || overlay_start >= segment.end {
                next.push(segment);
                continue;
            }

            let local_start = overlay_start
                .saturating_sub(segment.start)
                .min(segment.text.len());
            let local_end = overlay_end
                .saturating_sub(segment.start)
                .min(segment.text.len());

            if local_start > 0 {
                next.push(TextAreaStyledSegment {
                    text: &segment.text[..local_start],
                    style: segment.style,
                    start: segment.start,
                    end: segment.start + local_start,
                });
            }
            if local_end > local_start {
                next.push(TextAreaStyledSegment {
                    text: &segment.text[local_start..local_end],
                    style: segment.style.patch(overlay_style),
                    start: segment.start + local_start,
                    end: segment.start + local_end,
                });
            }
            if local_end < segment.text.len() {
                next.push(TextAreaStyledSegment {
                    text: &segment.text[local_end..],
                    style: segment.style,
                    start: segment.start + local_end,
                    end: segment.end,
                });
            }
        }
        current = next;
    }

    current
}

#[cfg(test)]
mod tests {
    use crate::style::{Color, Style};

    use super::*;

    fn fg(color: Color) -> Style {
        Style {
            fg: Some(color.into()),
            ..Style::default()
        }
    }

    fn bg(color: Color) -> Style {
        Style {
            bg: Some(color.into()),
            ..Style::default()
        }
    }

    fn underline_color(color: Color) -> Style {
        Style {
            underline_color: Some(color.into()),
            ..Style::default()
        }
    }

    #[test]
    fn resolver_applies_selection_after_public_decorations_by_default() {
        let base = segments_from_plain("abcd", Style::default(), 0, 4);
        let layers = vec![
            TextAreaRangeLayer::single(
                (1, 3),
                fg(Color::Red),
                TEXT_AREA_LAYER_PRIORITY_PUBLIC_DECORATION,
                TextAreaLayerKind::PublicDecoration,
            ),
            TextAreaRangeLayer::single(
                (2, 4),
                fg(Color::Blue),
                TEXT_AREA_LAYER_PRIORITY_SELECTION,
                TextAreaLayerKind::Selection,
            ),
        ];

        let resolved = resolve_text_area_spans(base, &layers);

        assert_eq!(
            resolved.iter().map(|s| s.text).collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        assert_eq!(resolved[1].style.fg, Some(Color::Red.into()));
        assert_eq!(resolved[2].style.fg, Some(Color::Blue.into()));
        assert_eq!(resolved[3].style.fg, Some(Color::Blue.into()));
    }

    #[test]
    fn resolver_preserves_search_current_search_decoration_selection_order() {
        let base = segments_from_plain("x", Style::default(), 0, 1);
        let layers = vec![
            TextAreaRangeLayer::single(
                (0, 1),
                fg(Color::Red),
                TEXT_AREA_LAYER_PRIORITY_SEARCH,
                TextAreaLayerKind::Search,
            ),
            TextAreaRangeLayer::single(
                (0, 1),
                fg(Color::Yellow),
                TEXT_AREA_LAYER_PRIORITY_CURRENT_SEARCH,
                TextAreaLayerKind::CurrentSearch,
            ),
            TextAreaRangeLayer::single(
                (0, 1),
                fg(Color::Green),
                TEXT_AREA_LAYER_PRIORITY_PUBLIC_DECORATION,
                TextAreaLayerKind::PublicDecoration,
            ),
            TextAreaRangeLayer::single(
                (0, 1),
                fg(Color::Blue),
                TEXT_AREA_LAYER_PRIORITY_SELECTION,
                TextAreaLayerKind::Selection,
            ),
        ];

        let resolved = resolve_text_area_spans(base, &layers);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].style.fg, Some(Color::Blue.into()));
    }

    #[test]
    fn resolver_merges_disjoint_attributes_across_layers() {
        // A diagnostic underline sits below a selection background. Because
        // composition is per-attribute (`Style::patch`), the higher-priority
        // selection must NOT erase the lower decoration's underline color: the
        // resolved segment carries both. This is the property that makes
        // squiggle-through-selection render correctly; a switch to full-style
        // replacement in `overlay_segments` would break it while the fg-only
        // ordering tests above stay green.
        let base = segments_from_plain("x", Style::default(), 0, 1);
        let layers = vec![
            TextAreaRangeLayer::single(
                (0, 1),
                underline_color(Color::Red),
                TEXT_AREA_LAYER_PRIORITY_PUBLIC_DECORATION,
                TextAreaLayerKind::PublicDecoration,
            ),
            TextAreaRangeLayer::single(
                (0, 1),
                bg(Color::Blue),
                TEXT_AREA_LAYER_PRIORITY_SELECTION,
                TextAreaLayerKind::Selection,
            ),
        ];

        let resolved = resolve_text_area_spans(base, &layers);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].style.bg, Some(Color::Blue.into()));
        assert_eq!(
            resolved[0].style.underline_color,
            Some(Color::Red.into()),
            "selection background must not erase the decoration underline color"
        );
    }

    #[test]
    fn underline_decorations_enable_underline_modifier() {
        let layers = public_decoration_layers_for_visible_range(
            &[TextAreaDecoration {
                range: 1..3,
                style: fg(Color::Red),
                priority: 0,
                kind: TextAreaDecorationKind::Underline,
            }],
            0,
            4,
            0,
            4,
        );

        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].ranges.as_slice(), &[(1, 3)]);
        assert_eq!(layers[0].style.fg, Some(Color::Red.into()));
        assert_eq!(layers[0].style.underline, Some(true));
    }
}
