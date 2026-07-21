use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
use crate::style::Span;
use crate::widgets::list::layout::measure_list;
use crate::widgets::list::{effective_extra_line_indent, effective_prefix, leading_metrics};
use crate::widgets::{List, ListItem, ListItemLine};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

pub fn reconcile_list(tree: &mut NodeTree, id: NodeId, list: &List, rect: Rect) -> NodeId {
    let auto_width = matches!(list.width, crate::style::Length::Auto);
    let auto_height = matches!(list.height, crate::style::Length::Auto);

    let (w, h) = if auto_width || auto_height {
        if auto_height {
            let mut measured = list.clone();
            let inner_for_wrap = rect.inner(list.border, list.padding);
            measured.items = wrap_right_lines_for_runtime(list, inner_for_wrap.w);
            measure_list(&measured)
        } else {
            measure_list(list)
        }
    } else {
        (0, 0)
    };

    let mut rect = rect;
    if auto_width {
        rect.w = w.min(rect.w);
    }
    if auto_height {
        rect.h = h.min(rect.h);
    }

    {
        let node = tree.node_mut(id);

        // Preserve scroll offset if reusing a list node.
        let (old_offset, scroll_override, old_selected, old_inner_h, old_inner_w) =
            if let NodeKind::List(list_node) = &node.kind {
                let old_inner = node.rect.inner(list_node.border, list_node.padding);
                (
                    list_node.offset,
                    list_node.scroll_override,
                    list_node.selected,
                    old_inner.h as usize,
                    old_inner.w as usize,
                )
            } else {
                (0, None, None, 0, 0)
            };

        let inner = rect.inner(list.border, list.padding);
        let wrapped_items = wrap_right_lines_for_runtime(list, inner.w);
        let len = wrapped_items.len();
        let selected = list
            .selected
            .and_then(|s| crate::widgets::List::nearest_selectable_index(&wrapped_items, s));
        let max_display = inner.h as usize;
        let viewport_changed = old_inner_h != max_display || old_inner_w != inner.w as usize;
        let visible_capacity = crate::widgets::list::utils::visible_items_for_height(
            &wrapped_items,
            old_offset.min(len.saturating_sub(1)),
            inner.h,
        );

        // If the selection changed while a scroll_override is active and the new
        // selection is no longer inside the pinned viewport, release the override
        // immediately so smart-scroll brings the new selection into view this frame.
        let effective_scroll_override = match scroll_override {
            Some(forced) if len > 0 && max_display > 0 => {
                let selection_unchanged = old_selected == selected;
                let (f_start, f_end, _, _) =
                    crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                        forced,
                        &wrapped_items,
                        max_display,
                        list.show_scroll_indicators,
                    );
                let selection_visible =
                    selected.is_some_and(|selected| selected >= f_start && selected < f_end);

                // Keep pinned viewport during explicit browsing (selection unchanged)
                // but release pin if a resize/wrap change made selection disappear.
                if selection_visible || (selection_unchanged && !viewport_changed) {
                    Some(forced)
                } else {
                    None
                }
            }
            _ => scroll_override,
        };

        let (new_offset, top_indicator, bottom_indicator, bottom_count) =
            if max_display == 0 || len == 0 {
                (0, false, false, 0)
            } else if let Some(forced) = effective_scroll_override {
                let forced = crate::widgets::list::utils::clamp_bottom_glued_offset_for_items(
                    forced,
                    &wrapped_items,
                    max_display,
                    list.show_scroll_indicators,
                );
                let (start, end, top, bot) =
                    crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                        forced,
                        &wrapped_items,
                        max_display,
                        list.show_scroll_indicators,
                    );
                (start, top, bot, len.saturating_sub(end))
            } else if let Some(selected) = selected.filter(|_| list.force_scroll_to_selected) {
                // Force scroll to show selected item at the bottom (for auto-follow)
                let visible_for_scroll = visible_capacity.max(1);
                let target_offset = selected.saturating_sub(visible_for_scroll.saturating_sub(1));
                let clamped_offset = target_offset.min(len.saturating_sub(visible_for_scroll));
                if list.show_scroll_indicators {
                    let (start, end, top, bot) =
                        crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                            clamped_offset,
                            &wrapped_items,
                            max_display,
                            true,
                        );
                    (start, top, bot, len.saturating_sub(end))
                } else {
                    (clamped_offset, false, false, 0)
                }
            } else if let Some(selected) = selected {
                if list.show_scroll_indicators {
                    let smart_off = crate::widgets::scroll::smart_list_offset_with_indicators(
                        old_offset,
                        selected,
                        len,
                        max_display,
                    );
                    let (start, end, top, bot) =
                        crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                            smart_off,
                            &wrapped_items,
                            max_display,
                            true,
                        );
                    (start, top, bot, len.saturating_sub(end))
                } else {
                    (
                        crate::widgets::scroll::smart_list_offset(
                            old_offset,
                            selected,
                            len,
                            visible_capacity.min(u16::MAX as usize) as u16,
                        ),
                        false,
                        false,
                        0,
                    )
                }
            } else if list.show_scroll_indicators {
                // No selection: keep the previous offset, but still derive the
                // overflow indicators from it so a read-only list keeps its
                // "N more" affordance.
                let (start, end, top, bot) =
                    crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                        old_offset,
                        &wrapped_items,
                        max_display,
                        true,
                    );
                (start, top, bot, len.saturating_sub(end))
            } else {
                // No selection: keep the previous offset (clamped below).
                (old_offset, false, false, 0)
            };

        let new_offset = new_offset.min(len.saturating_sub(1));

        let visible_items = if list.show_scroll_indicators && max_display > 0 && len > 0 {
            let (start, end, _, _) =
                crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
                    new_offset,
                    &wrapped_items,
                    max_display,
                    true,
                );
            end.saturating_sub(start)
        } else {
            crate::widgets::list::utils::visible_items_for_height(
                &wrapped_items,
                new_offset,
                inner.h,
            )
        };

        let mut next_scroll_override = None;
        if effective_scroll_override.is_some() {
            next_scroll_override = Some(new_offset);
        }

        node.rect = rect;
        node.children.clear();

        let is_standalone = list.scrollbar
            && (!list.border
                || matches!(
                    list.scrollbar_config.variant,
                    crate::style::ScrollbarVariant::Standalone
                ));

        // Determine if we actually need a scrollbar (adaptive).
        let scrollable = len > visible_items;
        let actual_scrollbar = if is_standalone {
            scrollable
        } else {
            list.scrollbar
        };

        // Create the new ListNode
        let mut list_node = crate::widgets::internal::ListNode {
            items: wrapped_items,
            selected,
            scroll_keys: list.scroll_keys,
            scroll_wheel: list.scroll_wheel,
            offset: new_offset,
            scroll_override: None, // Will be set below
            style: list.style,
            hover_style: list.hover_style,
            item_hover_style: list.item_hover_style,
            active_style: list.active_style,
            selection_style: list.selection_style,
            unfocused_selection_style: list.unfocused_selection_style,
            active_symbol: list.active_symbol.clone(),
            active_symbol_position: list.active_symbol_position,
            active_symbol_style: list.active_symbol_style,
            selection_symbol: list.selection_symbol.clone(),
            selection_symbol_right: list.selection_symbol_right.clone(),
            selection_symbol_style: list.selection_symbol_style,
            unfocused_selection_symbol_style: list.unfocused_selection_symbol_style,
            unselected_symbol: list.unselected_symbol.clone(),
            symbol_column: list.symbol_column,
            gutter_gap: list.gutter_gap,
            gutter_for_non_selectable: list.gutter_for_non_selectable,
            selection_full_width: list.selection_full_width,
            item_horizontal_padding: list.item_horizontal_padding,
            header_horizontal_padding: list.header_horizontal_padding,
            border: list.border,
            border_style: list.border_style,
            title: list.title.clone(),
            title_style: list.title_style,
            padding: list.padding,
            scrollbar: actual_scrollbar,
            scrollbar_variant: list.scrollbar_config.variant,
            scrollbar_gap: list.scrollbar_config.gap,
            scrollbar_thumb: list.scrollbar_config.thumb,
            scrollbar_thumb_style: list.scrollbar_config.thumb_style,
            scrollbar_thumb_focus_style: list.scrollbar_config.thumb_focus_style,
            scrollbar_track_style: list.scrollbar_config.track_style,
            show_scroll_indicators: list.show_scroll_indicators,
            scroll_indicator_style: list.scroll_indicator_style,
            top_indicator,
            bottom_indicator,
            bottom_count,
            disabled: list.disabled,
            disabled_style: list.disabled_style,
            empty_text: list.empty_text.clone(),
            empty_text_style: list.empty_text_style,
            on_select: list.on_select.clone(),
            on_item_click: list.on_item_click.clone(),
            on_activate: list.on_activate.clone(),
            on_click: list.on_click.clone(),
            activate_on_click: list.activate_on_click,
            on_scroll_to: list.on_scroll_to.clone(),
            on_key: list.on_key.clone(),
            focusable: list.focusable,
            tab_stop: list.tab_stop,
            on_focus: list.on_focus.clone(),
            on_blur: list.on_blur.clone(),
        };

        if let Some(forced) = next_scroll_override {
            if len > 0 && visible_items > 0 {
                if let Some(selected) = selected {
                    let margin = if visible_items <= 2 {
                        0
                    } else if visible_items <= 6 {
                        1
                    } else {
                        2
                    };
                    let selected = selected.min(len.saturating_sub(1));
                    let min_selected = forced.saturating_add(margin);
                    let max_selected = forced
                        .saturating_add(visible_items.saturating_sub(1).saturating_sub(margin))
                        .min(len.saturating_sub(1));
                    if selected < min_selected || selected > max_selected {
                        // Selection is outside the comfortable margin of the pinned
                        // viewport.  The effective_scroll_override logic above has already
                        // released the override when the selection moved outside the window,
                        // so reaching here means the selection is unchanged (user is
                        // browsing) - keep the viewport pinned.
                        list_node.scroll_override = Some(forced);
                    } else {
                        list_node.scroll_override = None;
                    }
                } else {
                    // No selection: keep the pinned viewport while browsing.
                    list_node.scroll_override = Some(forced);
                }
            } else {
                list_node.scroll_override = None;
            }
        } else {
            list_node.scroll_override = None;
        }

        node.kind = NodeKind::List(list_node);
    }

    tree.register_scrollbar_zone(id);

    id
}

