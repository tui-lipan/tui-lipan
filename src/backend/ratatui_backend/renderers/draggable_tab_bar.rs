use ratatui::text::{Line, Span, Text as RText};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    calculate_visible_borders, finalize_style, style_backdrop, style_paints_bg,
    to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect, to_ratatui_style,
    truncate_end_with_ellipsis,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::backend::ratatui_backend::renderers::theme::{
    with_theme_accent, with_theme_muted, with_theme_primary, with_theme_role,
};
use crate::core::node::NodeId;
use crate::style::resolve::{Durability, StateLayer, resolve_state_cascade};
use crate::style::{BorderStyle, FileIconPalette, Padding, Style, ThemeRole, resolve_slot};
use crate::utils::file_icons::FileIconOverride;
use crate::widgets::draggable_tab_bar::{DraggableTabHitTarget, OverflowControlSide};
use crate::widgets::{
    DraggableTab, DraggableTabBar, DraggableTabBarOverflow, DraggableTabBarVariant,
    DraggableTabHitPart, DraggableTabKind, FileIconStyle,
};
use std::collections::HashMap;

fn resolve_draggable_base_style(
    style: Style,
    focus_style: Style,
    hover_style: Style,
    disabled_style: Style,
    is_focused: bool,
    is_hovered: bool,
    disabled: bool,
) -> Style {
    if disabled {
        return resolve_state_cascade(
            style,
            &[StateLayer {
                style: &disabled_style,
                durability: Durability::Durable,
            }],
        );
    }

    let mut layers = Vec::with_capacity(2);
    if is_hovered {
        layers.push(StateLayer {
            style: &hover_style,
            durability: Durability::Transient,
        });
    }
    if is_focused {
        layers.push(StateLayer {
            style: &focus_style,
            durability: Durability::Durable,
        });
    }
    resolve_state_cascade(style, &layers)
}

struct DraggableTabStyleCtx {
    tab_style: Style,
    bar_tab_hover_style: Style,
    tab_hover_style: Style,
    bar_active_style: Style,
    tab_active_style: Style,
    disabled_style: Style,
    is_hovered: bool,
    is_active: bool,
    disabled: bool,
}

fn resolve_draggable_tab_style(base: Style, ctx: DraggableTabStyleCtx) -> Style {
    let DraggableTabStyleCtx {
        tab_style,
        bar_tab_hover_style,
        tab_hover_style,
        bar_active_style,
        tab_active_style,
        disabled_style,
        is_hovered,
        is_active,
        disabled,
    } = ctx;
    let base = base.patch(tab_style);
    let mut layers = Vec::with_capacity(3);
    if is_active {
        layers.push(StateLayer {
            style: &bar_active_style,
            durability: Durability::Durable,
        });
        layers.push(StateLayer {
            style: &tab_active_style,
            durability: Durability::Durable,
        });
    } else if is_hovered && !disabled {
        layers.push(StateLayer {
            style: &bar_tab_hover_style,
            durability: Durability::Transient,
        });
        layers.push(StateLayer {
            style: &tab_hover_style,
            durability: Durability::Transient,
        });
    }
    if disabled {
        layers.push(StateLayer {
            style: &disabled_style,
            durability: Durability::Durable,
        });
    }
    resolve_state_cascade(base, &layers)
}

fn resolve_hover_control_style(
    base: Style,
    control_style: Style,
    hover_style: Style,
    disabled_style: Style,
    is_hovered: bool,
    disabled: bool,
) -> Style {
    let base = base.patch(control_style);
    let mut layers = Vec::with_capacity(2);
    if is_hovered && !disabled {
        layers.push(StateLayer {
            style: &hover_style,
            durability: Durability::Transient,
        });
    }
    if disabled {
        layers.push(StateLayer {
            style: &disabled_style,
            durability: Durability::Durable,
        });
    }
    resolve_state_cascade(base, &layers)
}

fn resolve_draggable_accent_style(
    tab_style: Style,
    bar_accent_style: Style,
    tab_accent_style: Style,
    disabled_style: Style,
    disabled: bool,
) -> Style {
    let mut accent = bar_accent_style.patch(tab_accent_style);
    if disabled {
        accent = accent.patch(disabled_style);
    }
    tab_style.patch(accent)
}

