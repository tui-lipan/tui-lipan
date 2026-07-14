use ratatui::text::{Line, Span, Text as RText};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IndicatorDirection, IntegratedScrollbarAppearance,
    calculate_visible_borders, clear_fg_preserve_bg_clipped, finalize_style,
    integrated_vscrollbar_track_char, render_integrated_scrollbar_with_metrics,
    render_integrated_vscrollbar_half_block, render_vscrollbar_half_block,
    render_vscrollbar_with_metrics, resolve_interactive_style_raw, resolve_scrollbar_thumb_style,
    scroll_indicator_line, single_line_scroll_indicator, spaces, style_backdrop, style_paints_bg,
    style_uses_backdrop_bg, to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect,
    to_ratatui_style, truncate_end_with_ellipsis, truncate_spans,
};
use crate::backend::ratatui_backend::render::{
    FrameIntegratedVTrack, RenderState, ancestor_frame_integrated_vtrack,
};
use crate::backend::ratatui_backend::renderers::spinner::{SpinnerRenderCtx, render_spinner};
use crate::backend::ratatui_backend::renderers::theme::{
    scrollbar_styles, with_theme_muted, with_theme_optional_primary, with_theme_primary,
};
use crate::core::node::NodeId;
use crate::style::resolve::{Durability, StateLayer, resolve_state_cascade};
use crate::style::theme::merge_channel;
use crate::style::{
    BorderStyle, Padding, Rect, RowStylePolicy, ScrollbarVariant, Style, ThemeRole,
    resolve_selection_slot, resolve_slot,
};
use crate::widgets::list::{
    ListItemGutterKind, ListItemStatusKind, ListSymbolWidthCtx,
    effective_extra_line_indent_for_width, effective_prefix_for_width,
    item_symbol_width_for_reserved, item_uses_gutter, max_numbered_prefix_width_for_items,
    reserved_gutter_width_for_items, reserved_symbol_width_for_items,
};
use crate::widgets::{ListItem, ListSymbolPosition, SpinnerStyle};

/// Merge a list item rich-text span over its row/content base style.
///
/// This is intentionally list-specific: non-empty spans do not inherit row text
/// modifiers such as bold/dim/italic so explicit span styling remains protected
/// from row selection/hover modifiers. Empty spans keep inheriting the base.
fn list_rich_text_span_style(base: Style, span: Style) -> Style {
    if span.is_empty() {
        return base;
    }

    let (bg, bg_transform) =
        merge_channel(base.bg, base.bg_transform, span.bg, span.bg_transform, None);
    let (fg, fg_transform) = merge_channel(
        base.fg,
        base.fg_transform,
        span.fg,
        span.fg_transform,
        bg.or(span.bg).or(base.bg),
    );

    Style {
        fg,
        bg,
        fg_transform,
        bg_transform,
        contrast_policy: span.contrast_policy.or(base.contrast_policy),
        bold: span.bold,
        dim: span.dim,
        italic: span.italic,
        underline: span.underline,
        reverse: span.reverse,
        strikethrough: span.strikethrough,
        underline_color: span.underline_color.or(base.underline_color),
        dim_amount: span.dim_amount.or(base.dim_amount),
        tint: span.tint.or(base.tint),
    }
}

fn resolve_symbol_style(
    row_style: Style,
    symbol_style: Option<Style>,
    contrast_policy: ContrastPolicy,
) -> ratatui::style::Style {
    let resolved = symbol_style.map_or(row_style, |style| row_style.patch(style));
    to_ratatui_style(finalize_style(
        resolved,
        style_backdrop(row_style),
        contrast_policy,
    ))
}

fn effective_selection_symbol_style(
    selection_symbol_style: Option<Style>,
    unfocused_selection_symbol_style: Option<Style>,
    is_focused: bool,
) -> Option<Style> {
    if is_focused {
        selection_symbol_style
    } else {
        unfocused_selection_symbol_style.or(selection_symbol_style)
    }
}

fn concrete_list_state_style(style: Style) -> Style {
    let style = style.resolve_color_transforms();

    Style {
        fg: style.fg,
        bg: style.bg,
        fg_transform: None,
        bg_transform: None,
        contrast_policy: style.contrast_policy,
        bold: style.bold,
        dim: style.dim,
        italic: style.italic,
        underline: style.underline,
        reverse: style.reverse,
        strikethrough: style.strikethrough,
        underline_color: style.underline_color,
        dim_amount: None,
        tint: None,
    }
}

struct SpinnerGutterDraw {
    spinner_style: SpinnerStyle,
    frame: usize,
    label: Option<std::sync::Arc<str>>,
    gap: u16,
    style: Style,
    label_style: Style,
    rect: Rect,
}

struct GutterSpanPushCtx<'v, 's> {
    left_base_style: Style,
    left_style_override: Style,
    rs_base: ratatui::style::Style,
    item_spans: &'v mut Vec<Span<'s>>,
    item_spans_width: &'v mut usize,
    spinner_draws: &'v mut Vec<SpinnerGutterDraw>,
    content_inner: Rect,
    dx: u16,
    dy: u16,
    virtual_line_idx: usize,
    contrast_policy: ContrastPolicy,
}

struct ListRowStateStyleInput<'a> {
    base: Style,
    hovered: bool,
    hover_style: &'a Style,
    selected: bool,
    selection_style: &'a Style,
    active: bool,
    active_style: &'a Style,
}

pub(crate) struct ListRenderParams<'f, 'b, 'a> {
    pub f: &'f mut ratatui::Frame<'b>,
    pub items: &'a [ListItem],
    pub selected: usize,
    pub offset: usize,
    pub style: Style,
    pub hover_style: Style,
    pub item_hover_style: Style,
    pub active_style: Style,
    pub selection_style: Style,
    pub active_symbol: Option<&'a str>,
    pub active_symbol_position: ListSymbolPosition,
    pub active_symbol_style: Option<Style>,
    pub selection_symbol: Option<&'a str>,
    pub selection_symbol_right: Option<&'a str>,
    pub selection_symbol_style: Option<Style>,
    pub unselected_symbol: Option<&'a str>,
    pub symbol_column: bool,
    pub gutter_gap: u16,
    pub gutter_for_non_selectable: bool,
    pub selection_full_width: bool,
    pub item_horizontal_padding: Padding,
    pub header_horizontal_padding: Padding,
    pub border: bool,
    pub border_style: BorderStyle,
    pub title: Option<&'a str>,
    pub title_style: Style,
    pub padding: Padding,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
    pub empty_text: Option<&'a str>,
    pub empty_text_style: Style,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub disabled: bool,
    pub disabled_style: Style,
    pub rect: Rect,
    pub rrect: ratatui::layout::Rect,
    pub parent_integrated_v: Option<FrameIntegratedVTrack>,
    pub clip_rect: Option<Rect>,
    pub contrast_policy: ContrastPolicy,
}