pub(crate) fn wrap_right_lines_for_runtime(list: &List, inner_w: u16) -> Arc<[ListItem]> {
    if inner_w == 0 {
        return list.items.clone();
    }

    if !list_has_wrap_markers(list) {
        return list.items.clone();
    }

    let max_text_w = if list.scrollbar
        && matches!(
            list.scrollbar_config.variant,
            crate::style::ScrollbarVariant::Standalone
        ) {
        inner_w.saturating_sub(1)
    } else {
        inner_w
    };

    let wrapped: Vec<ListItem> = list
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            wrap_item_right_lines(item, list, max_text_w, list.selected == Some(idx))
        })
        .collect();

    wrapped.into()
}

fn list_has_wrap_markers(list: &List) -> bool {
    list.items.iter().any(|item| {
        item.primary_wrap_label
            || item.primary_wrap_description
            || item
                .extra_lines
                .iter()
                .any(|line| line.wrap_label || line.wrap_description)
    })
}

fn wrap_item_right_lines(
    item: &ListItem,
    list: &List,
    max_text_w: u16,
    is_selected: bool,
) -> ListItem {
    let row_padding = if matches!(item.role, crate::widgets::ListItemRole::Header) {
        list.header_horizontal_padding
    } else {
        list.item_horizontal_padding
    };
    let leading = leading_metrics(list, item, is_selected);
    let prefix_width = effective_prefix(list, item)
        .as_ref()
        .map(|p| UnicodeWidthStr::width(p.as_ref()) as u16)
        .unwrap_or(0);
    let extra_line_indent = effective_extra_line_indent(list, item);
    let common_budget = max_text_w
        .saturating_sub(leading.symbol_width)
        .saturating_sub(leading.gutter_width)
        .saturating_sub(row_padding.horizontal());
    let primary_cont_budget = common_budget.saturating_sub(extra_line_indent);

    let original_symbol_line = item.symbol_line;
    let original_gutter_line = item.gutter_line;
    let mut symbol_line = item.symbol_line;
    let mut gutter_line = item.gutter_line;

    let mut wrapped_lines = Vec::new();

    let mut primary_spans = item.spans.clone();
    let mut primary_description_spans = item.description_spans.clone();

    let budget_for_original_line = |line_no: usize, prefix_or_indent_width: u16| {
        let mut budget = common_budget.saturating_sub(prefix_or_indent_width);
        if original_symbol_line == line_no {
            budget = budget
                .saturating_sub(leading.active_right_symbol_width)
                .saturating_sub(leading.selection_right_symbol_width);
        }
        budget
    };

    let primary_first_budget = budget_for_original_line(0, prefix_width);

    if item.primary_wrap_label && primary_description_spans.is_empty() {
        let (first, rest) =
            split_for_wrap(&primary_spans, primary_first_budget, primary_cont_budget);
        let rest_count = rest.len();
        primary_spans = first;
        if original_symbol_line > 0 {
            symbol_line = symbol_line.saturating_add(rest_count);
        }
        if original_gutter_line > 0 {
            gutter_line = gutter_line.saturating_add(rest_count);
        }
        for extra in rest {
            wrapped_lines.push(
                ListItemLine::from_spans(extra)
                    .style(item.style)
                    .selection_label(item.primary_selection_label)
                    .selection_description(false)
                    .hover_label(item.primary_hover_label)
                    .hover_description(false)
                    .truncate_description_first(item.primary_truncate_description_first)
                    .wrap_label(false)
                    .wrap_description(false),
            );
        }
    }

    if item.primary_wrap_description {
        let left_w = spans_width(&primary_spans) as u16;
        let first_budget = primary_first_budget.saturating_sub(left_w);
        let cont_budget = primary_cont_budget;
        let (first, rest) = split_for_wrap(&primary_description_spans, first_budget, cont_budget);
        let rest_count = rest.len();
        primary_description_spans = first;
        if original_symbol_line > 0 {
            symbol_line = symbol_line.saturating_add(rest_count);
        }
        if original_gutter_line > 0 {
            gutter_line = gutter_line.saturating_add(rest_count);
        }
        for extra in rest {
            wrapped_lines.push(
                ListItemLine::from_spans(Vec::<Span>::new())
                    .description_spans(extra)
                    .style(item.style)
                    .selection_label(false)
                    .selection_description(item.primary_selection_description)
                    .hover_label(false)
                    .hover_description(item.primary_hover_description)
                    .truncate_description_first(item.primary_truncate_description_first)
                    .wrap_label(false)
                    .wrap_description(false),
            );
        }
    }

    let mut out = ListItem::from_spans(primary_spans)
        .description_spans(primary_description_spans)
        .style(item.style)
        .role(item.role)
        .active(item.active)
        .primary_selection_label(item.primary_selection_label)
        .primary_selection_description(item.primary_selection_description)
        .primary_hover_label(item.primary_hover_label)
        .primary_hover_description(item.primary_hover_description)
        .primary_truncate_description_first(item.primary_truncate_description_first)
        .primary_wrap_label(false)
        .primary_wrap_description(false)
        .symbol_line(symbol_line);
    out.status = item.status.clone();
    out.prefix = item.prefix.clone();
    out.prefix_kind = item.prefix_kind.clone();
    out.prefix_style = item.prefix_style;
    out.extra_line_indent = extra_line_indent;
    out.gutter = item.gutter.clone();
    out.gutter_line = gutter_line;

    for (extra_idx, line) in item.extra_lines.iter().enumerate() {
        let original_line_no = extra_idx + 1;
        let extra_first_budget = budget_for_original_line(original_line_no, extra_line_indent);
        let extra_cont_budget = common_budget.saturating_sub(extra_line_indent);
        if line.wrap_label && line.description_spans.is_empty() {
            let (first, rest) = split_for_wrap(&line.spans, extra_first_budget, extra_cont_budget);
            let rest_count = rest.len();
            if original_line_no < original_symbol_line {
                symbol_line = symbol_line.saturating_add(rest_count);
            }
            if original_line_no < original_gutter_line {
                gutter_line = gutter_line.saturating_add(rest_count);
            }
            let primary = ListItemLine::from_spans(first)
                .style(line.style)
                .selection_label(line.selection_label)
                .selection_description(false)
                .hover_label(line.hover_label)
                .hover_description(false)
                .truncate_description_first(line.truncate_description_first)
                .wrap_label(false)
                .wrap_description(false);
            wrapped_lines.push(primary);
            for extra in rest {
                wrapped_lines.push(
                    ListItemLine::from_spans(extra)
                        .style(line.style)
                        .selection_label(line.selection_label)
                        .selection_description(false)
                        .hover_label(line.hover_label)
                        .hover_description(false)
                        .truncate_description_first(line.truncate_description_first)
                        .wrap_label(false)
                        .wrap_description(false),
                );
            }
        } else if line.wrap_description {
            let left_w = spans_width(&line.spans) as u16;
            let first_budget = extra_first_budget.saturating_sub(left_w);
            let cont_budget = extra_cont_budget;
            let (first, rest) = split_for_wrap(&line.description_spans, first_budget, cont_budget);
            let rest_count = rest.len();
            if original_line_no < original_symbol_line {
                symbol_line = symbol_line.saturating_add(rest_count);
            }
            if original_line_no < original_gutter_line {
                gutter_line = gutter_line.saturating_add(rest_count);
            }
            let primary = ListItemLine::from_spans(line.spans.clone())
                .description_spans(first)
                .style(line.style)
                .selection_label(line.selection_label)
                .selection_description(line.selection_description)
                .hover_label(line.hover_label)
                .hover_description(line.hover_description)
                .truncate_description_first(line.truncate_description_first)
                .wrap_label(false)
                .wrap_description(false);
            wrapped_lines.push(primary);
            for extra in rest {
                wrapped_lines.push(
                    ListItemLine::from_spans(Vec::<Span>::new())
                        .description_spans(extra)
                        .style(line.style)
                        .selection_label(false)
                        .selection_description(line.selection_description)
                        .hover_label(false)
                        .hover_description(line.hover_description)
                        .truncate_description_first(line.truncate_description_first)
                        .wrap_label(false)
                        .wrap_description(false),
                );
            }
        } else {
            wrapped_lines.push(line.clone());
        }
    }

    out.symbol_line(symbol_line)
        .gutter_line(gutter_line)
        .lines(wrapped_lines)
}