pub(crate) struct DraggableTabBarRenderCtx<'a> {
    pub style: Style,
    pub focus_style: Style,
    pub hover_style: Style,
    pub tab_hover_style: Style,
    pub active_style: Style,
    pub close_style: Style,
    pub close_hover_style: Style,
    pub divider: char,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub variant: DraggableTabBarVariant,
    pub accent_symbol: char,
    pub active_accent_symbol: char,
    pub accent_style: Style,
    pub active_accent_style: Style,
    pub close_symbol: &'a str,
    pub show_close_buttons: bool,
    pub close_on_hover_only: bool,
    pub tab_max_width: Option<u16>,
    pub overflow: DraggableTabBarOverflow,
    pub scroll_offset: usize,
    pub show_overflow_controls: bool,
    pub overflow_style: Style,
    pub overflow_hover_style: Style,
    pub show_file_icons: bool,
    pub file_icon_style: FileIconStyle,
    pub file_icon_palette: &'a FileIconPalette,
    pub file_icon_overrides: &'a HashMap<std::sync::Arc<str>, FileIconOverride>,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub disabled: bool,
    pub disabled_style: Style,
    pub contrast_policy: ContrastPolicy,
    pub clip_rect: Option<crate::style::Rect>,
}

pub(crate) fn render_draggable_tab_bar(
    f: &mut ratatui::Frame<'_>,
    tabs: &[DraggableTab],
    active: usize,
    rect: crate::style::Rect,
    rrect: ratatui::layout::Rect,
    ctx: DraggableTabBarRenderCtx<'_>,
) {
    let DraggableTabBarRenderCtx {
        style,
        focus_style,
        hover_style,
        tab_hover_style,
        active_style,
        close_style,
        close_hover_style,
        divider,
        border,
        border_style,
        padding,
        variant,
        accent_symbol,
        active_accent_symbol,
        accent_style,
        active_accent_style,
        close_symbol,
        show_close_buttons,
        close_on_hover_only,
        tab_max_width,
        overflow,
        scroll_offset,
        show_overflow_controls,
        overflow_style,
        overflow_hover_style,
        show_file_icons,
        file_icon_style,
        file_icon_palette,
        file_icon_overrides,
        is_focused,
        is_hovered,
        mouse_pos,
        disabled,
        disabled_style,
        contrast_policy,
        clip_rect,
    } = ctx;
    let base_raw_style = resolve_draggable_base_style(
        style,
        focus_style,
        hover_style,
        disabled_style,
        is_focused,
        is_hovered,
        disabled,
    );
    let base_style = finalize_style(base_raw_style, None, contrast_policy);

    let mut inner = rect;

    if style_paints_bg(base_style) {
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

        f.render_widget(block, rrect);

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
    } else if style_paints_bg(base_style) {
        let bg = Block::default().style(to_ratatui_style(base_style));
        f.render_widget(bg, rrect);
    }

    inner = inner.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let inner_rrect_unclipped = to_ratatui_rect(inner);
    let inner_rrect = if let Some(clip) = clip_rect {
        let clip = to_ratatui_rect(clip);
        inner_rrect_unclipped.intersection(clip)
    } else {
        inner_rrect_unclipped
    };

    if inner_rrect.width == 0 || inner_rrect.height == 0 {
        return;
    }

    let dy = (inner_rrect.y as i32).saturating_sub(inner.y as i32).max(0) as u16;

    let len = tabs.len();
    if len == 0 {
        return;
    }
    let active = active.min(len.saturating_sub(1));

    let disp_opts = crate::widgets::draggable_tab_bar::TabDisplayOptions {
        variant,
        divider,
        accent_symbol,
        close_symbol,
        show_close_buttons,
        tab_max_width,
        overflow,
        show_file_icons,
        file_icon_style,
        file_icon_palette,
        file_icon_overrides,
    };
    let vp_opts = crate::widgets::draggable_tab_bar::TabViewportOptions {
        scroll_offset,
        viewport_width: inner.w as usize,
        show_overflow_controls,
    };
    let layout = DraggableTabBar::viewport_layout(tabs, &disp_opts, &vp_opts);

    let hovered_target = if !disabled && is_hovered {
        mouse_pos.and_then(|(mx, my)| {
            if (my as i16) == inner.y
                && (mx as i16) >= inner.x
                && (mx as i16) < inner.x.saturating_add(inner.w as i16)
            {
                let col = (mx as i32).saturating_sub(inner.x as i32).max(0) as usize;
                DraggableTabBar::hit_target_at_view_col(
                    tabs,
                    &disp_opts,
                    &crate::widgets::draggable_tab_bar::TabViewportOptions {
                        scroll_offset: layout.offset,
                        viewport_width: inner.w as usize,
                        show_overflow_controls,
                    },
                    col,
                )
            } else {
                None
            }
        })
    } else {
        None
    };

    let hovered_tab = match hovered_target {
        Some(DraggableTabHitTarget::Tab(hit)) => Some(hit.index),
        _ => None,
    };
    let hovered_close_tab = match hovered_target {
        Some(DraggableTabHitTarget::Tab(hit)) if hit.part == DraggableTabHitPart::Close => {
            Some(hit.index)
        }
        _ => None,
    };
    let hovered_control = match hovered_target {
        Some(DraggableTabHitTarget::Overflow(side)) => Some(side),
        _ => None,
    };

    let segment_rect = |start: usize, width: usize| {
        let start = start.min(inner.w as usize);
        let width = width.min((inner.w as usize).saturating_sub(start));
        if width == 0 {
            return ratatui::layout::Rect {
                x: inner_rrect_unclipped.x,
                y: inner_rrect_unclipped.y,
                width: 0,
                height: 0,
            };
        }

        let mut rect = inner_rrect_unclipped;
        rect.x = rect.x.saturating_add(start as u16);
        rect.width = width as u16;
        rect.intersection(inner_rrect)
    };

    let mut tab_spans = Vec::new();
    let mut content_scroll = 0u16;
    if let Some(first) = layout.visible_tabs.first() {
        let first_view_start = first.start.saturating_sub(layout.content_start);
        let first_full_start = layout
            .offset
            .saturating_add(first_view_start)
            .saturating_sub(first.clip_left);
        let line_start = first_full_start.min(layout.offset);
        let mut cursor = line_start;

        for vis in &layout.visible_tabs {
            let view_start = vis.start.saturating_sub(layout.content_start);
            let full_start = layout
                .offset
                .saturating_add(view_start)
                .saturating_sub(vis.clip_left);

            if full_start > cursor {
                let gap = full_start.saturating_sub(cursor);
                if matches!(variant, DraggableTabBarVariant::Bordered) {
                    tab_spans.push(Span::styled(
                        divider.to_string(),
                        to_ratatui_style(base_style),
                    ));
                    if gap > 1 {
                        tab_spans.push(Span::styled(
                            " ".repeat(gap.saturating_sub(1)),
                            to_ratatui_style(base_style),
                        ));
                    }
                } else {
                    tab_spans.push(Span::styled(" ".repeat(gap), to_ratatui_style(base_style)));
                }
            }

            let tab = &tabs[vis.index];
            let is_action_tab = tab.kind == DraggableTabKind::Action;
            let is_tab_hovered = hovered_tab == Some(vis.index);
            let is_close_hovered = hovered_close_tab == Some(vis.index);

            let tab_style = resolve_draggable_tab_style(
                base_raw_style,
                DraggableTabStyleCtx {
                    tab_style: tab.style,
                    bar_tab_hover_style: tab_hover_style,
                    tab_hover_style: tab.hover_style,
                    bar_active_style: active_style,
                    tab_active_style: tab.active_style,
                    disabled_style,
                    is_hovered: is_tab_hovered,
                    is_active: !is_action_tab && vis.index == active,
                    disabled,
                },
            );
            let tab_style = finalize_style(tab_style, style_backdrop(base_style), contrast_policy);

            let icon = crate::widgets::draggable_tab_bar::resolve_tab_icon(
                tab,
                show_file_icons,
                file_icon_style,
                file_icon_palette,
                file_icon_overrides,
            );

            match variant {
                DraggableTabBarVariant::Bordered => {
                    tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));

                    if let Some(icon) = &icon {
                        let icon_style = finalize_style(
                            tab_style.patch(icon.style),
                            style_backdrop(tab_style),
                            contrast_policy,
                        );
                        tab_spans.push(Span::styled(
                            icon.content.as_ref().to_string(),
                            to_ratatui_style(icon_style),
                        ));
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                    }

                    let label = truncate_end_with_ellipsis(
                        tab.label.as_ref(),
                        vis.metrics.label_width.min(u16::MAX as usize) as u16,
                    )
                    .into_owned();
                    tab_spans.push(Span::styled(label, to_ratatui_style(tab_style)));

                    if let Some(badge) = &tab.right_badge {
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                        let badge_style = finalize_style(
                            tab_style.patch(badge.style),
                            style_backdrop(tab_style),
                            contrast_policy,
                        );
                        tab_spans.push(Span::styled(
                            badge.content.as_ref().to_string(),
                            to_ratatui_style(badge_style),
                        ));
                    }

                    if show_close_buttons && tab.closeable && !is_action_tab {
                        let show_close_symbol = !close_on_hover_only || is_tab_hovered;
                        let close = resolve_hover_control_style(
                            tab_style,
                            close_style,
                            close_hover_style,
                            disabled_style,
                            is_close_hovered,
                            disabled,
                        );
                        let close =
                            finalize_style(close, style_backdrop(tab_style), contrast_policy);
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                        tab_spans.push(Span::styled(
                            if show_close_symbol {
                                close_symbol.to_string()
                            } else {
                                " ".to_string()
                            },
                            to_ratatui_style(close),
                        ));
                    }

                    tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                }
                DraggableTabBarVariant::FrameLine => {
                    let accent = if !is_action_tab && vis.index == active {
                        resolve_draggable_accent_style(
                            tab_style,
                            active_accent_style,
                            tab.active_accent_style,
                            disabled_style,
                            disabled,
                        )
                    } else {
                        resolve_draggable_accent_style(
                            tab_style,
                            accent_style,
                            tab.accent_style,
                            disabled_style,
                            disabled,
                        )
                    };
                    let accent = finalize_style(accent, style_backdrop(tab_style), contrast_policy);
                    tab_spans.push(Span::styled(
                        if !is_action_tab && vis.index == active {
                            active_accent_symbol.to_string()
                        } else {
                            accent_symbol.to_string()
                        },
                        to_ratatui_style(accent),
                    ));
                    tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));

                    if let Some(icon) = &icon {
                        let icon_style = finalize_style(
                            tab_style.patch(icon.style),
                            style_backdrop(tab_style),
                            contrast_policy,
                        );
                        tab_spans.push(Span::styled(
                            icon.content.as_ref().to_string(),
                            to_ratatui_style(icon_style),
                        ));
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                    }

                    let label = truncate_end_with_ellipsis(
                        tab.label.as_ref(),
                        vis.metrics.label_width.min(u16::MAX as usize) as u16,
                    )
                    .into_owned();
                    tab_spans.push(Span::styled(label, to_ratatui_style(tab_style)));

                    if let Some(badge) = &tab.right_badge {
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                        let badge_style = finalize_style(
                            tab_style.patch(badge.style),
                            style_backdrop(tab_style),
                            contrast_policy,
                        );
                        tab_spans.push(Span::styled(
                            badge.content.as_ref().to_string(),
                            to_ratatui_style(badge_style),
                        ));
                    }

                    if show_close_buttons && tab.closeable && !is_action_tab {
                        let show_close_symbol = !close_on_hover_only || is_tab_hovered;
                        let close = resolve_hover_control_style(
                            tab_style,
                            close_style,
                            close_hover_style,
                            disabled_style,
                            is_close_hovered,
                            disabled,
                        );
                        let close =
                            finalize_style(close, style_backdrop(tab_style), contrast_policy);
                        tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                        tab_spans.push(Span::styled(
                            if show_close_symbol {
                                close_symbol.to_string()
                            } else {
                                " ".to_string()
                            },
                            to_ratatui_style(close),
                        ));
                    }

                    tab_spans.push(Span::styled(" ", to_ratatui_style(tab_style)));
                }
            }

            cursor = full_start.saturating_add(vis.metrics.width);
        }

        content_scroll = layout
            .offset
            .saturating_sub(line_start)
            .min(u16::MAX as usize) as u16;
    }

    let content_rect = segment_rect(layout.content_start, layout.content_width);
    if content_rect.width > 0 && content_rect.height > 0 {
        let p = Paragraph::new(RText::from(Line::from(tab_spans))).scroll((dy, content_scroll));
        f.render_widget(p, content_rect);
    }

    if let Some(left) = &layout.left_control {
        let s = resolve_hover_control_style(
            base_raw_style,
            overflow_style,
            overflow_hover_style,
            disabled_style,
            hovered_control == Some(OverflowControlSide::Left),
            disabled,
        );
        let s = finalize_style(s, style_backdrop(base_style), contrast_policy);
        let rect = segment_rect(left.start, left.end.saturating_sub(left.start));
        if rect.width > 0 && rect.height > 0 {
            let p = Paragraph::new(RText::from(Line::from(vec![Span::styled(
                left.label.as_ref().to_string(),
                to_ratatui_style(s),
            )])))
            .scroll((dy, 0));
            f.render_widget(p, rect);
        }
    }

    if let Some(right) = &layout.right_control {
        let s = resolve_hover_control_style(
            base_raw_style,
            overflow_style,
            overflow_hover_style,
            disabled_style,
            hovered_control == Some(OverflowControlSide::Right),
            disabled,
        );
        let s = finalize_style(s, style_backdrop(base_style), contrast_policy);
        let rect = segment_rect(right.start, right.end.saturating_sub(right.start));
        if rect.width > 0 && rect.height > 0 {
            let p = Paragraph::new(RText::from(Line::from(vec![Span::styled(
                right.label.as_ref().to_string(),
                to_ratatui_style(s),
            )])))
            .scroll((dy, 0));
            f.render_widget(p, rect);
        }
    }
}