fn push_gutter_spans<'v, 's>(
    item: &ListItem,
    sub_line: usize,
    reserved_gutter_width: usize,
    ctx: GutterSpanPushCtx<'v, 's>,
) {
    let GutterSpanPushCtx {
        left_base_style,
        left_style_override,
        rs_base,
        item_spans,
        item_spans_width,
        spinner_draws,
        content_inner,
        dx,
        dy,
        virtual_line_idx,
        contrast_policy,
    } = ctx;
    if reserved_gutter_width == 0 {
        return;
    }

    let Some(gutter) = item
        .gutter
        .as_ref()
        .filter(|_| sub_line == item.gutter_line)
    else {
        item_spans.push(Span::styled(spaces(reserved_gutter_width), rs_base));
        *item_spans_width += reserved_gutter_width;
        return;
    };

    let content_width = gutter.width() as usize;
    match &gutter.kind {
        ListItemGutterKind::Text(spans) => {
            for span in spans {
                let span_style = list_rich_text_span_style(left_base_style, span.style)
                    .patch(left_style_override);
                let span_style =
                    finalize_style(span_style, style_backdrop(left_base_style), contrast_policy);
                item_spans.push(Span::styled(
                    span.content.to_string(),
                    to_ratatui_style(span_style),
                ));
            }
        }
        ListItemGutterKind::Spinner(spinner) => {
            let virtual_x = content_inner.x.saturating_add(*item_spans_width as i16);
            let virtual_y = content_inner.y.saturating_add(virtual_line_idx as i16);
            spinner_draws.push(SpinnerGutterDraw {
                spinner_style: spinner.spinner_style,
                frame: spinner.frame,
                label: spinner.label.clone(),
                gap: spinner.gap,
                style: left_base_style.patch(spinner.style),
                label_style: left_base_style.patch(spinner.label_style),
                rect: Rect {
                    x: virtual_x.saturating_sub(dx as i16),
                    y: virtual_y.saturating_sub(dy as i16),
                    w: content_width.min(u16::MAX as usize) as u16,
                    h: 1,
                },
            });
            item_spans.push(Span::styled(spaces(content_width), rs_base));
        }
    }

    let trailing = reserved_gutter_width.saturating_sub(content_width);
    if trailing > 0 {
        item_spans.push(Span::styled(spaces(trailing), rs_base));
    }
    *item_spans_width += reserved_gutter_width;
}

fn push_status_symbol_spans<'v, 's>(
    status: &crate::widgets::ListItemStatus,
    symbol_width: usize,
    ctx: GutterSpanPushCtx<'v, 's>,
) {
    let GutterSpanPushCtx {
        left_base_style,
        left_style_override,
        rs_base,
        item_spans,
        item_spans_width,
        spinner_draws,
        content_inner,
        dx,
        dy,
        virtual_line_idx,
        contrast_policy,
    } = ctx;
    let content_width = status.width() as usize;
    let left_pad = symbol_width.saturating_sub(content_width) / 2;
    let right_pad = symbol_width
        .saturating_sub(content_width)
        .saturating_sub(left_pad);

    if left_pad > 0 {
        item_spans.push(Span::styled(spaces(left_pad), rs_base));
    }

    match &status.kind {
        ListItemStatusKind::Text(spans) => {
            for span in spans {
                let span_style = list_rich_text_span_style(left_base_style, span.style)
                    .patch(left_style_override);
                let span_style =
                    finalize_style(span_style, style_backdrop(left_base_style), contrast_policy);
                item_spans.push(Span::styled(
                    span.content.to_string(),
                    to_ratatui_style(span_style),
                ));
            }
        }
        ListItemStatusKind::Spinner(spinner) => {
            let virtual_x = content_inner
                .x
                .saturating_add(item_spans_width.saturating_add(left_pad) as i16);
            let virtual_y = content_inner.y.saturating_add(virtual_line_idx as i16);
            spinner_draws.push(SpinnerGutterDraw {
                spinner_style: spinner.spinner_style,
                frame: spinner.frame,
                label: None,
                gap: 0,
                style: left_base_style.patch(spinner.style),
                label_style: left_base_style.patch(spinner.label_style),
                rect: Rect {
                    x: virtual_x.saturating_sub(dx as i16),
                    y: virtual_y.saturating_sub(dy as i16),
                    w: content_width.min(u16::MAX as usize) as u16,
                    h: 1,
                },
            });
            item_spans.push(Span::styled(spaces(content_width), rs_base));
        }
    }

    if right_pad > 0 {
        item_spans.push(Span::styled(spaces(right_pad), rs_base));
    }
    *item_spans_width += symbol_width;
}

fn resolve_list_row_state_style(input: ListRowStateStyleInput<'_>) -> Style {
    let ListRowStateStyleInput {
        base,
        hovered,
        hover_style,
        selected,
        selection_style,
        active,
        active_style,
    } = input;
    let empty = Style::default();
    let hover_style = if hovered { hover_style } else { &empty };
    let selection_style = if selected { selection_style } else { &empty };
    let active_style = if active { active_style } else { &empty };

    resolve_state_cascade(
        base,
        &[
            StateLayer {
                style: hover_style,
                durability: Durability::Transient,
            },
            StateLayer {
                style: selection_style,
                durability: Durability::Durable,
            },
            StateLayer {
                style: active_style,
                durability: Durability::Durable,
            },
        ],
    )
}