fn split_for_wrap(
    spans: &[Span],
    first_budget: u16,
    cont_budget: u16,
) -> (Vec<Span>, Vec<Vec<Span>>) {
    let mut lines = crate::utils::text::wrap_spans_for_budgets(spans, first_budget, cont_budget);
    if lines.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let first = lines.remove(0);
    (first, lines)
}

fn spans_width(spans: &[Span]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{reconcile_list, wrap_right_lines_for_runtime};
    use crate::core::node::{NodeKind, NodeTree};
    use crate::style::{Rect, Span};
    use crate::widgets::{List, ListItem, ListItemLine, ListSymbolPosition, Spinner};

    #[test]
    fn wrap_right_lines_uses_runtime_inner_width() {
        let list = List::new().items([ListItem::new("Label")
            .description_spans([Span::new(" DescLong")])
            .primary_wrap_description(true)]);

        let wrapped = wrap_right_lines_for_runtime(&list, 8);
        assert_eq!(wrapped.len(), 1);
        assert!(!wrapped[0].description_spans.is_empty());
        assert!(!wrapped[0].extra_lines.is_empty());
    }

    #[test]
    fn wrap_right_lines_keeps_wide_chars_when_budget_is_tight() {
        let list = List::new()
            .selection_symbol(Some("> "))
            .items([ListItem::new("L")
                .description_spans([Span::new("你")])
                .primary_wrap_description(true)]);

        let wrapped = wrap_right_lines_for_runtime(&list, 3);
        assert_eq!(wrapped.len(), 1);

        let mut combined = String::new();
        for span in &wrapped[0].description_spans {
            combined.push_str(span.content.as_ref());
        }
        for line in &wrapped[0].extra_lines {
            for span in &line.description_spans {
                combined.push_str(span.content.as_ref());
            }
        }

        assert!(combined.contains('你'));
    }

    #[test]
    fn wrap_left_prefers_word_boundaries() {
        let list = List::new().items([ListItem::new("alpha beta gamma").primary_wrap_label(true)]);

        let wrapped = wrap_right_lines_for_runtime(&list, 6);
        assert_eq!(wrapped.len(), 1);
        assert!(!wrapped[0].extra_lines.is_empty());

        let first: String = wrapped[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(first, "alpha ");
    }

    #[test]
    fn symbol_line_shifts_when_primary_wrap_left_inserts_lines() {
        let item = ListItem::new("alpha beta gamma")
            .primary_wrap_label(true)
            .symbol_line(1)
            .line(ListItemLine::new("Label"));
        let list = List::new().items([item]);

        let wrapped = wrap_right_lines_for_runtime(&list, 6);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].symbol_line, 3);
    }

    #[test]
    fn wrap_budget_subtracts_right_active_symbol_only_on_symbol_line() {
        let item = ListItem::new("abcd")
            .primary_wrap_label(true)
            .line(ListItemLine::new("marker"))
            .symbol_line(1)
            .active(true);
        let list = List::new()
            .items([item])
            .active_symbol(Some(" >>"))
            .active_symbol_position(ListSymbolPosition::Right);

        let wrapped = wrap_right_lines_for_runtime(&list, 4);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0].extra_lines.len(), 1);
        assert_eq!(wrapped[0].symbol_line, 1);
    }

    #[test]
    fn reconcile_copies_leading_column_config_to_node() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let list = List::new()
            .items([ListItem::new("Alpha").gutter(Spinner::new())])
            .symbol_column(false)
            .gutter_gap(2)
            .gutter_for_non_selectable(true);

        reconcile_list(
            &mut tree,
            id,
            &list,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
        );

        let NodeKind::List(node) = &tree.node(id).kind else {
            panic!("expected list node");
        };
        assert!(!node.symbol_column);
        assert_eq!(node.gutter_gap, 2);
        assert!(node.gutter_for_non_selectable);
    }

    #[test]
    fn selected_none_keeps_empty_selection_on_node() {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let list = List::new()
            .items([ListItem::new("Alpha"), ListItem::new("Beta")])
            .selected(None)
            .selection_symbol(Some("> "))
            .selection_style(crate::style::Style::new().bold());

        reconcile_list(
            &mut tree,
            id,
            &list,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 3,
            },
        );

        let NodeKind::List(node) = &tree.node(id).kind else {
            panic!("expected list node");
        };
        assert_eq!(node.selected, None);
    }

    fn overflow_indicators(selected: Option<usize>) -> (bool, bool, usize) {
        let mut tree = NodeTree::new();
        let id = tree.alloc();
        let list = List::new()
            .items((0..30).map(|i| ListItem::new(format!("row {i}"))))
            .selected(selected)
            .show_scroll_indicators(true);

        reconcile_list(
            &mut tree,
            id,
            &list,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 10,
            },
        );

        let NodeKind::List(node) = &tree.node(id).kind else {
            panic!("expected list node");
        };
        (node.top_indicator, node.bottom_indicator, node.bottom_count)
    }

    #[test]
    fn selected_none_still_reports_overflow_indicators() {
        // A read-only list must keep its "N more below" affordance; the
        // no-selection scroll branch used to hardcode the indicators off.
        let (top, bottom, bottom_count) = overflow_indicators(None);
        assert!(!top);
        assert!(bottom, "overflowing list should report a bottom indicator");
        assert!(bottom_count > 0, "bottom_count = {bottom_count}");

        // Matches what an equivalent selected list reports from the same offset.
        assert_eq!(overflow_indicators(Some(0)), (top, bottom, bottom_count));
    }
}