pub(crate) fn render_draggable_tab_bar_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::DraggableTabBarNode,
    rect: crate::style::Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<crate::style::Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !node.disabled;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = state.ctx.tree.node(node_id).active_theme();
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    let tab_hover_style = resolve_slot(theme, ThemeRole::ItemHover, &node.tab_hover_style);
    let active_style = resolve_slot(theme, ThemeRole::Selection, &node.active_style);
    let mut tabs = node.tabs.to_vec();
    for tab in &mut tabs {
        if let Some(spinner) = tab
            .leading
            .as_mut()
            .and_then(|leading| leading.spinner_mut())
        {
            spinner.spinner.style = with_theme_accent(theme, spinner.spinner.style);
        }
    }
    let file_icon_palette = if node.file_icon_palette == FileIconPalette::default() {
        &theme.file_icons
    } else {
        &node.file_icon_palette
    };
    render_draggable_tab_bar(
        state.f,
        &tabs,
        node.active,
        rect,
        rrect,
        DraggableTabBarRenderCtx {
            style: with_theme_primary(theme, node.style),
            focus_style,
            hover_style,
            tab_hover_style,
            active_style,
            close_style: with_theme_muted(theme, node.close_style),
            close_hover_style: with_theme_role(theme, ThemeRole::Hover, node.close_hover_style),
            divider: node.divider,
            border: node.border,
            border_style: node.border_style,
            padding: node.padding,
            variant: node.variant,
            accent_symbol: node.accent_symbol,
            active_accent_symbol: node.active_accent_symbol,
            accent_style: with_theme_muted(theme, node.accent_style),
            active_accent_style: with_theme_accent(theme, node.active_accent_style),
            close_symbol: &node.close_symbol,
            show_close_buttons: node.show_close_buttons,
            close_on_hover_only: node.close_on_hover_only,
            tab_max_width: node.tab_max_width,
            overflow: node.overflow,
            scroll_offset: node.scroll_offset,
            show_overflow_controls: node.show_overflow_controls,
            overflow_style: with_theme_muted(theme, node.overflow_style),
            overflow_hover_style: with_theme_role(
                theme,
                ThemeRole::Hover,
                node.overflow_hover_style,
            ),
            show_file_icons: node.show_file_icons,
            file_icon_style: node.file_icon_style,
            file_icon_palette,
            file_icon_overrides: &node.file_icon_overrides,
            is_focused,
            is_hovered,
            mouse_pos: state.ctx.mouse_pos,
            disabled: node.disabled,
            disabled_style: with_theme_muted(theme, node.disabled_style),
            contrast_policy,
            clip_rect: clip_bounds,
        },
    );
}

