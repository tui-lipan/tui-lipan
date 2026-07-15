use std::sync::Arc;

use crate::style::{RowStylePolicy, Span, Style};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::{ListItem, ListItemGutter, ListItemLine, ListItemStatus};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::matching::SearchResult;
use super::{DescriptionOverflow, DescriptionPlacement, SearchEntry, SearchHighlight, SearchItem};

pub(super) type RenderFn<'a, T> = &'a dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItem>;
pub(super) type GutterRenderFn<'a, T> =
    &'a dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItemGutter>;
pub(super) type StatusRenderFn<'a, T> =
    &'a dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItemStatus>;

#[derive(Clone)]
pub(super) struct RenderStyles {
    pub item: Style,
    /// Style for active items' label spans. Falls back to `item` when `None`.
    pub active_item: Option<Style>,
    pub description: Style,
    /// Style for active items' description spans. Falls back to `description` when `None`.
    pub active_description: Option<Style>,
    /// Style for the focused (selected) item's description spans. Falls back to
    /// `description` when `None`. Takes precedence over `active_description`.
    pub focused_description: Option<Style>,
    pub description_placement: DescriptionPlacement,
    pub description_separator: Option<Arc<str>>,
    pub description_selection: bool,
    pub description_overflow: DescriptionOverflow,
    pub line_width: Option<u16>,
    pub highlight: Style,
}

#[derive(Clone, Copy)]
pub(super) struct ScoreRender {
    pub show: bool,
    pub gradient: Option<ColorGradient>,
    pub range: Option<GradientRange>,
}

/// Output of `build_list_items`.
pub(super) struct ListItemsOutput {
    /// The list items to display (includes headers and spacers when applicable).
    pub items: Vec<ListItem>,
    /// Maps result index → visual row index in `items`.
    /// `result_to_row[i]` is the row in `items` that corresponds to `results[i]`.
    pub result_to_row: Vec<usize>,
    /// Maps visual row index → result index.
    /// `None` for rows that are headers or spacers.
    pub row_to_result: Vec<Option<usize>>,
}

pub(super) struct SearchListItemsCtx<'a, T> {
    pub renderer: Option<RenderFn<'a, T>>,
    pub status_renderer: Option<StatusRenderFn<'a, T>>,
    pub gutter_renderer: Option<GutterRenderFn<'a, T>>,
    pub styles: &'a RenderStyles,
    pub score: ScoreRender,
    pub selected_result_index: Option<usize>,
    /// Style applied to section header rows.
    pub header_style: Style,
}

