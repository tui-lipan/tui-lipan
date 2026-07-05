use std::cell::RefCell;

use ratatui::text::Text as RText;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IndicatorDirection, IntegratedScrollbarAppearance,
    ScrollbarAppearance, ScrollbarScrollState, calculate_visible_borders,
    integrated_vscrollbar_track_char, render_hscrollbar, render_integrated_scrollbar_with_metrics,
    render_integrated_vscrollbar_half_block, render_vscrollbar_half_block,
    render_vscrollbar_with_metrics, resolve_scrollbar_thumb_style, scroll_indicator_line,
    style_paints_bg, to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect,
    to_ratatui_style_with_terminal_bg,
};
use crate::backend::ratatui_backend::render::{
    FrameIntegratedVTrack, RenderState, ancestor_frame_integrated_vtrack,
};
use crate::style::resolve::{resolve_base_style, resolve_scrollbar_theme};
use crate::style::{Color, Rect, ScrollbarVariant, Style};
use crate::utils::scrollbar::ScrollbarMetricsCache;
use crate::widgets::internal::StackProps;
use crate::widgets::scroll_view_scrollbar_metrics;

pub(crate) struct ScrollViewRenderCtx<'a> {
    pub offset: usize,
    pub scroll_offset: usize,
    pub content_height: u16,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub reserve_bottom_rows: u16,
    pub is_focused: bool,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
    pub parent_integrated_v: Option<FrameIntegratedVTrack>,
    pub borders: Borders,
    pub clip_rect: Option<Rect>,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
    pub terminal_bg: Option<Color>,
}

