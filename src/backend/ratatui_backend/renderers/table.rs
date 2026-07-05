use std::borrow::Cow;
use std::cell::RefCell;

use ratatui::layout::{Constraint, Layout, Rect as RatatuiRect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IndicatorDirection, IntegratedScrollbarAppearance,
    ScrollbarAppearance, ScrollbarScrollState, calculate_visible_borders, finalize_style,
    integrated_vscrollbar_track_char, render_integrated_scrollbar, render_vscrollbar,
    resolve_interactive_style_raw, resolve_scrollbar_thumb_style, scroll_indicator_line, spaces,
    style_backdrop, style_paints_bg, to_ratatui_border_set, to_ratatui_border_type,
    to_ratatui_rect, to_ratatui_style, truncate_end_with_ellipsis,
};
use crate::backend::ratatui_backend::render::{
    FrameIntegratedVTrack, RenderState, ancestor_frame_integrated_vtrack,
};
use crate::backend::ratatui_backend::renderers::theme::{
    scrollbar_styles, with_theme_accent, with_theme_muted, with_theme_optional_primary,
    with_theme_primary,
};
use crate::core::node::NodeId;
use crate::style::resolve::{Durability, StateLayer, resolve_state_cascade};
use crate::style::{BorderStyle, Padding, Rect, ScrollbarVariant, Style, ThemeRole, resolve_slot};
use crate::utils::scrollbar::ScrollbarMetricsCache;
use crate::widgets::table::{
    resolved_row_height, resolved_row_total_height, row_index_at_visual_offset,
    table_header_reserved_height, visible_rows_for_height,
};
use crate::widgets::{ColumnWidth, TableDisclosureState, TableRow, TableRowRole};

pub(crate) struct TableLayoutCtx {
    pub selected: usize,
    pub column_spacing: u16,
    pub row_gap: u16,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub offset: usize,
}

pub(crate) struct TableStyleCtx<'a> {
    pub column_styles: &'a [Style],
    pub row_styles: &'a [Style],
    pub style: Style,
    pub hover_style: Style,
    pub item_hover_style: Style,
    pub alternating_row_style: Option<Style>,
    pub row_style_full_width: bool,
    pub selection_style: Style,
    pub selection_symbol: Option<&'a str>,
    pub selection_symbol_style: Option<Style>,
    pub unselected_symbol: Option<&'a str>,
    pub disabled_style: Style,
}

pub(crate) struct TableScrollbarCtx<'a> {
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
    pub parent_integrated_v: Option<FrameIntegratedVTrack>,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
}

pub(crate) struct TableInspectorCtx<'a> {
    pub inspector: bool,
    pub inspector_key_style: Style,
    pub inspector_value_style: Style,
    pub inspector_section_style: Style,
    pub inspector_separator_style: Style,
    pub inspector_indent_size: u16,
    pub inspector_collapsed_symbol: &'a str,
    pub inspector_expanded_symbol: &'a str,
    pub inspector_separator_char: char,
}

pub(crate) struct TableRenderCtx {
    pub is_focused: bool,
    pub is_hovered: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub disabled: bool,
    pub rect: Rect,
    pub rrect: RatatuiRect,
    pub clip_rect: Option<Rect>,
    pub contrast_policy: ContrastPolicy,
}

struct TableStateStyleCtx<'a> {
    hover_style: &'a Style,
    selection_style: &'a Style,
    disabled_style: &'a Style,
    hovered: bool,
    selected: bool,
    disabled: bool,
}

pub(crate) struct TableRenderParts<'a> {
    pub layout: TableLayoutCtx,
    pub styles: TableStyleCtx<'a>,
    pub scrollbar: TableScrollbarCtx<'a>,
    pub inspector: TableInspectorCtx<'a>,
}