pub(super) fn build_list_items<T>(
    items: &[SearchItem<T>],
    entries: &[SearchEntry<T>],
    results: &[SearchResult],
    ctx: SearchListItemsCtx<'_, T>,
) -> ListItemsOutput {
    let SearchListItemsCtx {
        renderer,
        status_renderer,
        gutter_renderer,
        styles,
        score,
        selected_result_index,
        header_style,
    } = ctx;
    let score = ScoreRender {
        range: score.range.or_else(|| {
            let min = results.iter().map(|r| r.score).min()? as u64;
            let max = results.iter().map(|r| r.score).max()? as u64;
            Some(GradientRange::new(min, max))
        }),
        ..score
    };

    // If no entries (headers/spacers) are defined, render results directly -
    // visual row index == result index.
    if entries.is_empty() {
        let list_items: Vec<ListItem> = results
            .iter()
            .enumerate()
            .filter_map(|(result_idx, result)| {
                render_result(
                    result_idx,
                    result,
                    items,
                    SearchResultRenderCtx {
                        renderer,
                        status_renderer,
                        gutter_renderer,
                        styles,
                        score,
                        selected_result_index,
                    },
                )
            })
            .collect();
        let len = list_items.len();
        return ListItemsOutput {
            items: list_items,
            result_to_row: (0..len).collect(),
            row_to_result: (0..len).map(Some).collect(),
        };
    }

    // Build a set of matched item indices for quick lookup.
    let matched: std::collections::HashSet<usize> = results.iter().map(|r| r.item_index).collect();

    // Map each searchable item's position in `items` to its result index.
    let result_idx_by_item: std::collections::HashMap<usize, usize> = results
        .iter()
        .enumerate()
        .map(|(ri, r)| (r.item_index, ri))
        .collect();

    // Walk entries in order. For each group (items following a header), emit
    // the header only if at least one item in the group matched.
    // Spacers are deferred: a pending spacer is only flushed immediately before
    // the first matched item of the next group, so spacers between two groups
    // that both have matches are preserved, while spacers adjacent to an empty
    // group are suppressed.
    let mut list_items: Vec<ListItem> = Vec::new();
    let mut row_to_result: Vec<Option<usize>> = Vec::new();
    let mut result_to_row: Vec<usize> = vec![0; results.len()];

    let mut item_idx: usize = 0; // tracks position within `items`
    let mut pending_header: Option<Arc<str>> = None;
    let mut pending_spacer: bool = false;

    for entry in entries {
        match entry {
            SearchEntry::Header(label) => {
                pending_header = Some(label.clone());
            }
            SearchEntry::Spacer => {
                // Only queue a spacer if there is already content above it.
                if !list_items.is_empty() {
                    pending_spacer = true;
                }
            }
            SearchEntry::Item(_) => {
                if matched.contains(&item_idx) {
                    // Flush the pending spacer and/or header before the first
                    // match in this group. The spacer comes first so the header
                    // follows it directly.
                    if pending_spacer {
                        list_items.push(ListItem::spacer());
                        row_to_result.push(None);
                        pending_spacer = false;
                    }
                    if let Some(label) = pending_header.take() {
                        list_items.push(ListItem::header(label).style(header_style));
                        row_to_result.push(None);
                    }
                    if let Some(&result_idx) = result_idx_by_item.get(&item_idx) {
                        let row = list_items.len();
                        result_to_row[result_idx] = row;
                        if let Some(li) = render_result(
                            result_idx,
                            &results[result_idx],
                            items,
                            SearchResultRenderCtx {
                                renderer,
                                status_renderer,
                                gutter_renderer,
                                styles,
                                score,
                                selected_result_index,
                            },
                        ) {
                            list_items.push(li);
                            row_to_result.push(Some(result_idx));
                        }
                    }
                }
                item_idx += 1;
            }
        }
    }

    ListItemsOutput {
        items: list_items,
        result_to_row,
        row_to_result,
    }
}

pub(super) struct SearchResultRenderCtx<'a, T> {
    pub renderer: Option<RenderFn<'a, T>>,
    pub status_renderer: Option<StatusRenderFn<'a, T>>,
    pub gutter_renderer: Option<GutterRenderFn<'a, T>>,
    pub styles: &'a RenderStyles,
    pub score: ScoreRender,
    pub selected_result_index: Option<usize>,
}

fn render_result<T>(
    result_idx: usize,
    result: &SearchResult,
    items: &[SearchItem<T>],
    ctx: SearchResultRenderCtx<'_, T>,
) -> Option<ListItem> {
    let SearchResultRenderCtx {
        renderer,
        status_renderer,
        gutter_renderer,
        styles,
        score,
        selected_result_index,
    } = ctx;
    let item = items.get(result.item_index)?;
    let highlight = SearchHighlight {
        label_hits: result.label_hits.clone(),
        description_hits: result.description_hits.clone(),
        description_right_hits: result.description_right_hits.clone(),
        score: result.score,
    };
    let mut rendered = if let Some(renderer) = renderer
        && let Some(li) = (renderer)(item, &highlight)
    {
        li
    } else {
        default_render(
            result_idx,
            item,
            &highlight,
            styles,
            result.score,
            score,
            selected_result_index,
        )
    };

    if let Some(status_renderer) = status_renderer
        && let Some(status) = status_renderer(item, &highlight)
    {
        rendered = rendered.status(status);
    }

    if let Some(gutter_renderer) = gutter_renderer
        && let Some(gutter) = gutter_renderer(item, &highlight)
    {
        let gutter_line = rendered.symbol_line;
        rendered = rendered.gutter(gutter).gutter_line(gutter_line);
    }

    Some(rendered)
}