pub(crate) fn render_scroll_view(
    f: &mut ratatui::Frame<'_>,
    props: &StackProps,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: ScrollViewRenderCtx<'_>,
) {
    let ScrollViewRenderCtx {
        offset,
        scroll_offset,
        content_height,
        scrollbar,
        scrollbar_variant,
        scrollbar_gap,
        scrollbar_thumb,
        scrollbar_thumb_style,
        scrollbar_thumb_focus_style,
        scrollbar_track_style,
        reserve_bottom_rows,
        is_focused,
        show_scroll_indicators,
        scroll_indicator_style,
        top_indicator,
        bottom_indicator,
        bottom_count,
        parent_integrated_v,
        borders,
        clip_rect,
        metrics_cache: _metrics_cache,
        terminal_bg,
    } = ctx;
    let to_ratatui_style = |style| to_ratatui_style_with_terminal_bg(style, terminal_bg);
    let needs_bg = style_paints_bg(props.style);
    let base_style = props.style;
    let clip_rrect = clip_rect.map(to_ratatui_rect);

    if needs_bg || props.border {
        if needs_bg {
            f.render_widget(Clear, rrect);
        }

        let mut block = Block::default().style(to_ratatui_style(base_style));
        if props.border {
            block = block
                .borders(borders)
                .border_type(to_ratatui_border_type(props.border_style));

            if let Some(set) = to_ratatui_border_set(props.border_style) {
                block = block.border_set(set);
            }
        }

        f.render_widget(block, rrect);
    }

    let mut inner = rect;
    if props.border {
        if borders.contains(Borders::LEFT) {
            inner.x = inner.x.saturating_add(1);
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::RIGHT) {
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::TOP) {
            inner.y = inner.y.saturating_add(1);
            inner.h = inner.h.saturating_sub(1);
        }
        if borders.contains(Borders::BOTTOM) {
            inner.h = inner.h.saturating_sub(1);
        }
    }
    inner = inner.inset(props.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    // Reserve the standalone horizontal scrollbar row so the vertical scrollbar
    // (and bottom indicator) stop above it, leaving a clean corner.
    inner.h = inner.h.saturating_sub(reserve_bottom_rows);
    if inner.h == 0 {
        return;
    }

    // Determine scrollbar mode
    let use_integrated = (props.border || parent_integrated_v.is_some())
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated);
    let use_standalone = scrollbar && !use_integrated;

    // If standalone scrollbar, reserve 1 column
    let mut content_inner = inner;
    if use_standalone && content_inner.w > 0 {
        content_inner.w = content_inner
            .w
            .saturating_sub(1u16.saturating_add(scrollbar_gap));
    }

    // Determine reserved rows for indicators (if show_scroll_indicators is on)
    if show_scroll_indicators {
        if top_indicator {
            let indicator_line = scroll_indicator_line(
                offset,
                IndicatorDirection::Top,
                base_style,
                scroll_indicator_style,
            );
            let text = RText::from(vec![indicator_line]);
            let indicator_rect = to_ratatui_rect(Rect {
                x: content_inner.x,
                y: content_inner.y,
                w: content_inner.w,
                h: 1,
            });
            let effective_indicator = indicator_rect.intersection(rrect);
            if effective_indicator.area() > 0 {
                let p = Paragraph::new(text);
                let dx = (effective_indicator.x as i32)
                    .saturating_sub(indicator_rect.x as i32)
                    .max(0) as u16;
                let dy = (effective_indicator.y as i32)
                    .saturating_sub(indicator_rect.y as i32)
                    .max(0) as u16;
                let p = p.scroll((dy, dx));
                f.render_widget(p, effective_indicator);
            }
        }

        if bottom_indicator && bottom_count > 0 {
            let indicator_line = scroll_indicator_line(
                bottom_count,
                IndicatorDirection::Bottom,
                base_style,
                scroll_indicator_style,
            );
            let text = RText::from(vec![indicator_line]);
            let indicator_rect = to_ratatui_rect(Rect {
                x: content_inner.x,
                y: content_inner
                    .y
                    .saturating_add(content_inner.h.saturating_sub(1) as i16),
                w: content_inner.w,
                h: 1,
            });
            let effective_indicator = indicator_rect.intersection(rrect);
            if effective_indicator.area() > 0 {
                let p = Paragraph::new(text);
                let dx = (effective_indicator.x as i32)
                    .saturating_sub(indicator_rect.x as i32)
                    .max(0) as u16;
                let dy = (effective_indicator.y as i32)
                    .saturating_sub(indicator_rect.y as i32)
                    .max(0) as u16;
                let p = p.scroll((dy, dx));
                f.render_widget(p, effective_indicator);
            }
        }
    }

    if scrollbar && inner.w > 0 && inner.h > 0 && content_height > 0 {
        let total = content_height as usize;
        // The thumb size and scroll range are based on the full content viewport
        // (matching reconcile's `max_offset`); only the physical track is one row
        // shorter for the reserved horizontal scrollbar row. Reducing the viewport
        // here instead would make the fully-scrolled thumb stop half a cell short.
        let viewport_h = inner.h.saturating_add(reserve_bottom_rows) as usize;
        let thumb = scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
        let use_half = thumb == DEFAULT_SCROLLBAR_THUMB;
        let Some(metrics) = scroll_view_scrollbar_metrics(
            scroll_offset,
            total,
            viewport_h,
            inner.h as usize,
            show_scroll_indicators,
            use_half,
        ) else {
            return;
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
                    .unwrap_or(props.border_style);
                let track_glyph = parent_integrated_v.and_then(|p| p.track_glyph);
                let mut track_scratch = [0u8; 4];
                let border_char =
                    integrated_vscrollbar_track_char(track_glyph, b_style, &mut track_scratch);
                let integrated_base_style = parent_integrated_v
                    .map(|p| p.track_style)
                    .unwrap_or(base_style);
                if use_half {
                    render_integrated_vscrollbar_half_block(
                        f,
                        sb_rect,
                        metrics,
                        IntegratedScrollbarAppearance {
                            thumb_char: thumb,
                            border_char,
                            base_style: integrated_base_style,
                            thumb_style,
                            track_style: scrollbar_track_style,
                            clip_rect: None,
                            metrics_cache: None,
                        },
                    );
                } else {
                    render_integrated_scrollbar_with_metrics(
                        f,
                        sb_rect,
                        metrics,
                        IntegratedScrollbarAppearance {
                            thumb_char: thumb,
                            border_char,
                            base_style: integrated_base_style,
                            thumb_style,
                            track_style: scrollbar_track_style,
                            clip_rect: None,
                            metrics_cache: None,
                        },
                    );
                }
            }
        } else {
            // Standalone
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
            if use_half {
                render_vscrollbar_half_block(
                    f,
                    sb_rect,
                    metrics,
                    thumb_style,
                    scrollbar_track_style,
                    clip_rrect,
                );
            } else {
                render_vscrollbar_with_metrics(
                    f,
                    sb_rect,
                    metrics,
                    thumb,
                    thumb_style,
                    scrollbar_track_style,
                    clip_rrect,
                );
            }
        }
    }
}