#[cfg(test)]
mod tests {
    use crate::style::{Color, ColorTransform, Style};

    use super::{
        DraggableTabStyleCtx, resolve_draggable_accent_style, resolve_draggable_tab_style,
    };

    #[test]
    fn active_draggable_tab_suppresses_hover_transform() {
        let base = Style::new().bg(Color::rgb(10, 10, 10));
        let hover = Style::new()
            .bg(Color::Blue)
            .transform_bg(ColorTransform::Dim(0.5));
        let active = Style::new().bg(Color::rgb(200, 180, 160));

        let resolved = resolve_draggable_tab_style(
            base,
            DraggableTabStyleCtx {
                tab_style: Style::new(),
                bar_tab_hover_style: hover,
                tab_hover_style: Style::new(),
                bar_active_style: active,
                tab_active_style: Style::new(),
                disabled_style: Style::new(),
                is_hovered: true,
                is_active: true,
                disabled: false,
            },
        )
        .resolve_color_transforms();

        assert_eq!(
            resolved.bg,
            Some(crate::style::Paint::Solid(Color::rgb(200, 180, 160)))
        );
    }

    #[test]
    fn hover_only_draggable_tab_uses_transient_hover_effect() {
        let base = Style::new().bg(Color::rgb(10, 10, 10));
        let hover = Style::new().transform_bg(ColorTransform::Dim(0.5));

        let resolved = resolve_draggable_tab_style(
            base,
            DraggableTabStyleCtx {
                tab_style: Style::new(),
                bar_tab_hover_style: hover,
                tab_hover_style: Style::new(),
                bar_active_style: Style::new().bg(Color::Green),
                tab_active_style: Style::new(),
                disabled_style: Style::new(),
                is_hovered: true,
                is_active: false,
                disabled: false,
            },
        )
        .resolve_color_transforms();

        assert_eq!(
            resolved.bg,
            Some(crate::style::Paint::Solid(Color::rgb(5, 5, 5)))
        );
    }