fn default_render<T>(
    result_idx: usize,
    item: &SearchItem<T>,
    highlight: &SearchHighlight,
    styles: &RenderStyles,
    score_value: u32,
    score: ScoreRender,
    selected_result_index: Option<usize>,
) -> ListItem {
    let label_style = if item.active {
        match styles.active_item {
            Some(s) => styles.item.patch(s),
            None => styles.item,
        }
    } else {
        styles.item
    };
    let mut label_spans = highlight_spans(
        &item.label,
        &highlight.label_hits,
        label_style,
        styles.highlight,
    );

    let is_focused = selected_result_index == Some(result_idx);
    let explicit_focused_description = if is_focused {
        styles.focused_description
    } else {
        None
    };
    let explicit_active_description = if !is_focused && item.active {
        styles.active_description
    } else {
        None
    };
    let explicit_description_override =
        explicit_focused_description.or(explicit_active_description);
    let description_style = explicit_description_override.unwrap_or(styles.description);
    let description_row_policy = if explicit_description_override.is_none() {
        RowStylePolicy::Full
    } else {
        RowStylePolicy::Disabled
    };

    let description_spans = item
        .description
        .as_ref()
        .and_then(|d| d.left.as_ref())
        .map(|desc| {
            let spans = highlight_spans(
                desc,
                &highlight.description_hits,
                description_style,
                styles.highlight,
            );
            if explicit_description_override.is_some() {
                disable_row_style_for_spans(spans)
            } else {
                spans
            }
        });
    let description_right = item.description.as_ref().and_then(|d| d.right.clone());

    let mut score_right_spans = Vec::new();
    if score.show {
        let mut score_style = description_style;
        if let (Some(gradient), Some(range)) = (score.gradient, score.range) {
            score_style =
                score_style.patch(Style::new().fg(gradient.color_for(score_value as u64, range)));
        }
        let score_span = Span::new(format!("{:>4}", score_value))
            .style(score_style)
            .row_style_policy(description_row_policy);
        score_right_spans.push(score_span);
    }
    let score_width = spans_width(&score_right_spans) as u16;

    let mut primary_right_spans: Vec<Span> = Vec::new();
    let mut primary_right_highlight = explicit_description_override.is_none();
    let mut primary_right_hover = explicit_description_override.is_none();
    let mut primary_truncate_description_first = false;
    let mut top_lines: Vec<ListItemLine> = Vec::new();
    let mut bottom_lines: Vec<ListItemLine> = Vec::new();

    if let Some(desc_spans) = description_spans {
        let overflow = effective_description_overflow(
            styles.description_placement,
            styles.description_overflow,
        );

        match styles.description_placement {
            DescriptionPlacement::Inline => {
                let sep = styles.description_separator.as_deref().unwrap_or(" - ");
                if let Some(width) = styles.line_width {
                    let left_budget = width.saturating_sub(score_width);
                    label_spans = inline_truncate(
                        label_spans,
                        desc_spans,
                        left_budget,
                        description_style,
                        sep,
                    );
                } else {
                    label_spans.push(
                        Span::new(sep)
                            .style(description_style)
                            .row_style_policy(description_row_policy),
                    );
                    label_spans.extend(desc_spans);
                }
            }
            DescriptionPlacement::Right => {
                primary_truncate_description_first = true;
                let mut desc_right = desc_spans;
                if let Some(width) = styles.line_width {
                    let label_width = spans_width(&label_spans) as u16;
                    let mut desc_budget = width.saturating_sub(label_width);
                    if score_width > 0 {
                        desc_budget = desc_budget.saturating_sub(score_width.saturating_add(2));
                    }
                    desc_right = truncate_spans_with_ellipsis(&desc_right, desc_budget);
                }
                if !desc_right.is_empty() {
                    primary_right_spans.push(
                        Span::new(" ")
                            .style(description_style)
                            .row_style_policy(description_row_policy),
                    );
                    primary_right_spans.extend(desc_right);
                    primary_right_highlight = explicit_description_override.is_none();
                    primary_right_hover = explicit_description_override.is_none();
                }
            }
            DescriptionPlacement::Above | DescriptionPlacement::Below => {
                let target = if matches!(styles.description_placement, DescriptionPlacement::Above)
                {
                    &mut top_lines
                } else {
                    &mut bottom_lines
                };
                target.push(
                    ListItemLine::from_spans(desc_spans)
                        .style(description_style)
                        .selection_label(
                            styles.description_selection && explicit_description_override.is_none(),
                        )
                        .selection_description(
                            styles.description_selection && explicit_description_override.is_none(),
                        )
                        .hover_label(
                            styles.description_selection && explicit_description_override.is_none(),
                        )
                        .hover_description(
                            styles.description_selection && explicit_description_override.is_none(),
                        )
                        .wrap_label(matches!(overflow, DescriptionOverflow::Wrap)),
                );
            }
        }
    }

    if let Some(right_text) = description_right.as_ref() {
        if !primary_right_spans.is_empty() {
            primary_right_spans.push(
                Span::new("  ")
                    .style(description_style)
                    .row_style_policy(description_row_policy),
            );
        } else {
            primary_right_spans.push(
                Span::new(" ")
                    .style(description_style)
                    .row_style_policy(description_row_policy),
            );
        }
        let desc_right_spans = highlight_spans(
            right_text,
            &highlight.description_right_hits,
            description_style,
            styles.highlight,
        );
        if !desc_right_spans.is_empty() {
            primary_truncate_description_first = true;
        }
        if explicit_description_override.is_some() {
            primary_right_spans.extend(disable_row_style_for_spans(desc_right_spans));
        } else {
            primary_right_spans.extend(desc_right_spans);
        }
    }

    if !score_right_spans.is_empty() {
        if !primary_right_spans.is_empty() {
            primary_right_spans.push(
                Span::new("  ")
                    .style(description_style)
                    .row_style_policy(description_row_policy),
            );
        }
        primary_right_spans.extend(score_right_spans);
    }

    if matches!(styles.description_placement, DescriptionPlacement::Right)
        && let Some(width) = styles.line_width
    {
        let label_width = spans_width(&label_spans) as u16;
        let right_budget = width.saturating_sub(label_width);
        primary_right_spans = truncate_spans_with_ellipsis(&primary_right_spans, right_budget);
    }

    if !top_lines.is_empty() {
        let symbol_line = top_lines.len();
        let mut top_iter = top_lines.into_iter();
        let first = top_iter.next().expect("non-empty");
        let mut list_item = ListItem::from_spans(first.spans)
            .style(styles.item)
            .active(item.active)
            .primary_selection_label(first.selection_label)
            .primary_selection_description(first.selection_description)
            .primary_hover_label(first.hover_label)
            .primary_hover_description(first.hover_description)
            .primary_wrap_label(first.wrap_label)
            .symbol_line(symbol_line);
        if !first.description_spans.is_empty() {
            list_item = list_item.description_spans(first.description_spans);
        }

        for line in top_iter {
            list_item = list_item.line(line);
        }

        list_item = list_item.line(
            ListItemLine::from_spans(label_spans)
                .style(styles.item)
                .description_spans(primary_right_spans)
                .selection_label(true)
                .selection_description(primary_right_highlight)
                .hover_label(true)
                .hover_description(primary_right_hover)
                .truncate_description_first(primary_truncate_description_first),
        );

        for line in bottom_lines {
            list_item = list_item.line(line);
        }

        return list_item;
    }

    let mut list_item = ListItem::from_spans(label_spans)
        .active(item.active)
        .primary_selection_description(primary_right_highlight)
        .primary_hover_description(primary_right_hover)
        .primary_truncate_description_first(primary_truncate_description_first);
    if !primary_right_spans.is_empty() {
        list_item = list_item.description_spans(primary_right_spans);
    }
    for line in bottom_lines {
        list_item = list_item.line(line);
    }

    list_item
}