pub(crate) fn render_list(params: ListRenderParams<'_, '_, '_>) {
    let ListRenderParams {
        f,
        items,
        selected,
        offset,
        style,
        hover_style,
        item_hover_style,
        active_style,
        selection_style,
        active_symbol,
        active_symbol_position,
        active_symbol_style,
        selection_symbol,
        selection_symbol_right,
        selection_symbol_style,
        unselected_symbol,
        symbol_column,
        gutter_gap,
        gutter_for_non_selectable,
        selection_full_width,
        item_horizontal_padding,
        header_horizontal_padding,
        border,
        border_style,
        title,
        title_style,
        padding,
        scrollbar,
        scrollbar_variant,
        scrollbar_gap,
        scrollbar_thumb,
        scrollbar_thumb_style,
        scrollbar_thumb_focus_style,
        scrollbar_track_style,
        show_scroll_indicators,
        scroll_indicator_style,
        top_indicator,
        bottom_indicator,
        bottom_count: _bottom_count,
        empty_text,
        empty_text_style,
        is_focused,
        is_hovered,
        mouse_pos,
        disabled,
        disabled_style,
        rect,
        rrect,
        parent_integrated_v,
        clip_rect,
        contrast_policy,
    } = params;
    let base_style = finalize_style(
        resolve_interactive_style_raw(
            style,
            Style::default(),
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            disabled,
        ),
        None,
        contrast_policy,
    );
    let mut highlight = selection_style;
    let mut active = active_style;
    let _clip_rrect = clip_rect.map(to_ratatui_rect);

    if disabled {
        highlight = highlight.patch(disabled_style);
        active = active.patch(disabled_style);
    }

    let mut inner = rect;

    if style_uses_backdrop_bg(base_style) {
        clear_fg_preserve_bg_clipped(f, rect, clip_rect);
    } else if style_paints_bg(base_style) {
        f.render_widget(Clear, rrect);
    }

    if border {
        let borders = calculate_visible_borders(rect, clip_rect);
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(border_style))
            .style(to_ratatui_style(base_style));

        if let Some(set) = to_ratatui_border_set(border_style) {
            block = block.border_set(set);
        }

        if let Some(t) = title {
            let max_title_w = rect.w.saturating_sub(2);
            let title = truncate_end_with_ellipsis(t, max_title_w).into_owned();
            let title_style = finalize_style(
                base_style.patch(title_style),
                style_backdrop(base_style),
                contrast_policy,
            );
            block = block.title(Span::styled(title, to_ratatui_style(title_style)));
        }

        f.render_widget(block, rrect);

        // Always reserve space for borders if enabled, even if clipped
        inner.x = inner.x.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.y = inner.y.saturating_add(1);
        inner.h = inner.h.saturating_sub(2);
    } else if style_uses_backdrop_bg(base_style) {
        clear_fg_preserve_bg_clipped(f, rect, clip_rect);
    } else if style_paints_bg(base_style) {
        let bg = Block::default().style(to_ratatui_style(base_style));
        f.render_widget(bg, rrect);
    }

    inner = inner.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let len = items.len();
    if len == 0 {
        if let Some(text) = empty_text {
            let style = finalize_style(
                base_style.patch(empty_text_style),
                style_backdrop(base_style),
                contrast_policy,
            );
            let empty_inner = Rect {
                x: inner.x.saturating_add(item_horizontal_padding.left as i16),
                w: inner.w.saturating_sub(item_horizontal_padding.horizontal()),
                ..inner
            };
            let content = truncate_end_with_ellipsis(text, empty_inner.w);
            let p = Paragraph::new(Span::styled(content.to_string(), to_ratatui_style(style)));
            let r = to_ratatui_rect(empty_inner);
            f.render_widget(p, r);
        }
        return;
    }

    let selected = selected.min(len.saturating_sub(1));
    let visible_height = inner.h as usize;
    let top_indicator = show_scroll_indicators && top_indicator;
    let bottom_indicator = show_scroll_indicators && bottom_indicator;

    // Determine scrollbar mode.
    let use_integrated = scrollbar
        && (border || parent_integrated_v.is_some())
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated);
    let use_standalone = scrollbar && !use_integrated;

    // For standalone scrollbar, reserve space from content.
    let content_inner = if use_standalone && inner.w > 0 {
        Rect {
            w: inner.w.saturating_sub(1u16.saturating_add(scrollbar_gap)),
            ..inner
        }
    } else {
        inner
    };

    let inner_rrect = to_ratatui_rect(content_inner);
    let effective_rrect = inner_rrect.intersection(rrect);
    let dx = (effective_rrect.x as i32)
        .saturating_sub(content_inner.x as i32)
        .max(0) as u16;
    let dy = (effective_rrect.y as i32)
        .saturating_sub(content_inner.y as i32)
        .max(0) as u16;

    let max_text_w = content_inner.w;
    let numbered_prefix_width = max_numbered_prefix_width_for_items(items);
    let reserved_gutter_width =
        reserved_gutter_width_for_items(items, gutter_gap, gutter_for_non_selectable) as usize;

    let mut lines = Vec::new();
    let mut spinner_draws = Vec::new();

    if show_scroll_indicators && visible_height == 1 && len > 1 {
        lines.push(single_line_scroll_indicator(
            len,
            Style::default(),
            finalize_style(
                base_style.patch(scroll_indicator_style),
                style_backdrop(base_style),
                contrast_policy,
            ),
        ));
        let text = RText::from(lines);
        let p = Paragraph::new(text).scroll((dy, dx));
        f.render_widget(p, effective_rrect);
        return;
    }

    let (start_index, end_index, _has_top, _has_bottom) =
        crate::widgets::list::utils::calc_list_window_for_items_with_indicators(
            offset,
            items,
            visible_height,
            show_scroll_indicators,
        );
    let item_area_lines = visible_height
        .saturating_sub(top_indicator as usize)
        .saturating_sub(bottom_indicator as usize);

    if top_indicator {
        lines.push(scroll_indicator_line(
            offset,
            IndicatorDirection::Top,
            Style::default(),
            finalize_style(
                base_style.patch(scroll_indicator_style),
                style_backdrop(base_style),
                contrast_policy,
            ),
        ));
    }

    let mut line_idx = if top_indicator { 1usize } else { 0usize };
    let mut used_item_lines = 0usize;

    // Pre-compute symbol widths and padding strings (constant across all items).
    let hl_symbol_width = if symbol_column {
        selection_symbol.map(UnicodeWidthStr::width).unwrap_or(0)
    } else {
        0
    };
    let active_symbol_width = active_symbol.map(UnicodeWidthStr::width).unwrap_or(0);
    let selection_symbol_right_width = selection_symbol_right
        .map(UnicodeWidthStr::width)
        .unwrap_or(0);
    let uhl_symbol_width = if symbol_column {
        unselected_symbol.map(UnicodeWidthStr::width).unwrap_or(0)
    } else {
        0
    };
    let reserved_symbol_width = reserved_symbol_width_for_items(
        items,
        symbol_column,
        active_symbol_position,
        active_symbol,
        selection_symbol,
        unselected_symbol,
    ) as usize;

    // Render items
    for (idx, item) in items.iter().enumerate().take(end_index).skip(start_index) {
        let item_height = crate::widgets::list::utils::list_item_height(item);
        let remaining_item_lines = item_area_lines.saturating_sub(used_item_lines);
        let render_lines = item_height.min(remaining_item_lines);
        if render_lines == 0 {
            break;
        }

        let item_selectable = item.is_selectable();
        let is_selected = item_selectable && idx == selected;
        let is_active = item.is_active();
        let item_symbol_width = item_symbol_width_for_reserved(
            reserved_symbol_width.min(u16::MAX as usize) as u16,
            item,
            is_selected,
            ListSymbolWidthCtx {
                active_symbol_position,
                active_symbol,
                selection_symbol,
                unselected_symbol,
            },
        ) as usize;
        let item_gutter_width = if item_uses_gutter(item, gutter_for_non_selectable) {
            reserved_gutter_width
        } else {
            0
        };
        let active_right_symbol_width =
            if matches!(active_symbol_position, ListSymbolPosition::Right)
                && is_active
                && active_symbol.is_some()
            {
                active_symbol_width
            } else {
                0usize
            };
        // Trailing selection cap: rendered after the label like a right-positioned
        // active symbol, but tracking the selected row. The active right symbol
        // shares this slot and wins when both apply.
        let selection_right_symbol_width =
            if is_selected && active_right_symbol_width == 0 && selection_symbol_right.is_some() {
                selection_symbol_right_width
            } else {
                0usize
            };
        let trailing_symbol_width =
            active_right_symbol_width.saturating_add(selection_right_symbol_width);
        let item_row_style = finalize_style(
            base_style.patch(item.style),
            style_backdrop(base_style),
            contrast_policy,
        );
        let active_item_row_style = if is_active {
            finalize_style(
                item_row_style.patch(active),
                style_backdrop(item_row_style),
                contrast_policy,
            )
        } else {
            item_row_style
        };
        let mut is_item_hovered = false;
        if !disabled && item_selectable && is_hovered {
            let row_start = line_idx;
            let row_end = line_idx.saturating_add(render_lines);
            is_item_hovered = mouse_pos.is_some_and(|(mx, my)| {
                let rel_y = (my as i16).saturating_sub(content_inner.y) as usize;
                (mx as i16) >= content_inner.x
                    && (mx as i16) < content_inner.x.saturating_add(content_inner.w as i16)
                    && rel_y >= row_start
                    && rel_y < row_end
            });
        }

        let row_padding = if matches!(item.role, crate::widgets::list::ListItemRole::Header) {
            header_horizontal_padding
        } else {
            item_horizontal_padding
        };

        for sub_line in 0..render_lines {
            let (
                line_spans_src,
                line_right_spans_src,
                line_style,
                selection_left,
                selection_right,
                hover_left,
                hover_right,
                truncate_description_first,
                max_label_width,
                max_description_width,
            ) = if sub_line == 0 {
                (
                    &item.spans,
                    &item.description_spans,
                    item.style,
                    item.primary_selection_label,
                    item.primary_selection_description,
                    item.primary_hover_label,
                    item.primary_hover_description,
                    item.primary_truncate_description_first,
                    item.primary_max_label_width,
                    item.primary_max_description_width,
                )
            } else {
                let line = &item.extra_lines[sub_line - 1];
                (
                    &line.spans,
                    &line.description_spans,
                    line.style,
                    line.selection_label,
                    line.selection_description,
                    line.hover_label,
                    line.hover_description,
                    line.truncate_description_first,
                    line.max_label_width,
                    line.max_description_width,
                )
            };

            let left_content_base_style = finalize_style(
                item_row_style.patch(line_style),
                style_backdrop(item_row_style),
                contrast_policy,
            );
            let right_content_base_style = finalize_style(
                item_row_style.patch(line_style),
                style_backdrop(item_row_style),
                contrast_policy,
            );
            let left_protected_content_base_style = finalize_style(
                active_item_row_style.patch(line_style),
                style_backdrop(active_item_row_style),
                contrast_policy,
            );
            let right_protected_content_base_style = finalize_style(
                active_item_row_style.patch(line_style),
                style_backdrop(active_item_row_style),
                contrast_policy,
            );
            // When hover/selection/active styles set explicit fields (fg, bold, etc.),
            // they must override span-level values (which come from item_style).
            // We capture the override style here and patch it onto each span
            // after list_rich_text_span_style, which otherwise lets span fields
            // take precedence. Hover < selection < active.
            let left_row_state_style = resolve_list_row_state_style(ListRowStateStyleInput {
                base: Style::default(),
                hovered: is_item_hovered && hover_left,
                hover_style: &item_hover_style,
                selected: is_selected && selection_left,
                selection_style: &highlight,
                active: is_active && selection_left,
                active_style: &active,
            });
            let right_row_state_style = resolve_list_row_state_style(ListRowStateStyleInput {
                base: Style::default(),
                hovered: is_item_hovered && hover_right,
                hover_style: &item_hover_style,
                selected: is_selected && selection_right,
                selection_style: &highlight,
                active: is_active && selection_right,
                active_style: &active,
            });
            let left_style_override = concrete_list_state_style(left_row_state_style);
            let right_style_override = concrete_list_state_style(right_row_state_style);

            let left_base_style = finalize_style(
                left_content_base_style.patch(left_row_state_style),
                style_backdrop(left_content_base_style),
                contrast_policy,
            );
            let right_base_style = finalize_style(
                right_content_base_style.patch(right_row_state_style),
                style_backdrop(right_content_base_style),
                contrast_policy,
            );

            let rs_base = to_ratatui_style(left_base_style);
            let rs_right_base = to_ratatui_style(right_base_style);
            let mut item_spans = Vec::with_capacity(
                4usize
                    .saturating_add(line_spans_src.len())
                    .saturating_add(line_right_spans_src.len()),
            );
            let mut item_spans_width = 0usize;
            let effective_prefix = effective_prefix_for_width(item, numbered_prefix_width);
            let prefix_or_indent_w: u16 = if sub_line == 0 {
                effective_prefix
                    .as_ref()
                    .map(|prefix| {
                        UnicodeWidthStr::width(prefix.as_ref()).min(u16::MAX as usize) as u16
                    })
                    .unwrap_or(0)
            } else {
                effective_extra_line_indent_for_width(item, numbered_prefix_width)
            };

            if symbol_column && item_selectable {
                let show_symbol_on_line = sub_line == item.symbol_line;
                if show_symbol_on_line {
                    // Symbol priority: active_symbol > selection_symbol > unselected_symbol > spaces.
                    if matches!(active_symbol_position, ListSymbolPosition::Left)
                        && is_active
                        && let Some(symbol) = active_symbol
                    {
                        let sym_rs = resolve_symbol_style(
                            left_base_style,
                            active_symbol_style,
                            contrast_policy,
                        );
                        item_spans.push(Span::styled(symbol, sym_rs));
                        item_spans_width += active_symbol_width;
                    } else if let Some(status) = item.status.as_ref() {
                        push_status_symbol_spans(
                            status,
                            item_symbol_width,
                            GutterSpanPushCtx {
                                left_base_style,
                                left_style_override,
                                rs_base,
                                item_spans: &mut item_spans,
                                item_spans_width: &mut item_spans_width,
                                spinner_draws: &mut spinner_draws,
                                content_inner,
                                dx,
                                dy,
                                virtual_line_idx: line_idx,
                                contrast_policy,
                            },
                        );
                    } else if is_selected {
                        if let Some(symbol) = selection_symbol {
                            let sym_rs = resolve_symbol_style(
                                left_base_style,
                                selection_symbol_style,
                                contrast_policy,
                            );
                            item_spans.push(Span::styled(symbol, sym_rs));
                            item_spans_width += hl_symbol_width;
                        } else if item_symbol_width > 0 {
                            item_spans.push(Span::styled(spaces(item_symbol_width), rs_base));
                            item_spans_width += item_symbol_width;
                        }
                    } else if let Some(symbol) = unselected_symbol {
                        item_spans.push(Span::styled(symbol, rs_base));
                        item_spans_width += uhl_symbol_width;
                    } else if item_symbol_width > 0 {
                        item_spans.push(Span::styled(spaces(item_symbol_width), rs_base));
                        item_spans_width += item_symbol_width;
                    }
                } else if item_symbol_width > 0 {
                    item_spans.push(Span::styled(spaces(item_symbol_width), rs_base));
                    item_spans_width += item_symbol_width;
                }
            }

            push_gutter_spans(
                item,
                sub_line,
                item_gutter_width,
                GutterSpanPushCtx {
                    left_base_style,
                    left_style_override,
                    rs_base,
                    item_spans: &mut item_spans,
                    item_spans_width: &mut item_spans_width,
                    spinner_draws: &mut spinner_draws,
                    content_inner,
                    dx,
                    dy,
                    virtual_line_idx: line_idx,
                    contrast_policy,
                },
            );

            if sub_line == 0 {
                if let Some(prefix) = effective_prefix.as_ref() {
                    let prefix_style = item
                        .prefix_style
                        .map_or(left_base_style, |style| left_base_style.patch(style));
                    let prefix_style = finalize_style(
                        prefix_style,
                        style_backdrop(left_base_style),
                        contrast_policy,
                    );
                    item_spans.push(Span::styled(
                        prefix.to_string(),
                        to_ratatui_style(prefix_style),
                    ));
                    item_spans_width += UnicodeWidthStr::width(prefix.as_ref());
                }
            } else if prefix_or_indent_w > 0 {
                let indent = spaces(prefix_or_indent_w as usize);
                item_spans.push(Span::styled(indent, rs_base));
                item_spans_width += prefix_or_indent_w as usize;
            }

            if row_padding.left > 0 {
                item_spans.push(Span::styled(spaces(row_padding.left as usize), rs_base));
                item_spans_width += row_padding.left as usize;
            }

            // Prepare item content spans
            let mut content_spans = Vec::with_capacity(line_spans_src.len());
            let mut left_content_width = 0usize;
            for s in line_spans_src {
                let (span_base_style, span_override_style) =
                    if s.row_style_policy == RowStylePolicy::Disabled {
                        (left_protected_content_base_style, Style::default())
                    } else {
                        (left_base_style, left_style_override)
                    };
                let content_style = list_rich_text_span_style(span_base_style, s.style);
                let mut span_style = content_style.patch(span_override_style);
                if s.row_style_policy == RowStylePolicy::PreserveForeground {
                    span_style.fg = content_style.fg;
                    span_style.fg_transform = content_style.fg_transform;
                    span_style.contrast_policy = content_style.contrast_policy;
                }
                let span_style =
                    finalize_style(span_style, style_backdrop(span_base_style), contrast_policy);
                left_content_width += UnicodeWidthStr::width(s.content.as_ref());
                content_spans.push(Span::styled(
                    s.content.as_ref(),
                    to_ratatui_style(span_style),
                ));
            }

            // Prepare right spans
            let mut right_spans = Vec::with_capacity(line_right_spans_src.len());
            let mut right_width = 0usize;
            for s in line_right_spans_src {
                let (span_base_style, span_override_style) =
                    if s.row_style_policy == RowStylePolicy::Disabled {
                        (right_protected_content_base_style, Style::default())
                    } else {
                        (right_base_style, right_style_override)
                    };
                let content_style = list_rich_text_span_style(span_base_style, s.style);
                let mut span_style = content_style.patch(span_override_style);
                if s.row_style_policy == RowStylePolicy::PreserveForeground {
                    span_style.fg = content_style.fg;
                    span_style.fg_transform = content_style.fg_transform;
                    span_style.contrast_policy = content_style.contrast_policy;
                }
                let span_style =
                    finalize_style(span_style, style_backdrop(span_base_style), contrast_policy);
                right_width += UnicodeWidthStr::width(s.content.as_ref());
                right_spans.push(Span::styled(
                    s.content.as_ref(),
                    to_ratatui_style(span_style),
                ));
            }

            if truncate_description_first {
                let left_budget = max_text_w
                    .saturating_sub(item_symbol_width as u16)
                    .saturating_sub(item_gutter_width as u16)
                    .saturating_sub(prefix_or_indent_w)
                    .saturating_sub(trailing_symbol_width as u16)
                    .saturating_sub(row_padding.horizontal());
                let max_right_width = if (left_content_width as u16) >= left_budget {
                    0
                } else {
                    left_budget.saturating_sub(left_content_width as u16)
                };
                right_spans = truncate_spans(right_spans, max_right_width);
                right_width = right_spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
            }

            // Apply max_description_width cap: limit how much space description can take
            if let Some(max_desc_w) = max_description_width {
                let capped = right_spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum::<usize>()
                    .min(max_desc_w as usize);
                right_spans = truncate_spans(right_spans, capped as u16);
                right_width = right_spans
                    .iter()
                    .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
            }

            // Truncate content to fit available width.
            let reserved_width = (item_symbol_width as u16)
                .saturating_add(item_gutter_width as u16)
                .saturating_add(prefix_or_indent_w)
                .saturating_add(trailing_symbol_width as u16)
                .saturating_add(right_width as u16)
                .saturating_add(row_padding.horizontal());
            let available_left_width = max_text_w.saturating_sub(reserved_width);
            // Apply max_label_width cap
            let available_left_width = if let Some(max_lbl_w) = max_label_width {
                available_left_width.min(max_lbl_w)
            } else {
                available_left_width
            };
            let truncated_left = truncate_spans(content_spans, available_left_width);
            let truncated_left_width: usize = truncated_left
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            item_spans.extend(truncated_left);
            item_spans_width += truncated_left_width;

            // Right-positioned active symbol renders immediately after the label.
            if active_right_symbol_width > 0
                && sub_line == item.symbol_line
                && let Some(symbol) = active_symbol
            {
                let sym_rs =
                    resolve_symbol_style(left_base_style, active_symbol_style, contrast_policy);
                item_spans.push(Span::styled(symbol, sym_rs));
                item_spans_width += active_right_symbol_width;
            }

            // The trailing selection cap closes the right edge of the *highlighted
            // region*, rendered last (after content, any fill, and right padding).
            // The highlighted region only spans the full row width when the caller
            // opts in via `selection_full_width` (or when right-aligned content must
            // be pushed to the edge); plain `item_horizontal_padding` stays interior
            // to the highlight and does not force a full-width bar. `edge_cap_width`
            // reserves the cap cell so the fill stops one column short of it.
            let render_selection_cap =
                selection_right_symbol_width > 0 && sub_line == item.symbol_line;
            let edge_cap_width = if render_selection_cap {
                selection_right_symbol_width
            } else {
                0
            };

            // Pad to push right_spans to the row edge / paint a full-width highlight.
            if right_width > 0 || selection_full_width {
                let total_available = max_text_w as usize;
                let padding = total_available
                    .saturating_sub(item_spans_width)
                    .saturating_sub(right_width)
                    .saturating_sub(row_padding.right as usize)
                    .saturating_sub(edge_cap_width);
                if padding > 0 {
                    item_spans.push(Span::styled(spaces(padding), rs_base));
                }
            }

            item_spans.extend(right_spans);

            if row_padding.right > 0 {
                item_spans.push(Span::styled(
                    spaces(row_padding.right as usize),
                    rs_right_base,
                ));
            }

            if render_selection_cap && let Some(symbol) = selection_symbol_right {
                // Shares the leading symbol's style so both ends of a "pill" match.
                item_spans.push(Span::styled(
                    symbol,
                    resolve_symbol_style(left_base_style, selection_symbol_style, contrast_policy),
                ));
            }

            lines.push(Line::from(item_spans));
            line_idx = line_idx.saturating_add(1);
            used_item_lines = used_item_lines.saturating_add(1);
        }
    }

    if bottom_indicator {
        let remaining_below = len.saturating_sub(end_index);
        lines.push(scroll_indicator_line(
            remaining_below,
            IndicatorDirection::Bottom,
            Style::default(),
            finalize_style(
                base_style.patch(scroll_indicator_style),
                style_backdrop(base_style),
                contrast_policy,
            ),
        ));
    }

    let text = RText::from(lines);
    let p = Paragraph::new(text).scroll((dy, dx));
    f.render_widget(p, effective_rrect);

    let content_clip = Rect {
        x: effective_rrect.x as i16,
        y: effective_rrect.y as i16,
        w: effective_rrect.width,
        h: effective_rrect.height,
    };
    let gutter_clip = clip_rect.map_or(content_clip, |clip| content_clip.intersection(&clip));
    for draw in spinner_draws {
        render_spinner(
            f,
            draw.spinner_style,
            draw.rect,
            to_ratatui_rect(draw.rect),
            SpinnerRenderCtx {
                frame: draw.frame,
                label: draw.label.as_deref(),
                gap: draw.gap,
                style: draw.style,
                label_style: draw.label_style,
                clip_rect: Some(gutter_clip),
                paint_glyph_caches: None,
            },
        );
    }

    // Render scrollbar.
    let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
    let use_half = thumb == DEFAULT_SCROLLBAR_THUMB;
    let scrollbar_metrics = if use_half {
        crate::widgets::list::utils::list_scrollbar_metrics_half(
            items,
            offset,
            inner.h as usize,
            show_scroll_indicators,
        )
    } else {
        crate::widgets::list::utils::list_scrollbar_metrics(
            items,
            offset,
            inner.h as usize,
            show_scroll_indicators,
        )
    };
    if use_integrated {
        let border_x = parent_integrated_v
            .map(|p| p.track_x)
            .unwrap_or_else(|| rect.x.saturating_add(rect.w.saturating_sub(1) as i16));
        let sb_rect = Rect {
            x: border_x,
            y: inner.y,
            w: 1,
            h: inner.h,
        };
        if sb_rect.h > 0 {
            let thumb_style = resolve_scrollbar_thumb_style(
                is_focused,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
            );
            let b_style = parent_integrated_v
                .map(|p| p.border_style_fallback)
                .unwrap_or(border_style);
            let track_glyph = parent_integrated_v.and_then(|p| p.track_glyph);
            let mut track_scratch = [0u8; 4];
            let border_char =
                integrated_vscrollbar_track_char(track_glyph, b_style, &mut track_scratch);
            let integrated_base_style = parent_integrated_v
                .map(|p| p.track_style)
                .unwrap_or(base_style);
            if let Some(metrics) = scrollbar_metrics {
                let appearance = IntegratedScrollbarAppearance {
                    thumb_char: thumb,
                    border_char,
                    base_style: integrated_base_style,
                    thumb_style,
                    track_style: scrollbar_track_style,
                    clip_rect: None,
                    metrics_cache: None,
                };
                if use_half {
                    render_integrated_vscrollbar_half_block(f, sb_rect, metrics, appearance);
                } else {
                    render_integrated_scrollbar_with_metrics(f, sb_rect, metrics, appearance);
                }
            }
        }
    } else if use_standalone {
        let sb_rect = Rect {
            x: inner.x.saturating_add(inner.w.saturating_sub(1) as i16),
            y: inner.y,
            w: 1,
            h: inner.h,
        };
        let thumb_style = resolve_scrollbar_thumb_style(
            is_focused,
            scrollbar_thumb_style,
            scrollbar_thumb_focus_style,
        );
        if let Some(metrics) = scrollbar_metrics {
            if use_half {
                render_vscrollbar_half_block(
                    f,
                    sb_rect,
                    metrics,
                    thumb_style,
                    scrollbar_track_style,
                    _clip_rrect,
                );
            } else {
                render_vscrollbar_with_metrics(
                    f,
                    sb_rect,
                    metrics,
                    thumb,
                    thumb_style,
                    scrollbar_track_style,
                    _clip_rrect,
                );
            }
        }
    }
}