pub(crate) fn render_table(
    f: &mut ratatui::Frame<'_>,
    rows: &[TableRow],
    header: Option<&TableRow>,
    widths: &[ColumnWidth],
    parts: TableRenderParts<'_>,
    ctx: TableRenderCtx,
) {
    let TableRenderParts {
        layout,
        styles,
        scrollbar,
        inspector,
    } = parts;
    let TableLayoutCtx {
        selected,
        column_spacing,
        row_gap,
        border,
        border_style,
        padding,
        offset,
    } = layout;
    let TableStyleCtx {
        column_styles,
        row_styles,
        style,
        hover_style,
        item_hover_style,
        alternating_row_style,
        row_style_full_width,
        selection_style,
        selection_symbol,
        selection_symbol_style,
        unselected_symbol,
        disabled_style,
    } = styles;
    let TableScrollbarCtx {
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
        bottom_count,
        parent_integrated_v,
        metrics_cache,
    } = scrollbar;
    let TableInspectorCtx {
        inspector,
        inspector_key_style,
        inspector_value_style,
        inspector_section_style,
        inspector_separator_style,
        inspector_indent_size,
        inspector_collapsed_symbol,
        inspector_expanded_symbol,
        inspector_separator_char,
    } = inspector;
    let TableRenderCtx {
        is_focused,
        is_hovered,
        mouse_pos,
        disabled,
        rect,
        rrect,
        clip_rect,
        contrast_policy,
    } = ctx;
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
    let mut hl_style = selection_style;
    let _clip_rrect = clip_rect.map(to_ratatui_rect);

    if disabled {
        hl_style = hl_style.patch(disabled_style);
    }

    let mut inner = rect;

    if border {
        let borders = calculate_visible_borders(rect, clip_rect);
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(border_style))
            .style(to_ratatui_style(base_style));

        if let Some(set) = to_ratatui_border_set(border_style) {
            block = block.border_set(set);
        }

        f.render_widget(block, rrect);

        // Always reserve space for borders if enabled, even if clipped
        inner.x = inner.x.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.y = inner.y.saturating_add(1);
        inner.h = inner.h.saturating_sub(2);
    } else if style_paints_bg(base_style) {
        // Render background if no border
        let block = Block::default().style(to_ratatui_style(base_style));
        f.render_widget(block, rrect);
    }

    inner = inner.inset(padding);

    if inner.w == 0 || inner.h == 0 {
        return;
    }

    // Scrollbar layout logic
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
    if effective_rrect.width == 0 || effective_rrect.height == 0 {
        return;
    }

    let header_height = table_header_reserved_height(header, rows.len(), row_gap);
    let hovered_row = if disabled || !is_hovered {
        None
    } else {
        mouse_pos.and_then(|(mx, my)| {
            let within_x = (mx as i16) >= content_inner.x
                && (mx as i16) < content_inner.x.saturating_add(content_inner.w as i16);
            let within_y = (my as i16) >= content_inner.y
                && (my as i16) < content_inner.y.saturating_add(content_inner.h as i16);
            if !within_x || !within_y {
                return None;
            }
            let top_reserved = if show_scroll_indicators && top_indicator {
                1
            } else {
                0
            };
            let row_start_y = content_inner
                .y
                .saturating_add(top_reserved as i16)
                .saturating_add(header_height as i16);
            if (my as i16) < row_start_y {
                return None;
            }
            let y = (my as i16).saturating_sub(row_start_y) as u16;
            row_index_at_visual_offset(rows, offset, y, row_gap)
        })
    };

    // Convert constraints
    let constraints: Vec<Constraint> = widths
        .iter()
        .map(|w| match w {
            ColumnWidth::Fixed(v) => Constraint::Length(*v),
            ColumnWidth::Percent(v) => Constraint::Percentage(*v),
            ColumnWidth::Min(v) => Constraint::Min(*v),
            ColumnWidth::Max(v) => Constraint::Max(*v),
            ColumnWidth::Fill(v) => Constraint::Fill(*v),
        })
        .collect();

    let layout_area = to_ratatui_rect(content_inner);
    let col_rects = Layout::horizontal(&constraints)
        .spacing(column_spacing)
        .split(layout_area);

    let mut y_offset = 0u16;

    if show_scroll_indicators && top_indicator {
        let indicator_rect = RatatuiRect {
            x: effective_rrect.x,
            y: effective_rrect.y.saturating_add(y_offset),
            width: effective_rrect.width,
            height: 1,
        };
        if indicator_rect.area() > 0 {
            let line = scroll_indicator_line(
                offset,
                IndicatorDirection::Top,
                Style::default(),
                finalize_style(
                    base_style.patch(scroll_indicator_style),
                    style_backdrop(base_style),
                    contrast_policy,
                ),
            );
            f.render_widget(Paragraph::new(line), indicator_rect);
        }
        y_offset = y_offset.saturating_add(1);
    }

    if let Some(h) = header {
        let row_height = resolved_row_height(h);
        let total_row_height = resolved_row_total_height(h);

        if y_offset < content_inner.h {
            let row_y = content_inner.y.saturating_add(y_offset as i16);

            for (col_idx, cell) in h.cells.iter().enumerate() {
                if col_idx >= col_rects.len() {
                    break;
                }
                let mut col_rect = col_rects[col_idx];
                col_rect.y = row_y.max(0) as u16;
                col_rect.height = row_height;

                let effective_cell = col_rect.intersection(rrect);
                if effective_cell.width > 0 && effective_cell.height > 0 {
                    let column_style = column_styles.get(col_idx).copied().unwrap_or_default();
                    let mut s = h.style.patch(column_style).patch(cell.style);
                    if disabled {
                        s = s.patch(disabled_style);
                    }
                    let cell_style = finalize_style(s, style_backdrop(h.style), contrast_policy);

                    let dx = (effective_cell.x as i32)
                        .saturating_sub(col_rect.x as i32)
                        .max(0) as u16;
                    let dy = (effective_cell.y as i32)
                        .saturating_sub(col_rect.y as i32)
                        .max(0) as u16;

                    let symbol = if col_idx == 0 {
                        unselected_symbol
                    } else {
                        None
                    };
                    let text = build_cell_text(
                        None,
                        cell.content.as_ref(),
                        col_rect.width,
                        row_height,
                        symbol,
                        cell_style,
                        cell_style,
                    );
                    let p = Paragraph::new(text).scroll((dy, dx));
                    f.render_widget(p, effective_cell);
                }
            }
        }
        y_offset = y_offset.saturating_add(total_row_height);
        if !rows.is_empty() {
            y_offset = y_offset.saturating_add(row_gap);
        }
    }

    let table_row_state = |hovered: bool, selected: bool| TableStateStyleCtx {
        hover_style: &item_hover_style,
        selection_style: &hl_style,
        disabled_style: &disabled_style,
        hovered,
        selected,
        disabled,
    };

    for (idx, row) in rows.iter().enumerate().skip(offset) {
        if y_offset >= content_inner.h {
            break;
        }

        let row_height = resolved_row_height(row);
        let total_row_height = resolved_row_total_height(row);

        let row_y = content_inner.y.saturating_add(y_offset as i16);
        let row_hovered = hovered_row == Some(idx);
        let is_selected = idx == selected && is_focused && !disabled;

        let row_raw_style = if matches!(row.role, TableRowRole::Normal) && idx % 2 == 1 {
            alternating_row_style.unwrap_or_default()
        } else {
            Style::default()
        }
        .patch(row.style)
        .patch(row_styles.get(idx).copied().unwrap_or_default());
        let row_base_style = finalize_style(
            resolve_table_state_style(row_raw_style, table_row_state(row_hovered, is_selected)),
            style_backdrop(base_style),
            contrast_policy,
        );

        if row_style_full_width {
            let row_rect = RatatuiRect {
                x: content_inner.x.max(0) as u16,
                y: row_y.max(0) as u16,
                width: content_inner.w,
                height: row_height,
            }
            .intersection(rrect);

            if row_rect.width > 0 && row_rect.height > 0 {
                let row_bg = Block::default().style(to_ratatui_style(row_base_style));
                f.render_widget(row_bg, row_rect);
            }
        }

        let inspector_prefix = if inspector {
            build_inspector_prefix(
                row,
                inspector_indent_size,
                inspector_collapsed_symbol,
                inspector_expanded_symbol,
            )
        } else {
            String::new()
        };

        if matches!(row.role, TableRowRole::Separator) {
            let mut separator_style = resolve_table_state_style(
                row_raw_style.patch(inspector_separator_style),
                table_row_state(row_hovered, is_selected),
            );
            separator_style = finalize_style(
                separator_style,
                style_backdrop(row_base_style),
                contrast_policy,
            );

            let row_rect = RatatuiRect {
                x: content_inner.x.max(0) as u16,
                y: row_y.max(0) as u16,
                width: content_inner.w,
                height: row_height,
            }
            .intersection(rrect);

            if row_rect.width > 0 && row_rect.height > 0 {
                let line = inspector_separator_char
                    .to_string()
                    .repeat(row_rect.width as usize);
                let paragraph = Paragraph::new(Line::from(Span::styled(
                    line,
                    to_ratatui_style(separator_style),
                )));
                f.render_widget(paragraph, row_rect);
            }

            y_offset = y_offset.saturating_add(total_row_height);
            if idx + 1 < rows.len() {
                y_offset = y_offset.saturating_add(row_gap);
            }
            continue;
        }

        if matches!(row.role, TableRowRole::Section) {
            let mut section_style = resolve_table_state_style(
                row_raw_style.patch(inspector_section_style),
                table_row_state(row_hovered, is_selected),
            );
            section_style = finalize_style(
                section_style,
                style_backdrop(row_base_style),
                contrast_policy,
            );

            let title = row
                .cells
                .first()
                .map(|cell| cell.content.as_ref())
                .unwrap_or_default();
            let symbol = if is_selected {
                selection_symbol
            } else {
                unselected_symbol
            };
            let symbol_style = if is_selected {
                selection_symbol_style.unwrap_or(section_style)
            } else {
                section_style
            };

            let row_rect = RatatuiRect {
                x: content_inner.x.max(0) as u16,
                y: row_y.max(0) as u16,
                width: content_inner.w,
                height: row_height,
            }
            .intersection(rrect);

            if row_rect.width > 0 && row_rect.height > 0 {
                let text = build_cell_text(
                    Some(inspector_prefix.as_str()),
                    title,
                    row_rect.width,
                    row_height,
                    symbol,
                    section_style,
                    symbol_style,
                );
                f.render_widget(Paragraph::new(text), row_rect);
            }

            y_offset = y_offset.saturating_add(total_row_height);
            if idx + 1 < rows.len() {
                y_offset = y_offset.saturating_add(row_gap);
            }
            continue;
        }

        for (col_idx, cell) in row.cells.iter().enumerate() {
            if col_idx >= col_rects.len() {
                break;
            }
            let mut col_rect = col_rects[col_idx];
            col_rect.y = row_y as u16;
            col_rect.height = row_height;

            let effective_cell = col_rect.intersection(rrect);
            if effective_cell.width > 0 && effective_cell.height > 0 {
                let column_style = column_styles.get(col_idx).copied().unwrap_or_default();
                let mut final_style = resolve_table_state_style(
                    row_raw_style.patch(column_style).patch(cell.style),
                    table_row_state(row_hovered, is_selected),
                );
                if inspector {
                    if col_idx == 0 {
                        final_style = final_style.patch(inspector_key_style);
                    } else {
                        final_style = final_style.patch(inspector_value_style);
                    }
                }
                final_style =
                    finalize_style(final_style, style_backdrop(row_base_style), contrast_policy);

                let dx = (effective_cell.x as i32)
                    .saturating_sub(col_rect.x as i32)
                    .max(0) as u16;
                let dy = (effective_cell.y as i32)
                    .saturating_sub(col_rect.y as i32)
                    .max(0) as u16;

                let symbol = if col_idx == 0 {
                    if is_selected {
                        selection_symbol
                    } else {
                        unselected_symbol
                    }
                } else {
                    None
                };
                let symbol_style = if is_selected {
                    selection_symbol_style.unwrap_or(final_style)
                } else {
                    final_style
                };
                let text = build_cell_text(
                    if inspector && col_idx == 0 {
                        Some(inspector_prefix.as_str())
                    } else {
                        None
                    },
                    cell.content.as_ref(),
                    col_rect.width,
                    row_height,
                    symbol,
                    final_style,
                    symbol_style,
                );
                let p = Paragraph::new(text).scroll((dy, dx));
                f.render_widget(p, effective_cell);
            }
        }

        y_offset = y_offset.saturating_add(total_row_height);
        if idx + 1 < rows.len() {
            y_offset = y_offset.saturating_add(row_gap);
        }
    }

    let len = rows.len();
    let available_height = inner.h.saturating_sub(header_height);
    let visible_rows = visible_rows_for_height(rows, offset, available_height, row_gap);

    let scrollbar_visible = if len > visible_rows {
        visible_rows.saturating_sub(1).max(1)
    } else {
        visible_rows
    };

    if use_integrated {
        let border_x = parent_integrated_v
            .map(|p| p.track_x)
            .unwrap_or_else(|| rect.x.saturating_add(rect.w.saturating_sub(1) as i16));

        let mut sb_rect = Rect {
            x: border_x,
            y: inner.y,
            w: 1,
            h: inner.h,
        };

        let sb_y = sb_rect.y.saturating_add(header_height as i16);
        let sb_h = sb_rect.h.saturating_sub(header_height);
        sb_rect.y = sb_y;
        sb_rect.h = sb_h;

        if sb_rect.h > 0 && len > 0 {
            let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
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
            render_integrated_scrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset,
                    visible: scrollbar_visible,
                    total: len,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: thumb,
                    border_char,
                    base_style: integrated_base_style,
                    thumb_style,
                    track_style: scrollbar_track_style,
                    clip_rect: None,
                    metrics_cache,
                },
            );
        }
    } else if use_standalone {
        let mut sb_rect = Rect {
            x: inner.x.saturating_add(inner.w.saturating_sub(1) as i16),
            y: inner.y,
            w: 1,
            h: inner.h,
        };

        let sb_y = sb_rect.y.saturating_add(header_height as i16);
        let sb_h = sb_rect.h.saturating_sub(header_height);
        sb_rect.y = sb_y;
        sb_rect.h = sb_h;

        if sb_rect.h > 0 && len > 0 {
            let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
            let thumb_style = resolve_scrollbar_thumb_style(
                is_focused,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
            );
            render_vscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset,
                    visible: scrollbar_visible,
                    total: len,
                },
                ScrollbarAppearance {
                    thumb_char: thumb,
                    thumb_style,
                    track_style: scrollbar_track_style,
                    clip_rect: clip_rect.map(to_ratatui_rect),
                    metrics_cache,
                },
            );
        }
    }

    if show_scroll_indicators && bottom_indicator {
        let indicator_rect = RatatuiRect {
            x: effective_rrect.x,
            y: effective_rrect
                .y
                .saturating_add(effective_rrect.height.saturating_sub(1)),
            width: effective_rrect.width,
            height: 1,
        };
        if indicator_rect.area() > 0 {
            let line = scroll_indicator_line(
                bottom_count,
                IndicatorDirection::Bottom,
                Style::default(),
                finalize_style(
                    base_style.patch(scroll_indicator_style),
                    style_backdrop(base_style),
                    contrast_policy,
                ),
            );
            f.render_widget(Paragraph::new(line), indicator_rect);
        }
    }
}