fn effective_description_overflow(
    placement: DescriptionPlacement,
    overflow: DescriptionOverflow,
) -> DescriptionOverflow {
    match placement {
        DescriptionPlacement::Above | DescriptionPlacement::Below => overflow,
        DescriptionPlacement::Inline | DescriptionPlacement::Right => DescriptionOverflow::Truncate,
    }
}

fn spans_width(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn inline_truncate(
    label_spans: Vec<Span>,
    desc_spans: Vec<Span>,
    line_budget: u16,
    description_style: Style,
    separator: &str,
) -> Vec<Span> {
    let label_width = spans_width(&label_spans) as u16;
    let separator_width = UnicodeWidthStr::width(separator) as u16;
    let separator = Span::new(separator).style(description_style);

    if line_budget <= label_width {
        return truncate_spans_with_ellipsis(&label_spans, line_budget);
    }

    if line_budget <= label_width.saturating_add(separator_width) {
        return truncate_spans_with_ellipsis(&label_spans, line_budget);
    }

    let mut out = label_spans;
    out.push(separator);
    let desc_budget = line_budget.saturating_sub(label_width.saturating_add(separator_width));
    out.extend(truncate_spans_with_ellipsis(&desc_spans, desc_budget));
    out
}

fn truncate_spans_with_ellipsis(spans: &[Span], max_width: u16) -> Vec<Span> {
    if max_width == 0 {
        return Vec::new();
    }

    if spans_width(spans) <= max_width as usize {
        return spans.to_vec();
    }

    let ellipsis = "…";
    let ellipsis_width = UnicodeWidthStr::width(ellipsis) as u16;
    if max_width <= ellipsis_width {
        let style = spans.last().map(|span| span.style).unwrap_or_default();
        return vec![Span::new(ellipsis).style(style)];
    }

    let target = max_width.saturating_sub(ellipsis_width);
    let (mut prefix, _) = take_prefix_spans(spans, target);
    let style = prefix
        .last()
        .map(|span| span.style)
        .or_else(|| spans.last().map(|span| span.style))
        .unwrap_or_default();
    prefix.push(Span::new(ellipsis).style(style));
    prefix
}

fn take_prefix_spans(spans: &[Span], max_width: u16) -> (Vec<Span>, Vec<Span>) {
    if max_width == 0 {
        return (Vec::new(), spans.to_vec());
    }

    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    let mut used = 0usize;
    let max = max_width as usize;

    for (index, span) in spans.iter().enumerate() {
        if used >= max {
            suffix.push(span.clone());
            continue;
        }

        let text = span.content.as_ref();
        let span_width = UnicodeWidthStr::width(text);
        if used + span_width <= max {
            prefix.push(span.clone());
            used += span_width;
            continue;
        }

        let mut split_byte = 0usize;
        let mut local_width = 0usize;
        for (byte, ch) in text.char_indices() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + local_width + cw > max {
                break;
            }
            local_width += cw;
            split_byte = byte + ch.len_utf8();
        }

        if split_byte == 0
            && prefix.is_empty()
            && let Some(ch) = text.chars().next()
        {
            split_byte = ch.len_utf8();
        }

        if split_byte > 0 {
            let head = &text[..split_byte];
            prefix.push(Span::new(head.to_owned()).style(span.style));
        }

        if split_byte < text.len() {
            let tail = &text[split_byte..];
            if !tail.is_empty() {
                suffix.push(Span::new(tail.to_owned()).style(span.style));
            }
        }

        suffix.extend(spans.iter().skip(index + 1).cloned());
        break;
    }

    (prefix, suffix)
}