pub(crate) fn render_list_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::core::node::Node,
    list_node: &crate::widgets::internal::ListNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !list_node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !list_node.disabled;
    let pointer_item_hover_mouse = if state
        .ctx
        .suppress_pointer_item_hover_nodes
        .is_some_and(|set| set.contains(&node_id))
    {
        None
    } else {
        state.ctx.mouse_pos
    };
    let contrast_policy = state.ctx.contrast_policy;
    let theme = node.active_theme();
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &list_node.hover_style);
    let item_hover_style = resolve_slot(theme, ThemeRole::ItemHover, &list_node.item_hover_style);
    let active_style = resolve_slot(theme, ThemeRole::Active, &list_node.active_style);
    let selection_style = resolve_selection_slot(
        theme,
        &list_node.selection_style,
        &list_node.unfocused_selection_style,
        is_focused,
    );
    let selection_symbol_style = effective_selection_symbol_style(
        list_node.selection_symbol_style,
        list_node.unfocused_selection_symbol_style,
        is_focused,
    );
    let (scrollbar_thumb_style, scrollbar_thumb_focus_style, scrollbar_track_style) =
        scrollbar_styles(
            theme,
            list_node.scrollbar_thumb_style,
            list_node.scrollbar_thumb_focus_style,
            list_node.scrollbar_track_style,
        );
    let parent_integrated_v = if !list_node.border
        && list_node.scrollbar
        && matches!(list_node.scrollbar_variant, ScrollbarVariant::Integrated)
    {
        ancestor_frame_integrated_vtrack(state, node.parent)
    } else {
        None
    };

    render_list(ListRenderParams {
        f: state.f,
        items: &list_node.items,
        selected: list_node.selected,
        offset: list_node.offset,
        style: with_theme_primary(theme, list_node.style),
        hover_style,
        item_hover_style,
        active_style,
        selection_style,
        active_symbol: list_node.active_symbol.as_deref(),
        active_symbol_position: list_node.active_symbol_position,
        active_symbol_style: with_theme_optional_primary(theme, list_node.active_symbol_style),
        selection_symbol: list_node.selection_symbol.as_deref(),
        selection_symbol_right: list_node.selection_symbol_right.as_deref(),
        selection_symbol_style: with_theme_optional_primary(theme, selection_symbol_style),
        unselected_symbol: list_node.unselected_symbol.as_deref(),
        symbol_column: list_node.symbol_column,
        gutter_gap: list_node.gutter_gap,
        gutter_for_non_selectable: list_node.gutter_for_non_selectable,
        selection_full_width: list_node.selection_full_width,
        item_horizontal_padding: list_node.item_horizontal_padding,
        header_horizontal_padding: list_node.header_horizontal_padding,
        border: list_node.border,
        border_style: list_node.border_style,
        title: list_node.title.as_deref(),
        title_style: with_theme_primary(theme, list_node.title_style),
        padding: list_node.padding,
        scrollbar: list_node.scrollbar,
        scrollbar_variant: list_node.scrollbar_variant,
        scrollbar_gap: list_node.scrollbar_gap,
        scrollbar_thumb: list_node.scrollbar_thumb,
        scrollbar_thumb_style,
        scrollbar_thumb_focus_style,
        scrollbar_track_style,
        show_scroll_indicators: list_node.show_scroll_indicators,
        scroll_indicator_style: with_theme_muted(theme, list_node.scroll_indicator_style),
        top_indicator: list_node.top_indicator,
        bottom_indicator: list_node.bottom_indicator,
        bottom_count: list_node.bottom_count,
        empty_text: list_node.empty_text.as_deref(),
        empty_text_style: with_theme_muted(theme, list_node.empty_text_style),
        is_focused,
        is_hovered,
        mouse_pos: pointer_item_hover_mouse,
        disabled: list_node.disabled,
        disabled_style: with_theme_muted(theme, list_node.disabled_style),
        rect,
        rrect,
        parent_integrated_v,
        clip_rect: clip_bounds,
        contrast_policy,
    });
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Modifier;
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use super::{ListRenderParams, effective_selection_symbol_style, render_list};
    use crate::app::ContrastPolicy;
    use crate::style::{
        BorderStyle, Color, ColorTransform, Padding, Rect, RowStylePolicy, ScrollbarVariant, Span,
        Style,
    };
    use crate::widgets::{ListItem, ListItemGutter, ListItemLine, ListSymbolPosition, Spinner};

    struct DrawListStateCaseInput {
        item: ListItem,
        selected: usize,
        item_hover_style: Style,
        active_style: Style,
        selection_style: Style,
        is_hovered: bool,
        mouse_pos: Option<(u16, u16)>,
    }

    struct DrawPlainListWithLeadingInput<'a> {
        items: &'a [ListItem],
        selected: usize,
        active_symbol: Option<&'a str>,
        active_symbol_position: ListSymbolPosition,
        selection_symbol: Option<&'a str>,
        unselected_symbol: Option<&'a str>,
        symbol_column: bool,
        gutter_gap: u16,
        gutter_for_non_selectable: bool,
        rect: Rect,
    }

    fn draw_list_state_case(input: DrawListStateCaseInput) -> Buffer {
        let DrawListStateCaseInput {
            item,
            selected,
            item_hover_style,
            active_style,
            selection_style,
            is_hovered,
            mouse_pos,
        } = input;
        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style,
                    active_style,
                    selection_style,
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered,
                    mouse_pos,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        terminal.backend().buffer().clone()
    }

    fn draw_plain_list_with_leading(input: DrawPlainListWithLeadingInput<'_>) -> Buffer {
        let DrawPlainListWithLeadingInput {
            items,
            selected,
            active_symbol,
            active_symbol_position,
            selection_symbol,
            unselected_symbol,
            symbol_column,
            gutter_gap,
            gutter_for_non_selectable,
            rect,
        } = input;
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items,
                    selected,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol,
                    active_symbol_position,
                    active_symbol_style: None,
                    selection_symbol,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol,
                    symbol_column,
                    gutter_gap,
                    gutter_for_non_selectable,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        terminal.backend().buffer().clone()
    }

    fn row_string(buffer: &Buffer, width: u16, y: u16) -> String {
        (0..width).map(|x| buffer[(x, y)].symbol()).collect()
    }

    #[test]
    fn effective_selection_symbol_style_uses_unfocused_override_only_without_focus() {
        let focused = Style::new().bg(crate::style::Color::Blue);
        let unfocused = Style::new().fg(crate::style::Color::Green);

        assert_eq!(
            effective_selection_symbol_style(Some(focused), Some(unfocused), true),
            Some(focused)
        );
        assert_eq!(
            effective_selection_symbol_style(Some(focused), Some(unfocused), false),
            Some(unfocused)
        );
        assert_eq!(
            effective_selection_symbol_style(Some(focused), None, false),
            Some(focused)
        );
    }

    #[test]
    fn above_rows_render_second_line_label() {
        let item = ListItem::from_spans([Span::new("Desc")])
            .symbol_line(1)
            .line(ListItemLine::new("Label"));

        let rect = Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some("> "),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();

        assert!(row0.contains("Desc"));
        assert!(row1.contains("Label"));
    }

    #[test]
    fn selection_symbol_right_caps_only_the_selected_row() {
        let items = [
            ListItem::from_spans([Span::new("Aa")]),
            ListItem::from_spans([Span::new("Bb")]),
        ];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some("["),
                    selection_symbol_right: Some("]"),
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();

        // Selected row is wrapped by both caps: "[Aa]".
        assert!(row0.starts_with("[Aa]"), "row0 = {row0:?}");
        // Non-selected row gets neither the leading nor the trailing cap.
        assert!(row1.contains("Bb"), "row1 = {row1:?}");
        assert!(!row1.contains('['), "row1 = {row1:?}");
        assert!(!row1.contains(']'), "row1 = {row1:?}");
    }

    #[test]
    fn selection_symbol_right_moves_to_edge_when_highlight_fills_row() {
        let items = [ListItem::from_spans([Span::new("Aa")])];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 1,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some("["),
                    selection_symbol_right: Some("]"),
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: true,
                    item_horizontal_padding: // selection_full_width
                    Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();

        // Left cap hugs the label, but with a full-width highlight the right cap
        // moves to the row's right edge instead of sitting next to the label.
        assert!(row0.starts_with("[Aa"), "row0 = {row0:?}");
        assert!(row0.ends_with(']'), "row0 = {row0:?}");
        assert!(!row0.starts_with("[Aa]"), "row0 = {row0:?}");
    }

    #[test]
    fn spinner_gutter_reserves_width_for_all_rows() {
        let items = [
            ListItem::new("Alpha").gutter(Spinner::new()),
            ListItem::new("Beta"),
        ];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 16,
            h: 3,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 1,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();

        assert!(row0.starts_with("⠋ Alpha"), "row0 was {row0:?}");
        assert!(row1.starts_with("  Beta"), "row1 was {row1:?}");
    }

    #[test]
    fn gutter_does_not_indent_group_headers() {
        let items = [
            ListItem::header("Pinned"),
            ListItem::new("Alpha").gutter(ListItemGutter::text(" ●")),
            ListItem::new("Beta").gutter(ListItemGutter::text("  ")),
        ];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 1,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 1,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();
        let row2 = (0..rect.w)
            .map(|x| buffer[(x, 2)].symbol())
            .collect::<String>();

        assert!(row0.starts_with("Pinned"), "row0 was {row0:?}");
        assert!(row1.starts_with(" ● Alpha"), "row1 was {row1:?}");
        assert!(row2.starts_with("   Beta"), "row2 was {row2:?}");
    }

    #[test]
    fn gutter_rows_share_one_marker_column_with_headers_left_aligned() {
        let items = [
            ListItem::header("Pinned"),
            ListItem::new("Current session").gutter(ListItemGutter::text(" ●")),
            ListItem::new("Pinned slot").gutter(ListItemGutter::text(" 2")),
            ListItem::new("Plain session"),
        ];
        let rect = Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 4,
        };

        let buffer = draw_plain_list_with_leading(DrawPlainListWithLeadingInput {
            items: &items,
            selected: 1,
            active_symbol: None,
            active_symbol_position: ListSymbolPosition::Left,
            selection_symbol: Some("> "),
            unselected_symbol: Some("  "),
            symbol_column: false,
            gutter_gap: 1,
            gutter_for_non_selectable: false,
            rect,
        });

        assert!(row_string(&buffer, rect.w, 0).starts_with("Pinned"));
        assert!(row_string(&buffer, rect.w, 1).starts_with(" ● Current session"));
        assert!(row_string(&buffer, rect.w, 2).starts_with(" 2 Pinned slot"));
        assert!(row_string(&buffer, rect.w, 3).starts_with("   Plain session"));
    }

    #[test]
    fn symbol_column_false_suppresses_selected_active_and_status_prefixes() {
        let items = [
            ListItem::new("Busy").status_symbol("!!"),
            ListItem::new("Active").active(true),
        ];
        let rect = Rect {
            x: 0,
            y: 0,
            w: 16,
            h: 2,
        };

        let buffer = draw_plain_list_with_leading(DrawPlainListWithLeadingInput {
            items: &items,
            selected: 0,
            active_symbol: Some("@@"),
            active_symbol_position: ListSymbolPosition::Left,
            selection_symbol: Some(">>"),
            unselected_symbol: Some(".."),
            symbol_column: false,
            gutter_gap: 0,
            gutter_for_non_selectable: false,
            rect,
        });

        assert!(row_string(&buffer, rect.w, 0).starts_with("Busy"));
        assert!(row_string(&buffer, rect.w, 1).starts_with("Active"));
    }

    #[test]
    fn status_spinner_replaces_unselected_symbol_column() {
        let items = [
            ListItem::new("Subagents").status_spinner(Spinner::new()),
            ListItem::new("Home").active(true),
        ];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 1,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: Some(" ● "),
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some(">  "),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some("   "),
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();

        assert!(row0.starts_with(" ⠋ Subagents"), "row0 was {row0:?}");
        assert!(row1.starts_with(" ● Home"), "row1 was {row1:?}");
    }

    #[test]
    fn status_symbol_takes_priority_over_selection_symbol() {
        let items = [ListItem::new("Working").status_spinner(Spinner::new())];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some(">  "),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some("   "),
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();

        assert!(row0.starts_with(" ⠋ Working"), "row0 was {row0:?}");
    }

    #[test]
    fn empty_selected_and_unselected_symbols_do_not_inherit_active_symbol_indent() {
        let item = ListItem::new("Unit tests only")
            .numbered(1)
            .line(ListItemLine::new("Fast, isolated"));

        let rect = Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: Some("✓ "),
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some(""),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some(""),
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        let row1 = (0..rect.w)
            .map(|x| buffer[(x, 1)].symbol())
            .collect::<String>();

        assert!(row0.starts_with("1. Unit tests only"));
        assert!(row1.starts_with("   Fast, isolated"));
    }

    #[test]
    fn active_symbol_can_render_to_the_right_of_label() {
        let item = ListItem::new("Unit tests only").active(true);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: Some(" ✓"),
                    active_symbol_position: ListSymbolPosition::Right,
                    active_symbol_style: None,
                    selection_symbol: Some(""),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some(""),
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();

        assert!(row0.starts_with("Unit tests only ✓"));
    }

    #[test]
    fn symbol_column_false_keeps_right_active_symbol_without_left_indent() {
        let item = ListItem::new("Unit").active(true);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: Some(" ✓"),
                    active_symbol_position: ListSymbolPosition::Right,
                    active_symbol_style: None,
                    selection_symbol: Some("> "),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some("  "),
                    symbol_column: false,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row0 = (0..rect.w)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();

        assert!(row0.starts_with("Unit ✓"), "row0 was {row0:?}");
    }

    #[test]
    fn numbered_rows_align_to_widest_number_prefix() {
        let items = vec![
            ListItem::new("Security")
                .numbered(1)
                .line(ListItemLine::new("Tutaj opis do Security."))
                .line(ListItemLine::new("Druga linia opisu.")),
            ListItem::new("Laziness")
                .numbered(10)
                .line(ListItemLine::new("A tutaj opis do Laziness."))
                .line(ListItemLine::new("Druga linia opisu.")),
        ];

        let rect = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 6,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some(""),
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: Some(""),
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let rows = (0..rect.h)
            .map(|y| {
                (0..rect.w)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(rows[0].starts_with(" 1. Security"));
        assert!(rows[1].starts_with("    Tutaj opis do Security."));
        assert!(rows[2].starts_with("    Druga linia opisu."));
        assert!(rows[3].starts_with("10. Laziness"));
        assert!(rows[4].starts_with("    A tutaj opis do Laziness."));
        assert!(rows[5].starts_with("    Druga linia opisu."));
    }

    #[test]
    fn indicators_disabled_fill_full_viewport_height() {
        let items: Vec<ListItem> = (0..8).map(|i| ListItem::new(format!("Row {i}"))).collect();

        let rect = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 6,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &items,
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row5 = (0..rect.w)
            .map(|x| buffer[(x, 5)].symbol())
            .collect::<String>();

        assert!(row5.contains("Row 5"));
    }

    #[test]
    fn active_symbol_style_inherits_selected_background() {
        let item = ListItem::new("Label").active(true);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::new().bg(crate::style::Color::Blue),
                    active_symbol: Some(">"),
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: Some(Style::new().fg(crate::style::Color::Yellow)),
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Yellow);
        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Blue);
    }

    #[test]
    fn active_style_overrides_selection_style_when_both_apply() {
        let item = ListItem::new("Label").active(true);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::new().bg(crate::style::Color::Green),
                    selection_style: Style::new().bg(crate::style::Color::Blue),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Green);
    }

    #[test]
    fn selected_concrete_style_overrides_hover_concrete_style_when_hovered() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label"),
            selected: 0,
            item_hover_style: Style::new().bg(Color::Red),
            active_style: Style::default(),
            selection_style: Style::new().bg(Color::Blue),
            is_hovered: true,
            mouse_pos: Some((0, 0)),
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Blue);
    }

    #[test]
    fn active_concrete_style_overrides_selection_and_hover_concrete_style_when_hovered() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label").active(true),
            selected: 0,
            item_hover_style: Style::new().bg(Color::Red),
            active_style: Style::new().bg(Color::Green),
            selection_style: Style::new().bg(Color::Blue),
            is_hovered: true,
            mouse_pos: Some((0, 0)),
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Green);
    }

    #[test]
    fn active_transform_only_style_applies_once_to_row_styled_content() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label")
                .style(Style::new().bg(Color::Rgb(100, 100, 100)))
                .active(true),
            selected: 0,
            item_hover_style: Style::default(),
            active_style: Style::new().transform_bg(ColorTransform::Dim(0.5)),
            selection_style: Style::default(),
            is_hovered: false,
            mouse_pos: None,
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(50, 50, 50));
    }

    #[test]
    fn selected_active_transform_applies_once_after_selection_concrete_bg() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label").active(true),
            selected: 0,
            item_hover_style: Style::default(),
            active_style: Style::new().transform_bg(ColorTransform::Dim(0.5)),
            selection_style: Style::new().bg(Color::Rgb(100, 100, 100)),
            is_hovered: false,
            mouse_pos: None,
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(50, 50, 50));
    }

    #[test]
    fn hover_transform_applies_over_selected_concrete_style_when_hovered() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label"),
            selected: 0,
            item_hover_style: Style::new().transform_bg(ColorTransform::Lighten(1.0)),
            active_style: Style::default(),
            selection_style: Style::new().bg(Color::Rgb(10, 20, 30)),
            is_hovered: true,
            mouse_pos: Some((0, 0)),
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(255, 255, 255));
    }

    #[test]
    fn hover_transform_applies_over_active_concrete_style_when_hovered() {
        let buffer = draw_list_state_case(DrawListStateCaseInput {
            item: ListItem::new("Label").active(true),
            selected: 0,
            item_hover_style: Style::new().transform_bg(ColorTransform::Lighten(1.0)),
            active_style: Style::new().bg(Color::Rgb(10, 20, 30)),
            selection_style: Style::new().bg(Color::Blue),
            is_hovered: true,
            mouse_pos: Some((0, 0)),
        });

        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Rgb(255, 255, 255));
    }

    #[test]
    fn selection_symbol_style_inherits_selected_background() {
        let item = ListItem::new("Label");

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::new().bg(crate::style::Color::Blue),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: Some(">"),
                    selection_symbol_right: None,
                    selection_symbol_style: Some(Style::new().fg(crate::style::Color::Yellow)),
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Yellow);
        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Blue);
    }

    #[test]
    fn protected_span_keeps_explicit_fg_under_selection() {
        let item = ListItem::from_spans([
            Span::new("Label "),
            Span::new("Desc")
                .fg(crate::style::Color::Red)
                .row_style_policy(RowStylePolicy::Disabled),
        ]);

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::new().fg(crate::style::Color::Green),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(6, 0)].fg, ratatui::style::Color::Red);
    }

    #[test]
    fn styled_spans_do_not_inherit_row_bold() {
        let item = ListItem::from_spans([
            Span::new("Label").style(Style::new().fg(crate::style::Color::White))
        ])
        .style(Style::new().bold());

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert!(!buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unstyled_spans_still_inherit_row_bold() {
        let item = ListItem::new("Label").style(Style::new().bold());

        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 2,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_list(ListRenderParams {
                    f,
                    items: &[item],
                    selected: 0,
                    offset: 0,
                    style: Style::default(),
                    hover_style: Style::default(),
                    item_hover_style: Style::default(),
                    active_style: Style::default(),
                    selection_style: Style::default(),
                    active_symbol: None,
                    active_symbol_position: ListSymbolPosition::Left,
                    active_symbol_style: None,
                    selection_symbol: None,
                    selection_symbol_right: None,
                    selection_symbol_style: None,
                    unselected_symbol: None,
                    symbol_column: true,
                    gutter_gap: 0,
                    gutter_for_non_selectable: false,
                    selection_full_width: false,
                    item_horizontal_padding: Padding::default(),
                    header_horizontal_padding: Padding::default(),
                    border: false,
                    border_style: BorderStyle::Plain,
                    title: None,
                    title_style: Style::default(),
                    padding: Padding::default(),
                    scrollbar: false,
                    scrollbar_variant: ScrollbarVariant::Standalone,
                    scrollbar_gap: 0,
                    scrollbar_thumb: None,
                    scrollbar_thumb_style: None,
                    scrollbar_thumb_focus_style: None,
                    scrollbar_track_style: None,
                    show_scroll_indicators: false,
                    scroll_indicator_style: Style::default(),
                    top_indicator: false,
                    bottom_indicator: false,
                    bottom_count: 0,
                    empty_text: None,
                    empty_text_style: Style::default(),
                    is_focused: true,
                    is_hovered: false,
                    mouse_pos: None,
                    disabled: false,
                    disabled_style: Style::default(),
                    rect,
                    rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
                    parent_integrated_v: None,
                    clip_rect: None,
                    contrast_policy: ContrastPolicy::Off,
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    }
}
