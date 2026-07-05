use std::sync::Arc;

use ratatui::widgets::Block;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, border_horizontal_char, border_tabs_title_line,
    calculate_visible_borders, clear_fg_preserve_bg_clipped, fill_rect_clipped_style,
    style_paints_bg, style_uses_backdrop_bg, to_ratatui_border_set, to_ratatui_border_type,
    to_ratatui_style_with_terminal_bg,
};
use crate::style::{Color, Rect, RichText, Style};
use crate::widgets::GridProps;
use crate::widgets::TabVariant;
use crate::widgets::internal::StackProps;

pub(crate) struct VStackRenderCtx<'a> {
    pub active_tab_style: &'a Style,
    pub inactive_tab_style: &'a Style,
    pub tab_variant: &'a TabVariant,
    pub title_prefix: &'a Option<Arc<str>>,
    pub clip_rect: Option<Rect>,
    pub terminal_bg: Option<Color>,
}

pub(crate) fn render_vstack(
    f: &mut ratatui::Frame<'_>,
    props: &StackProps,
    tab_titles: &[RichText],
    active_tab: usize,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: VStackRenderCtx<'_>,
) {
    let VStackRenderCtx {
        active_tab_style,
        inactive_tab_style,
        tab_variant,
        title_prefix,
        clip_rect,
        terminal_bg,
    } = ctx;
    let needs_bg = style_paints_bg(props.style);
    if needs_bg || props.border {
        if style_uses_backdrop_bg(props.style) {
            clear_fg_preserve_bg_clipped(f, rect, clip_rect);
        } else if needs_bg {
            fill_rect_clipped_style(f, rect, props.style, clip_rect, terminal_bg);
        }

        if props.border {
            let borders = calculate_visible_borders(rect, clip_rect);
            let mut block = Block::default()
                .borders(borders)
                .border_type(to_ratatui_border_type(props.border_style))
                .style(to_ratatui_style_with_terminal_bg(props.style, terminal_bg));

            if let Some(set) = to_ratatui_border_set(props.border_style) {
                block = block.border_set(set);
            }

            if !tab_titles.is_empty() {
                let active_tab_style = props.style.patch(*active_tab_style);
                let inactive_tab_style = props.style.patch(*inactive_tab_style);
                let mut line = border_tabs_title_line(
                    tab_titles,
                    active_tab,
                    active_tab_style,
                    inactive_tab_style,
                    *tab_variant,
                    props.style,
                    props.style,
                );

                if let Some(prefix) = title_prefix {
                    use ratatui::text::Span;
                    let prefix_style =
                        to_ratatui_style_with_terminal_bg(active_tab_style, terminal_bg);
                    let prefix_span = Span::styled(prefix.to_string(), prefix_style);

                    let h_char = border_horizontal_char(props.border_style);
                    let sep_span = Span::styled(
                        h_char.to_string(),
                        to_ratatui_style_with_terminal_bg(props.style, terminal_bg),
                    );

                    line.spans.insert(0, sep_span);
                    line.spans.insert(0, prefix_span);
                }

                block = block.title(line);
            }
            f.render_widget(block, rrect);
        }
    }
}

pub(crate) struct GridRenderCtx {
    pub clip_rect: Option<Rect>,
    pub terminal_bg: Option<Color>,
}

pub(crate) fn render_grid(
    f: &mut ratatui::Frame<'_>,
    props: &GridProps,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: GridRenderCtx,
) {
    let GridRenderCtx {
        clip_rect,
        terminal_bg,
    } = ctx;
    let needs_bg = style_paints_bg(props.style);
    if needs_bg || props.border {
        if style_uses_backdrop_bg(props.style) {
            clear_fg_preserve_bg_clipped(f, rect, clip_rect);
        } else if needs_bg {
            fill_rect_clipped_style(f, rect, props.style, clip_rect, terminal_bg);
        }

        if props.border {
            let borders = calculate_visible_borders(rect, clip_rect);
            let mut block = Block::default()
                .borders(borders)
                .border_type(to_ratatui_border_type(props.border_style))
                .style(to_ratatui_style_with_terminal_bg(props.style, terminal_bg));

            if let Some(set) = to_ratatui_border_set(props.border_style) {
                block = block.border_set(set);
            }
            f.render_widget(block, rrect);
        }
    }
}

pub(crate) fn render_hstack(
    f: &mut ratatui::Frame<'_>,
    props: &StackProps,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_rect: Option<Rect>,
    terminal_bg: Option<Color>,
) {
    let needs_bg = style_paints_bg(props.style);
    if needs_bg || props.border {
        if style_uses_backdrop_bg(props.style) {
            clear_fg_preserve_bg_clipped(f, rect, clip_rect);
        } else if needs_bg {
            fill_rect_clipped_style(f, rect, props.style, clip_rect, terminal_bg);
        }

        if props.border {
            let borders = calculate_visible_borders(rect, clip_rect);
            let mut block = Block::default()
                .borders(borders)
                .border_type(to_ratatui_border_type(props.border_style))
                .style(to_ratatui_style_with_terminal_bg(props.style, terminal_bg));

            if let Some(set) = to_ratatui_border_set(props.border_style) {
                block = block.border_set(set);
            }
            f.render_widget(block, rrect);
        }
    }
}

pub(crate) fn render_zstack_center(
    f: &mut ratatui::Frame<'_>,
    style: &Style,
    rect: Rect,
    _rrect: ratatui::layout::Rect,
    clip_rect: Option<Rect>,
    terminal_bg: Option<ratatui::style::Color>,
) {
    if style_uses_backdrop_bg(*style) {
        clear_fg_preserve_bg_clipped(f, rect, clip_rect);
    } else if style_paints_bg(*style) {
        fill_rect_clipped_style(
            f,
            rect,
            *style,
            clip_rect,
            terminal_bg.map(crate::backend::ratatui_backend::common::from_ratatui_color),
        );
    }

    apply_effect_style_clipped(f, rect, *style, clip_rect, terminal_bg);
}