fn resolve_table_state_style(base: Style, ctx: TableStateStyleCtx<'_>) -> Style {
    let TableStateStyleCtx {
        hover_style,
        selection_style,
        disabled_style,
        hovered,
        selected,
        disabled,
    } = ctx;
    let empty = Style::default();
    let hover_style = if hovered && !disabled {
        hover_style
    } else {
        &empty
    };
    let selection_style = if selected && !disabled {
        selection_style
    } else {
        &empty
    };
    let disabled_style = if disabled { disabled_style } else { &empty };

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
                style: disabled_style,
                durability: Durability::Durable,
            },
        ],
    )
}

fn build_cell_text<'a>(
    prefix: Option<&'a str>,
    content: &'a str,
    width: u16,
    row_height: u16,
    symbol: Option<&'a str>,
    content_style: Style,
    symbol_style: Style,
) -> Text<'a> {
    if width == 0 || row_height == 0 {
        return Text::default();
    }

    let symbol_width = symbol
        .map(|s| UnicodeWidthStr::width(s) as u16)
        .unwrap_or(0);
    let available = width.saturating_sub(symbol_width);
    let content_style = to_ratatui_style(content_style);
    let symbol_style = to_ratatui_style(symbol_style);
    let prefix = prefix.unwrap_or("");
    let prefix_width = UnicodeWidthStr::width(prefix) as u16;

    let line_count = content.lines().count().max(1);
    let mut lines_out = Vec::with_capacity((row_height as usize).min(line_count));

    let push_line = |idx: usize, line: &'a str, lines_out: &mut Vec<Line<'a>>| {
        if idx as u16 >= row_height {
            return false;
        }

        let mut spans = Vec::with_capacity(3);
        if let Some(sym) = symbol {
            if idx == 0 {
                spans.push(Span::styled(sym, symbol_style));
            } else if symbol_width > 0 {
                spans.push(Span::styled(spaces(symbol_width as usize), content_style));
            }
        }

        let line_width = if idx == 0 && !prefix.is_empty() {
            if prefix_width >= available {
                spans.push(Span::styled(
                    truncate_end_with_ellipsis(prefix, available),
                    content_style,
                ));
                lines_out.push(Line::from(spans));
                return true;
            }

            spans.push(Span::styled(prefix, content_style));
            available.saturating_sub(prefix_width)
        } else {
            available
        };

        let text = if line_width == 0 {
            Cow::Borrowed("")
        } else {
            truncate_end_with_ellipsis(line, line_width)
        };

        if !text.is_empty() || spans.is_empty() {
            spans.push(Span::styled(text, content_style));
        }
        lines_out.push(Line::from(spans));
        true
    };

    if content.is_empty() {
        push_line(0, "", &mut lines_out);
    } else {
        for (idx, line) in content.lines().enumerate() {
            if !push_line(idx, line, &mut lines_out) {
                break;
            }
        }
    }

    if lines_out.is_empty() {
        lines_out.push(Line::from(""));
    }

    Text::from(lines_out)
}