    #[test]
    fn per_tab_active_style_patches_over_bar_active_style() {
        let resolved = resolve_draggable_tab_style(
            Style::new(),
            DraggableTabStyleCtx {
                tab_style: Style::new(),
                bar_tab_hover_style: Style::new(),
                tab_hover_style: Style::new(),
                bar_active_style: Style::new().fg(Color::Blue).bg(Color::Green),
                tab_active_style: Style::new().fg(Color::Red),
                disabled_style: Style::new(),
                is_hovered: false,
                is_active: true,
                disabled: false,
            },
        );

        assert_eq!(resolved.fg, Some(crate::style::Paint::Solid(Color::Red)));
        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Green)));
    }

    #[test]
    fn per_tab_hover_style_patches_over_bar_tab_hover_style() {
        let resolved = resolve_draggable_tab_style(
            Style::new(),
            DraggableTabStyleCtx {
                tab_style: Style::new(),
                bar_tab_hover_style: Style::new().fg(Color::Blue).bg(Color::Green),
                tab_hover_style: Style::new().fg(Color::Red),
                bar_active_style: Style::new(),
                tab_active_style: Style::new(),
                disabled_style: Style::new(),
                is_hovered: true,
                is_active: false,
                disabled: false,
            },
        );

        assert_eq!(resolved.fg, Some(crate::style::Paint::Solid(Color::Red)));
        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Green)));
    }

    #[test]
    fn disabled_style_overrides_per_tab_active_style() {
        let resolved = resolve_draggable_tab_style(
            Style::new(),
            DraggableTabStyleCtx {
                tab_style: Style::new(),
                bar_tab_hover_style: Style::new(),
                tab_hover_style: Style::new(),
                bar_active_style: Style::new().fg(Color::Blue),
                tab_active_style: Style::new().fg(Color::Red),
                disabled_style: Style::new().fg(Color::DarkGray),
                is_hovered: false,
                is_active: true,
                disabled: true,
            },
        );

        assert_eq!(
            resolved.fg,
            Some(crate::style::Paint::Solid(Color::DarkGray))
        );
    }

    #[test]
    fn per_tab_accent_style_patches_over_bar_accent_style() {
        let resolved = resolve_draggable_accent_style(
            Style::new().bg(Color::Black),
            Style::new().fg(Color::Blue).bold(),
            Style::new().fg(Color::Red),
            Style::new(),
            false,
        );

        assert_eq!(resolved.fg, Some(crate::style::Paint::Solid(Color::Red)));
        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Black)));
        assert_eq!(resolved.bold, Some(true));
    }

    #[test]
    fn disabled_style_overrides_per_tab_accent_style() {
        let resolved = resolve_draggable_accent_style(
            Style::new(),
            Style::new().fg(Color::Blue),
            Style::new().fg(Color::Red),
            Style::new().fg(Color::DarkGray),
            true,
        );

        assert_eq!(
            resolved.fg,
            Some(crate::style::Paint::Solid(Color::DarkGray))
        );
    }
}