fn highlight_spans(
    text: &str,
    hits: &[u32],
    base_style: Style,
    selection_style: Style,
) -> Vec<Span> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut hits = hits.to_vec();
    hits.sort_unstable();
    hits.dedup();

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_highlight = false;
    let mut hit_idx = 0;
    let highlight_policy = if selection_style.fg.is_some() || selection_style.fg_transform.is_some()
    {
        RowStylePolicy::PreserveForeground
    } else {
        RowStylePolicy::Full
    };

    for (idx, ch) in text.chars().enumerate() {
        while hit_idx < hits.len() && (hits[hit_idx] as usize) < idx {
            hit_idx += 1;
        }
        let hit = hit_idx < hits.len() && (hits[hit_idx] as usize) == idx;

        if hit != current_highlight && !current.is_empty() {
            let (style, policy) = if current_highlight {
                (base_style.patch(selection_style), highlight_policy)
            } else {
                (base_style, RowStylePolicy::Full)
            };
            spans.push(
                Span::new(std::mem::take(&mut current))
                    .style(style)
                    .row_style_policy(policy),
            );
        }
        current_highlight = hit;
        current.push(ch);
    }

    if !current.is_empty() {
        let (style, policy) = if current_highlight {
            (base_style.patch(selection_style), highlight_policy)
        } else {
            (base_style, RowStylePolicy::Full)
        };
        spans.push(Span::new(current).style(style).row_style_policy(policy));
    }

    spans
}

