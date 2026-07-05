use ratatui::text::{Line, Span, Text as RText};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    calculate_visible_borders, finalize_style, style_backdrop, style_paints_bg,
    to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect, to_ratatui_style,
    truncate_end_with_ellipsis,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::backend::ratatui_backend::renderers::theme::{with_theme_muted, with_theme_primary};
use crate::core::node::NodeId;
use crate::style::resolve::{Durability, StateLayer, resolve_state_cascade};
use crate::style::{BorderStyle, Padding, Style, ThemeRole, resolve_slot};
use crate::widgets::{Tab, TabsOverflow, tab_width_budgets};

fn resolve_tabs_base_style(
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

struct TabsTabStyleCtx {
    tab_style: Style,
    tab_hover_style: Style,
    active_style: Style,
    disabled_style: Style,
    is_hovered: bool,
    is_active: bool,
    disabled: bool,
}

fn resolve_tabs_tab_style(base: Style, ctx: TabsTabStyleCtx) -> Style {
    let TabsTabStyleCtx {
        tab_style,
        tab_hover_style,
        active_style,
        disabled_style,
        is_hovered,
        is_active,
        disabled,
    } = ctx;
    let base = base.patch(tab_style);
    let mut layers = Vec::with_capacity(3);
    if is_hovered && !disabled {
        layers.push(StateLayer {
            style: &tab_hover_style,
            durability: Durability::Transient,
        });
    }
    if is_active {
        layers.push(StateLayer {
            style: &active_style,
            durability: Durability::Durable,
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

pub(crate) struct TabsRenderCtx {
    pub style: Style,
    pub focus_style: Style,
    pub hover_style: Style,
    pub tab_hover_style: Style,
    pub active_style: Style,
    pub divider: char,
    pub overflow: TabsOverflow,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub disabled: bool,
    pub disabled_style: Style,
    pub contrast_policy: ContrastPolicy,
    pub clip_rect: Option<crate::style::Rect>,
}

pub(crate) fn render_tabs(
    f: &mut ratatui::Frame<'_>,
    tabs: &[Tab],
    active: usize,
    rect: crate::style::Rect,
    rrect: ratatui::layout::Rect,
    ctx: TabsRenderCtx,
) {
    let TabsRenderCtx {
        style,
        focus_style,
        hover_style,
        tab_hover_style,
        active_style,
        divider,
        overflow,
        border,
        border_style,
        padding,
        is_focused,
        is_hovered,
        mouse_pos,
        disabled,
        disabled_style,
        contrast_policy,
        clip_rect,
    } = ctx;
    let base_raw_style = resolve_tabs_base_style(
        style,
        focus_style,
        hover_style,
        disabled_style,
        is_focused,
        is_hovered,
        disabled,
    );
    let base_style = finalize_style(base_raw_style, None, contrast_policy);
    let base_backdrop = style_backdrop(base_style);

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

    // Calculate scroll offset to account for clipping
    let dx = (inner_rrect.x as i32).saturating_sub(inner.x as i32).max(0) as u16;
    let dy = (inner_rrect.y as i32).saturating_sub(inner.y as i32).max(0) as u16;

    let len = tabs.len();
    if len == 0 {
        return;
    }

    let active = active.min(len.saturating_sub(1));

    let hovered_x = if !disabled && is_hovered {
        mouse_pos.and_then(|(mx, my)| {
            if (my as i16) == inner.y
                && (mx as i16) >= inner.x
                && (mx as i16) < inner.x.saturating_add(inner.w as i16)
            {
                Some((mx as i32).saturating_sub(inner.x as i32).max(0) as u16)
            } else {
                None
            }
        })
    } else {
        None
    };

    let mut spans = Vec::new();
    let mut used = 0usize;
    let max_w = inner.w as usize;
    let budgets = tab_width_budgets(tabs, divider, max_w, overflow);

    for (idx, tab) in tabs.iter().enumerate() {
        if used >= max_w {
            break;
        }

        let seg = format!(" {} ", tab.label.as_ref());
        let remaining = budgets.as_ref().map_or_else(
            || max_w.saturating_sub(used).min(u16::MAX as usize) as u16,
            |budgets| budgets.get(idx).copied().unwrap_or(0),
        );
        let seg = truncate_end_with_ellipsis(&seg, remaining).into_owned();
        let seg_w = UnicodeWidthStr::width(seg.as_str()) as u16;
        let start_x = used.min(u16::MAX as usize) as u16;

        let is_tab_hovered =
            hovered_x.is_some_and(|hx| hx >= start_x && hx < start_x.saturating_add(seg_w));
        let tab_style = resolve_tabs_tab_style(
            base_raw_style,
            TabsTabStyleCtx {
                tab_style: tab.style,
                tab_hover_style,
                active_style,
                disabled_style,
                is_hovered: is_tab_hovered,
                is_active: idx == active,
                disabled,
            },
        );
        let tab_style = finalize_style(tab_style, base_backdrop, contrast_policy);

        let rs = to_ratatui_style(tab_style);
        used = used.saturating_add(seg_w as usize);
        spans.push(Span::styled(seg, rs));

        if used >= max_w {
            break;
        }

        if idx + 1 < len {
            let div = divider.to_string();
            let remaining = max_w.saturating_sub(used).min(u16::MAX as usize) as u16;
            let div = truncate_end_with_ellipsis(&div, remaining).into_owned();
            used = used.saturating_add(UnicodeWidthStr::width(div.as_str()));
            spans.push(Span::styled(div, to_ratatui_style(base_style)));
        }
    }

    let line = Line::from(spans);
    let p = Paragraph::new(RText::from(line)).scroll((dy, dx));
    f.render_widget(p, inner_rrect);
}

pub(crate) fn render_tabs_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::TabsNode,
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
    render_tabs(
        state.f,
        &node.tabs,
        node.active,
        rect,
        rrect,
        TabsRenderCtx {
            style: with_theme_primary(theme, node.style),
            focus_style,
            hover_style,
            tab_hover_style,
            active_style,
            divider: node.divider,
            overflow: node.overflow,
            border: node.border,
            border_style: node.border_style,
            padding: node.padding,
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

    use super::{TabsTabStyleCtx, resolve_tabs_tab_style};

    #[test]
    fn active_hover_tab_keeps_active_colors_and_applies_hover_transform() {
        let base = Style::new().bg(Color::rgb(10, 10, 10));
        let tab = Style::new();
        let hover = Style::new()
            .bg(Color::Blue)
            .transform_bg(ColorTransform::Dim(0.5));
        let active = Style::new().bg(Color::rgb(200, 180, 160));

        let resolved = resolve_tabs_tab_style(
            base,
            TabsTabStyleCtx {
                tab_style: tab,
                tab_hover_style: hover,
                active_style: active,
                disabled_style: Style::new(),
                is_hovered: true,
                is_active: true,
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
    fn disabled_tab_state_remains_terminal() {
        let base = Style::new().bg(Color::Black);
        let hover = Style::new().transform_bg(ColorTransform::Dim(0.5));
        let active = Style::new().bg(Color::Green);
        let disabled = Style::new().bg(Color::Gray);

        let resolved = resolve_tabs_tab_style(
            base,
            TabsTabStyleCtx {
                tab_style: Style::new(),
                tab_hover_style: hover,
                active_style: active,
                disabled_style: disabled,
                is_hovered: true,
                is_active: true,
                disabled: true,
            },
        )
        .resolve_color_transforms();

        assert_eq!(resolved.bg, Some(crate::style::Paint::Solid(Color::Gray)));
    }
}