pub(crate) fn render_scroll_view_node(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    scroll_view: &crate::widgets::internal::ScrollViewNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) -> Rect {
    let is_focused = state.focus_chain.contains(&node.id);
    let parent_integrated_v = if !scroll_view.props.border
        && scroll_view.scrollbar
        && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated)
    {
        ancestor_frame_integrated_vtrack(state, node.parent)
    } else {
        None
    };

    let borders = if scroll_view.props.border {
        calculate_visible_borders(rect, clip_bounds)
    } else {
        Borders::empty()
    };

    let mut props = scroll_view.props.clone();
    props.style = resolve_base_style(node.active_theme(), props.style);
    let scroll_indicator_style = crate::style::resolve::resolve_muted_style(
        node.active_theme(),
        scroll_view.scroll_indicator_style,
    );
    let (scrollbar_thumb_style, scrollbar_thumb_focus_style, scrollbar_track_style) =
        resolve_scrollbar_theme(
            node.active_theme(),
            scroll_view.scrollbar_thumb_style,
            scroll_view.scrollbar_thumb_focus_style,
            scroll_view.scrollbar_track_style,
        );

    // A standalone horizontal scrollbar reserves the bottom content row; reserve
    // the same row for the vertical scrollbar so the two don't share a cell.
    let h_integrated = scroll_view.props.border
        && scroll_view.h_scrollbar
        && matches!(
            scroll_view.h_scrollbar_variant,
            ScrollbarVariant::Integrated
        );
    let reserve_bottom_rows =
        if scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 && !h_integrated {
            1u16.saturating_add(scroll_view.h_scrollbar_gap)
        } else {
            0
        };

    let scrollbar_cache = &state.ctx.scrollbar_metrics_cache;
    let f = &mut *state.f;
    render_scroll_view(
        f,
        &props,
        rect,
        rrect,
        ScrollViewRenderCtx {
            offset: scroll_view.offset,
            scroll_offset: scroll_view.scroll_offset as usize,
            content_height: scroll_view.content_height,
            scrollbar: scroll_view.scrollbar,
            scrollbar_variant: scroll_view.scrollbar_variant,
            scrollbar_gap: scroll_view.scrollbar_gap,
            scrollbar_thumb: scroll_view.scrollbar_thumb,
            scrollbar_thumb_style,
            scrollbar_thumb_focus_style,
            scrollbar_track_style,
            reserve_bottom_rows,
            is_focused,
            show_scroll_indicators: scroll_view.show_scroll_indicators,
            scroll_indicator_style,
            top_indicator: scroll_view.top_indicator,
            bottom_indicator: scroll_view.bottom_indicator,
            bottom_count: scroll_view.bottom_count,
            parent_integrated_v,
            borders,
            clip_rect: clip_bounds,
            metrics_cache: Some(scrollbar_cache),
            terminal_bg: state
                .ctx
                .terminal_bg
                .map(crate::backend::ratatui_backend::common::from_ratatui_color),
        },
    );

    let mut inner = rect.inner(scroll_view.props.border, scroll_view.props.padding);
    if scroll_view.show_scroll_indicators {
        if scroll_view.top_indicator {
            inner.y = inner.y.saturating_add(1);
            inner.h = inner.h.saturating_sub(1);
        }
        if scroll_view.bottom_indicator {
            inner.h = inner.h.saturating_sub(1);
        }
    }
    let use_integrated = (scroll_view.props.border || parent_integrated_v.is_some())
        && matches!(scroll_view.scrollbar_variant, ScrollbarVariant::Integrated);
    let use_standalone = scroll_view.scrollbar && !use_integrated;
    if use_standalone && inner.w > 0 {
        inner.w = inner
            .w
            .saturating_sub(1u16.saturating_add(scroll_view.scrollbar_gap));
    }

    let h_standalone = scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 && !h_integrated;
    if h_standalone && inner.h > 0 {
        inner.h = inner
            .h
            .saturating_sub(1u16.saturating_add(scroll_view.h_scrollbar_gap));
    }

    if scroll_view.h_scrollbar && scroll_view.h_max_offset > 0 {
        let (h_thumb_style, h_thumb_focus_style, h_track_style) = resolve_scrollbar_theme(
            node.active_theme(),
            scroll_view.h_scrollbar_thumb_style,
            scroll_view.h_scrollbar_thumb_focus_style,
            scroll_view.h_scrollbar_track_style,
        );
        let h_thumb = scroll_view
            .h_scrollbar_thumb
            .unwrap_or(DEFAULT_SCROLLBAR_THUMB);
        let h_thumb_style =
            resolve_scrollbar_thumb_style(is_focused, h_thumb_style, h_thumb_focus_style);
        let viewport_cols = inner.w;
        let sb_rect = if h_integrated {
            Rect {
                x: inner.x,
                y: rect.y.saturating_add(rect.h.saturating_sub(1) as i16),
                w: inner.w,
                h: 1,
            }
        } else {
            Rect {
                x: inner.x,
                y: inner.y.saturating_add(inner.h as i16),
                w: inner.w,
                h: 1,
            }
        };
        render_hscrollbar(
            f,
            sb_rect,
            ScrollbarScrollState {
                offset: scroll_view.h_offset,
                visible: viewport_cols as usize,
                total: scroll_view.content_width as usize,
            },
            ScrollbarAppearance {
                thumb_char: h_thumb,
                thumb_style: h_thumb_style,
                track_style: h_track_style,
                clip_rect: clip_bounds.map(to_ratatui_rect),
                metrics_cache: Some(scrollbar_cache),
            },
        );
    }

    clip_bounds.map(|c| inner.intersection(&c)).unwrap_or(inner)
}
