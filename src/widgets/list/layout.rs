use crate::style::Length;
use crate::widgets::List;
use crate::widgets::list::effective_extra_line_indent;
use crate::widgets::list::effective_prefix;
use crate::widgets::list::leading_metrics;
use crate::widgets::list::reconcile::wrap_right_lines_for_runtime;
use unicode_width::UnicodeWidthStr;

fn spans_width(spans: &[crate::style::Span]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

/// Width-constrained measurement: when a parent knows the available width,
/// wrap items first so the returned height reflects the wrapped line count.
/// Mirrors `measure_text_constrained` for the `Text` widget.
pub fn measure_list_constrained(list: &List, max_w: Option<u16>) -> (u16, u16) {
    let Some(outer_w) = max_w else {
        return measure_list(list);
    };
    if outer_w == 0 {
        return measure_list(list);
    }
    let mut inner_w = outer_w;
    if list.border {
        inner_w = inner_w.saturating_sub(2);
    }
    inner_w = inner_w.saturating_sub(list.padding.horizontal());
    if inner_w == 0 {
        return measure_list(list);
    }
    let wrapped = wrap_right_lines_for_runtime(list, inner_w);
    let mut clone = list.clone();
    clone.items = wrapped;
    measure_list(&clone)
}

pub fn measure_list(list: &List) -> (u16, u16) {
    let mut w = 0usize;
    for (idx, item) in list.items.iter().enumerate() {
        let row_padding_w = if matches!(item.role, crate::widgets::ListItemRole::Header) {
            list.header_horizontal_padding.horizontal() as usize
        } else {
            list.item_horizontal_padding.horizontal() as usize
        };

        let leading = leading_metrics(list, item, idx == list.selected);
        let symbol_w = leading.symbol_width as usize;
        let gutter_w = leading.gutter_width as usize;
        let active_right_w = leading.active_right_symbol_width as usize;
        let selection_right_w = leading.selection_right_symbol_width as usize;
        let trailing_symbol_w = active_right_w.saturating_add(selection_right_w);

        let prefix_w = effective_prefix(list, item)
            .as_ref()
            .map(|prefix| UnicodeWidthStr::width(prefix.as_ref()))
            .unwrap_or(0);
        let extra_line_indent = effective_extra_line_indent(list, item) as usize;

        let primary_w = spans_width(&item.spans)
            .saturating_add(spans_width(&item.description_spans))
            .saturating_add(if item.symbol_line == 0 {
                trailing_symbol_w
            } else {
                0
            })
            .saturating_add(prefix_w);
        let max_item_w = item
            .extra_lines
            .iter()
            .enumerate()
            .map(|(line_idx, line)| {
                spans_width(&line.spans)
                    .saturating_add(spans_width(&line.description_spans))
                    .saturating_add(if item.symbol_line == line_idx + 1 {
                        trailing_symbol_w
                    } else {
                        0
                    })
                    .saturating_add(extra_line_indent)
            })
            .fold(primary_w, usize::max)
            .saturating_add(symbol_w)
            .saturating_add(gutter_w)
            .saturating_add(row_padding_w);
        w = w.max(max_item_w);
    }

    if list.border
        && let Some(title) = &list.title
    {
        w = w.max(UnicodeWidthStr::width(title.as_ref()));
    }

    let mut h = list.items.iter().map(|item| item.line_count()).sum();

    if h == 0
        && let Some(empty) = &list.empty_text
    {
        w = w.max(UnicodeWidthStr::width(empty.as_ref()));
        h = 1;
    }

    w = w.saturating_add(list.padding.horizontal() as usize);
    h = h.saturating_add(list.padding.vertical() as usize);

    if list.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    let mut w = w.min(u16::MAX as usize) as u16;
    let mut h = h.min(u16::MAX as usize) as u16;

    if let Length::Px(px) = list.width {
        w = px;
    }
    if let Length::Px(px) = list.height {
        h = px;
    }

    (w, h)
}

#[cfg(test)]
mod tests {
    use super::measure_list;
    use crate::widgets::{List, ListItem, ListItemGutter, ListItemLine, Spinner};

    #[test]
    fn measure_accounts_for_active_symbol_width() {
        let list = List::new()
            .items([ListItem::new("A").active(true)])
            .active_symbol(Some("@@"));
        let (w, _h) = measure_list(&list);
        assert_eq!(w, 3);
    }

    #[test]
    fn measure_accounts_for_reserved_gutter_width() {
        let list = List::new()
            .items([
                ListItem::new("Alpha").gutter(Spinner::new()),
                ListItem::new("Beta"),
            ])
            .gutter_gap(1);

        let (w, h) = measure_list(&list);
        assert_eq!(w, 7);
        assert_eq!(h, 2);
    }

    #[test]
    fn measure_accounts_for_header_and_item_horizontal_padding() {
        let list = List::new()
            .items([ListItem::header("H"), ListItem::new("X")])
            .header_horizontal_padding(3)
            .item_horizontal_padding(1);
        let (w, _h) = measure_list(&list);
        assert_eq!(w, 7);
    }

    #[test]
    fn measure_multiline_rows_sum_height_and_max_width() {
        let list = List::new().items([
            ListItem::new("A").line(ListItemLine::new("BBBB")),
            ListItem::new("CC"),
        ]);
        let (w, h) = measure_list(&list);
        assert_eq!(w, 4);
        assert_eq!(h, 3);
    }

    #[test]
    fn measure_numbered_item_accounts_for_prefix_and_indent() {
        let list = List::new().items([ListItem::new("Short")
            .numbered(1)
            .line(ListItemLine::new("description"))]);

        let (w, h) = measure_list(&list);
        assert_eq!(w, 14);
        assert_eq!(h, 2);
    }

    #[test]
    fn measure_ignores_active_symbol_width_when_unselected_symbol_is_explicitly_empty() {
        let list = List::new()
            .items([ListItem::new("Short")
                .numbered(1)
                .line(ListItemLine::new("description"))])
            .selection_symbol(Some(""))
            .unselected_symbol(Some(""))
            .active_symbol(Some("✓ "));

        let (w, h) = measure_list(&list);
        assert_eq!(w, 14);
        assert_eq!(h, 2);
    }

    #[test]
    fn measure_accounts_for_active_symbol_width_on_right() {
        let list = List::new()
            .items([ListItem::new("A").active(true)])
            .active_symbol(Some(" @@"))
            .active_symbol_position(crate::widgets::ListSymbolPosition::Right);
        let (w, _h) = measure_list(&list);
        assert_eq!(w, 4);
    }

    #[test]
    fn measure_symbol_column_false_keeps_right_active_symbol() {
        let list = List::new()
            .items([ListItem::new("A").active(true)])
            .active_symbol(Some(" @@"))
            .active_symbol_position(crate::widgets::ListSymbolPosition::Right)
            .symbol_column(false);
        let (w, _h) = measure_list(&list);
        assert_eq!(w, 4);
    }

    #[test]
    fn measure_symbol_column_false_drops_left_symbols_and_status() {
        let list = List::new()
            .items([ListItem::new("A").status_symbol("!!").active(true)])
            .selection_symbol(Some(">>"))
            .unselected_symbol(Some(".."))
            .active_symbol(Some("@@"))
            .symbol_column(false);

        let (w, h) = measure_list(&list);
        assert_eq!(w, 1);
        assert_eq!(h, 1);
    }

    #[test]
    fn measure_gutter_gap_defaults_to_zero() {
        let list = List::new().items([ListItem::new("Alpha").gutter(Spinner::new())]);
        let (w, _h) = measure_list(&list);
        assert_eq!(w, 6);
    }

    #[test]
    fn measure_gutter_gap_two_changes_width_predictably() {
        let list = List::new()
            .items([ListItem::new("Alpha").gutter(Spinner::new())])
            .gutter_gap(2);

        let (w, _h) = measure_list(&list);
        assert_eq!(w, 8);
    }

    #[test]
    fn measure_headers_do_not_reserve_gutter_by_default() {
        let list = List::new().items([
            ListItem::header("Header"),
            ListItem::new("A").gutter(ListItemGutter::text(">>>>")),
        ]);

        let (w, _h) = measure_list(&list);
        assert_eq!(w, 6);
    }

    #[test]
    fn measure_non_selectable_rows_can_reserve_gutter() {
        let list = List::new()
            .items([
                ListItem::header("Header"),
                ListItem::spacer(),
                ListItem::new("A").gutter(ListItemGutter::text(">>>>")),
            ])
            .gutter_for_non_selectable(true);

        let (w, _h) = measure_list(&list);
        assert_eq!(w, 10);
    }

    #[test]
    fn measure_numbered_rows_use_widest_prefix_for_indent() {
        let list = List::new().items([
            ListItem::new("Security")
                .numbered(1)
                .line(ListItemLine::new("desc")),
            ListItem::new("Laziness")
                .numbered(10)
                .line(ListItemLine::new("desc")),
        ]);

        let (w, h) = measure_list(&list);
        assert_eq!(w, 12);
        assert_eq!(h, 4);
    }
}