fn disable_row_style_for_spans(spans: Vec<Span>) -> Vec<Span> {
    spans
        .into_iter()
        .map(|span| span.row_style_policy(RowStylePolicy::Disabled))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::matching::all_item_results;
    use super::{
        RenderStyles, ScoreRender, SearchListItemsCtx, build_list_items, default_render,
        highlight_spans,
    };
    use crate::style::{Color, RowStylePolicy, Style};
    use crate::widgets::search_palette::{
        DescriptionOverflow, DescriptionPlacement, ItemDescription, SearchEntry, SearchHighlight,
        SearchItem,
    };

    fn default_styles() -> RenderStyles {
        RenderStyles {
            active_item: None,
            item: Style::default(),
            description: Style::default(),
            active_description: None,
            focused_description: None,
            description_separator: None,
            description_placement: DescriptionPlacement::Inline,
            description_selection: true,
            description_overflow: DescriptionOverflow::Truncate,
            line_width: None,
            highlight: Style::default(),
        }
    }

    #[test]
    fn explicit_match_foreground_is_preserved_from_row_selection() {
        let highlight = Style::new().fg(Color::Yellow).bold();
        let spans = highlight_spans("alpha", &[1, 2], Style::default(), highlight);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].row_style_policy, RowStylePolicy::Full);
        assert_eq!(
            spans[1].row_style_policy,
            RowStylePolicy::PreserveForeground
        );
        assert_eq!(spans[1].style.fg, highlight.fg);
        assert_eq!(spans[2].row_style_policy, RowStylePolicy::Full);
    }

    #[test]
    fn match_without_foreground_keeps_row_foreground_precedence() {
        let spans = highlight_spans("alpha", &[1, 2], Style::default(), Style::new().bold());

        assert!(
            spans
                .iter()
                .all(|span| span.row_style_policy == RowStylePolicy::Full)
        );
    }

    #[test]
    fn grouped_headers_use_header_style() {
        let items = vec![SearchItem::new("Alpha", ()), SearchItem::new("Beta", ())];
        let entries = vec![
            SearchEntry::header("Group"),
            SearchEntry::item("Alpha", ()),
            SearchEntry::item("Beta", ()),
        ];
        let header_style = Style::new().fg(Color::Yellow).bold();
        let styles = default_styles();

        let output = build_list_items(
            &items,
            &entries,
            &all_item_results(items.len()),
            SearchListItemsCtx {
                renderer: None,
                status_renderer: None,
                gutter_renderer: None,
                styles: &styles,
                score: ScoreRender {
                    show: false,
                    gradient: None,
                    range: None,
                },
                selected_result_index: Some(0),
                header_style,
            },
        );

        assert_eq!(output.row_to_result.first(), Some(&None));
        assert_eq!(output.items[0].style, header_style);
    }

    #[test]
    fn right_description_always_highlights_primary_line() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Right,
                description_selection: false,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(rendered.primary_selection_description);
        assert!(rendered.primary_hover_description);
        assert!(rendered.extra_lines.is_empty());
    }

    #[test]
    fn above_description_moves_symbol_to_label_line() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::new().dim(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Above,
                description_selection: false,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert_eq!(rendered.symbol_line, 1);
        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(!rendered.primary_selection_label);
        assert!(!rendered.primary_selection_description);
        assert_eq!(rendered.style, Style::default());
        let primary: String = rendered
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let secondary: String = rendered.extra_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(primary, "Desc");
        assert_eq!(secondary, "Label");
    }

    #[test]
    fn right_description_adds_minimum_one_space_gap() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Right,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(!rendered.description_spans.is_empty());
        assert_eq!(rendered.description_spans[0].content.as_ref(), " ");
    }

    #[test]
    fn inline_truncate_keeps_label_and_truncates_description_first() {
        let item = SearchItem::new("Label", ()).description("VeryLongDescription");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Inline,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(11),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        let primary: String = rendered
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(primary.starts_with("Label"));
        assert!(primary.contains(" - "));
    }

    #[test]
    fn right_metadata_truncates_before_label_in_default_inline_placement() {
        let item = SearchItem::new("Label", ())
            .description(ItemDescription::new().right("VeryLongRightMetadata"));
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Inline,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(10),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(rendered.primary_truncate_description_first);
        assert!(!rendered.description_spans.is_empty());
    }

    #[test]
    fn right_metadata_truncates_before_label_in_above_primary_line() {
        let item = SearchItem::new("Label", ()).description(
            ItemDescription::new()
                .left("Desc")
                .right("VeryLongRightMetadata"),
        );
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Above,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(10),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(rendered.extra_lines[0].truncate_description_first);
        assert!(!rendered.extra_lines[0].description_spans.is_empty());
    }

    #[test]
    fn inline_wrap_falls_back_to_truncate() {
        let item = SearchItem::new("Label", ()).description("VeryLongDescription");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Inline,
                description_selection: true,
                description_overflow: DescriptionOverflow::Wrap,
                line_width: Some(11),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        let primary: String = rendered
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(rendered.extra_lines.is_empty());
        assert!(primary.contains('…'));
    }

    #[test]
    fn right_description_selection_flag_is_ignored() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Right,
                description_selection: false,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(rendered.primary_selection_description);
        assert!(rendered.primary_hover_description);
    }

    #[test]
    fn right_wrap_falls_back_to_truncate() {
        let item = SearchItem::new("Label", ()).description("ABCDEFGHIJK");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Right,
                description_selection: true,
                description_overflow: DescriptionOverflow::Wrap,
                line_width: Some(10),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        let right: String = rendered
            .description_spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(!rendered.description_spans.is_empty());
        assert!(!rendered.primary_wrap_description);
        assert!(rendered.extra_lines.is_empty());
        assert!(right.contains('…'));
    }

    #[test]
    fn right_truncate_limits_right_side_to_preserve_label() {
        let item = SearchItem::new("Label", ()).description("ABCDEFG");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Right,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(5),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(rendered.description_spans.is_empty());
    }

    #[test]
    fn below_description_selection_false_disables_hover_on_description_line() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Below,
                description_selection: false,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(!rendered.extra_lines[0].hover_label);
        assert!(!rendered.extra_lines[0].hover_description);
    }

    #[test]
    fn below_wrap_splits_description_lines() {
        let item = SearchItem::new("Label", ()).description("ABCDEFGHIJK");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Below,
                description_selection: true,
                description_overflow: DescriptionOverflow::Wrap,
                line_width: Some(4),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(rendered.extra_lines[0].wrap_label);
    }

    #[test]
    fn active_description_override_protects_inline_description_spans() {
        let item = SearchItem::new("Label", ())
            .description("Desc")
            .active(true);
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: Some(Style::new().dim()),
                focused_description: None,
                description_separator: None,
                description_placement: DescriptionPlacement::Inline,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            None,
        );

        assert!(
            rendered
                .spans
                .iter()
                .any(|span| span.row_style_policy == RowStylePolicy::Disabled)
        );
    }

    #[test]
    fn focused_description_override_disables_row_highlight_for_description() {
        let item = SearchItem::new("Label", ()).description("Desc");
        let rendered = default_render(
            0,
            &item,
            &SearchHighlight::default(),
            &RenderStyles {
                active_item: None,
                item: Style::default(),
                description: Style::default(),
                active_description: None,
                focused_description: Some(Style::new().dim()),
                description_separator: None,
                description_placement: DescriptionPlacement::Below,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                line_width: Some(20),
                highlight: Style::default(),
            },
            0,
            ScoreRender {
                show: false,
                gradient: None,
                range: None,
            },
            Some(0),
        );

        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(!rendered.extra_lines[0].selection_label);
        assert!(!rendered.extra_lines[0].selection_description);
        assert!(!rendered.extra_lines[0].hover_label);
        assert!(!rendered.extra_lines[0].hover_description);
    }
}