fn build_inspector_prefix(
    row: &TableRow,
    indent_size: u16,
    collapsed_symbol: &str,
    expanded_symbol: &str,
) -> String {
    let mut prefix = String::new();

    if row.depth > 0 {
        prefix.push_str(spaces((row.depth as usize).saturating_mul(indent_size as usize)).as_ref());
    }

    if let Some(disclosure) = row.disclosure {
        let symbol = match disclosure {
            TableDisclosureState::Collapsed => collapsed_symbol,
            TableDisclosureState::Expanded => expanded_symbol,
        };
        prefix.push_str(symbol);
        if !symbol.is_empty() {
            prefix.push(' ');
        }
    }

    prefix
}

pub(crate) fn render_table_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::core::node::Node,
    table_node: &crate::widgets::internal::TableNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !table_node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !table_node.disabled;
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
    let theme = state.ctx.tree.node(node_id).active_theme();
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &table_node.hover_style);
    let item_hover_style = resolve_slot(theme, ThemeRole::ItemHover, &table_node.item_hover_style);
    let selection_style = resolve_slot(theme, ThemeRole::Selection, &table_node.selection_style);
    let mut header = table_node.header.clone();
    if let Some(header) = header.as_mut() {
        header.style = with_theme_primary(theme, header.style);
    }
    let (scrollbar_thumb_style, scrollbar_thumb_focus_style, scrollbar_track_style) =
        scrollbar_styles(
            theme,
            table_node.scrollbar_thumb_style,
            table_node.scrollbar_thumb_focus_style,
            table_node.scrollbar_track_style,
        );
    let parent_integrated_v = if !table_node.border
        && table_node.scrollbar
        && matches!(table_node.scrollbar_variant, ScrollbarVariant::Integrated)
    {
        ancestor_frame_integrated_vtrack(state, node.parent)
    } else {
        None
    };

    let scrollbar_cache = &state.ctx.scrollbar_metrics_cache;
    let f = &mut *state.f;
    render_table(
        f,
        &table_node.rows,
        header.as_ref(),
        &table_node.widths,
        TableRenderParts {
            layout: TableLayoutCtx {
                selected: table_node.selected,
                column_spacing: table_node.column_spacing,
                row_gap: table_node.row_gap,
                border: table_node.border,
                border_style: table_node.border_style,
                padding: table_node.padding,
                offset: table_node.offset,
            },
            styles: TableStyleCtx {
                column_styles: &table_node.column_styles,
                row_styles: &table_node.row_styles,
                style: with_theme_primary(theme, table_node.style),
                hover_style,
                item_hover_style,
                alternating_row_style: with_theme_optional_primary(
                    theme,
                    table_node.alternating_row_style,
                ),
                row_style_full_width: table_node.row_style_full_width,
                selection_style,
                selection_symbol: table_node.selection_symbol.as_deref(),
                selection_symbol_style: with_theme_optional_primary(
                    theme,
                    table_node.selection_symbol_style,
                ),
                unselected_symbol: table_node.unselected_symbol.as_deref(),
                disabled_style: with_theme_muted(theme, table_node.disabled_style),
            },
            scrollbar: TableScrollbarCtx {
                scrollbar: table_node.scrollbar,
                scrollbar_variant: table_node.scrollbar_variant,
                scrollbar_gap: table_node.scrollbar_gap,
                scrollbar_thumb: table_node.scrollbar_thumb,
                scrollbar_thumb_style,
                scrollbar_thumb_focus_style,
                scrollbar_track_style,
                show_scroll_indicators: table_node.show_scroll_indicators,
                scroll_indicator_style: with_theme_muted(theme, table_node.scroll_indicator_style),
                top_indicator: table_node.top_indicator,
                bottom_indicator: table_node.bottom_indicator,
                bottom_count: table_node.bottom_count,
                parent_integrated_v,
                metrics_cache: Some(scrollbar_cache),
            },
            inspector: TableInspectorCtx {
                inspector: table_node.inspector,
                inspector_key_style: with_theme_muted(theme, table_node.inspector_key_style),
                inspector_value_style: with_theme_primary(theme, table_node.inspector_value_style),
                inspector_section_style: with_theme_accent(
                    theme,
                    table_node.inspector_section_style,
                ),
                inspector_separator_style: with_theme_muted(
                    theme,
                    table_node.inspector_separator_style,
                ),
                inspector_indent_size: table_node.inspector_indent_size,
                inspector_collapsed_symbol: table_node.inspector_collapsed_symbol.as_ref(),
                inspector_expanded_symbol: table_node.inspector_expanded_symbol.as_ref(),
                inspector_separator_char: table_node.inspector_separator_char,
            },
        },
        TableRenderCtx {
            is_focused,
            is_hovered,
            mouse_pos: pointer_item_hover_mouse,
            disabled: table_node.disabled,
            rect,
            rrect,
            clip_rect: clip_bounds,
            contrast_policy,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, ColorTransform};

    #[test]
    fn selected_table_state_bg_beats_hover_bg() {
        let base = Style::new().bg(Color::Black);
        let hover = Style::new().bg(Color::Blue);
        let selection = Style::new().bg(Color::Green);
        let disabled = Style::new().bg(Color::Red);

        let resolved = resolve_table_state_style(
            base,
            TableStateStyleCtx {
                hover_style: &hover,
                selection_style: &selection,
                disabled_style: &disabled,
                hovered: true,
                selected: true,
                disabled: false,
            },
        );

        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Green)));
    }

    #[test]
    fn hovered_table_state_transform_applies_over_selected_bg() {
        let base = Style::new().bg(Color::Black);
        let hover = Style::new().transform_bg(ColorTransform::Dim(0.5));
        let selection = Style::new().bg(Color::rgb(200, 180, 160));
        let disabled = Style::new().bg(Color::Red);

        let resolved = resolve_table_state_style(
            base,
            TableStateStyleCtx {
                hover_style: &hover,
                selection_style: &selection,
                disabled_style: &disabled,
                hovered: true,
                selected: true,
                disabled: false,
            },
        )
        .resolve_color_transforms();

        assert_eq!(
            resolved.bg,
            Some(crate::style::Paint::Solid(Color::rgb(100, 90, 80)))
        );
    }

    #[test]
    fn disabled_table_state_remains_terminal() {
        let base = Style::new().bg(Color::Black);
        let hover = Style::new()
            .bg(Color::Blue)
            .transform_bg(ColorTransform::Dim(0.5));
        let selection = Style::new().bg(Color::Green);
        let disabled = Style::new().bg(Color::Red);

        let resolved = resolve_table_state_style(
            base,
            TableStateStyleCtx {
                hover_style: &hover,
                selection_style: &selection,
                disabled_style: &disabled,
                hovered: true,
                selected: true,
                disabled: true,
            },
        )
        .resolve_color_transforms();

        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Red)));
    }
}
