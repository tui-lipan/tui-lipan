use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::backend::ratatui_backend::common::{
    calculate_visible_borders, finalize_style, resolve_interactive_style_raw, style_backdrop,
    style_paints_bg, to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect,
    to_ratatui_style, truncate_end_with_ellipsis,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::backend::ratatui_backend::renderers::theme::{with_theme_muted, with_theme_primary};
use crate::core::node::NodeId;
use crate::style::{Align, BorderStyle, Padding, Rect, Style, ThemeRole, resolve_slot};
use crate::widgets::ButtonVariant;

pub(crate) struct ButtonRenderCtx {
    pub icon_style: Style,
    pub icon_gap: u16,
    pub shortcut_style: Style,
    pub shortcut_gap: u16,
    pub style: Style,
    pub hover_style: Style,
    pub focus_style: Style,
    pub align: Align,
    pub variant: ButtonVariant,
    pub border_style: BorderStyle,
    pub hover_border_style: Option<BorderStyle>,
    pub focus_border_style: Option<BorderStyle>,
    pub padding: Padding,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub disabled: bool,
    pub disabled_style: Style,
    pub contrast_policy: crate::app::ContrastPolicy,
    pub clip_rect: Option<Rect>,
}

pub(crate) fn render_button(
    f: &mut ratatui::Frame<'_>,
    label: &str,
    icon: Option<&str>,
    shortcut: Option<&str>,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: ButtonRenderCtx,
) {
    let ButtonRenderCtx {
        icon_style,
        icon_gap,
        shortcut_style,
        shortcut_gap,
        style,
        hover_style,
        focus_style,
        align,
        variant,
        border_style,
        hover_border_style,
        focus_border_style,
        padding,
        is_focused,
        is_hovered,
        disabled,
        disabled_style,
        contrast_policy,
        clip_rect,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let interactive_raw = resolve_interactive_style_raw(
        style,
        focus_style,
        hover_style,
        disabled_style,
        is_focused,
        is_hovered,
        disabled,
    );
    let s = finalize_style(interactive_raw, None, contrast_policy);
    let base_backdrop = style_backdrop(s);
    let mut border_type = border_style;

    if !disabled {
        if is_hovered && let Some(bt) = hover_border_style {
            border_type = bt;
        }
        if is_focused && let Some(bt) = focus_border_style {
            border_type = bt;
        }
    }

    match variant {
        ButtonVariant::Outlined => {
            let mut border_style = s;
            border_style.bg = None;

            let mut label_style = style;
            if disabled {
                label_style = label_style.patch(disabled_style);
            }
            label_style.bg = None;
            label_style = finalize_style(label_style, None, contrast_policy);

            let icon_style_final =
                finalize_style(label_style.patch(icon_style), None, contrast_policy);
            let shortcut_style_final =
                finalize_style(label_style.patch(shortcut_style), None, contrast_policy);

            let borders = calculate_visible_borders(rect, clip_rect);

            let block_style = to_ratatui_style(border_style);
            let mut block = Block::default()
                .borders(borders)
                .border_type(to_ratatui_border_type(border_type))
                .style(block_style);

            if let Some(set) = to_ratatui_border_set(border_type) {
                block = block.border_set(set);
            }

            f.render_widget(block, rrect);

            let mut inner = rect;
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
            inner = inner.inset(padding);
            if inner.w == 0 || inner.h == 0 {
                return;
            }

            let spans = build_button_spans(
                label,
                icon,
                shortcut,
                inner.w,
                ButtonSpansCtx {
                    align,
                    label_style,
                    icon_style: icon_style_final,
                    shortcut_style: shortcut_style_final,
                    icon_gap,
                    shortcut_gap,
                },
            );

            let label_y = inner
                .y
                .saturating_add((inner.h.saturating_sub(1) / 2) as i16);
            let label_rect = Rect {
                x: inner.x,
                y: label_y,
                w: inner.w,
                h: 1,
            };

            let p = Paragraph::new(Line::from(spans)).style(to_ratatui_style(label_style));
            let r_label_rect = to_ratatui_rect(label_rect);
            let effective_label = r_label_rect.intersection(rrect);
            let dx = effective_label.x.saturating_sub(r_label_rect.x);
            let dy = effective_label.y.saturating_sub(r_label_rect.y);
            let p = p.scroll((dy, dx));
            f.render_widget(p, effective_label);
        }
        ButtonVariant::Filled => {
            if style_paints_bg(s) {
                f.render_widget(Clear, rrect);
            }
            let bg = Block::default().style(to_ratatui_style(s));
            f.render_widget(bg, rrect);

            let inner = rect.inset(padding);
            if inner.w == 0 || inner.h == 0 {
                return;
            }

            let spans = build_button_spans(
                label,
                icon,
                shortcut,
                inner.w,
                ButtonSpansCtx {
                    align,
                    label_style: s,
                    icon_style: finalize_style(s.patch(icon_style), base_backdrop, contrast_policy),
                    shortcut_style: finalize_style(
                        s.patch(shortcut_style),
                        base_backdrop,
                        contrast_policy,
                    ),
                    icon_gap,
                    shortcut_gap,
                },
            );

            let label_y = inner
                .y
                .saturating_add((inner.h.saturating_sub(1) / 2) as i16);
            let label_rect = Rect {
                x: inner.x,
                y: label_y,
                w: inner.w,
                h: 1,
            };

            let p = Paragraph::new(Line::from(spans)).style(to_ratatui_style(s));
            let r_label_rect = to_ratatui_rect(label_rect);
            let effective_label = r_label_rect.intersection(rrect);
            let dx = effective_label.x.saturating_sub(r_label_rect.x);
            let dy = effective_label.y.saturating_sub(r_label_rect.y);
            let p = p.scroll((dy, dx));
            f.render_widget(p, effective_label);
        }
        ButtonVariant::Bracket => {
            if style_paints_bg(s) {
                f.render_widget(Clear, rrect);
                let bg = Block::default().style(to_ratatui_style(s));
                f.render_widget(bg, rrect);
            }

            let y = rect.y.saturating_add((rect.h.saturating_sub(1) / 2) as i16);
            let label_rect = Rect {
                x: rect.x,
                y,
                w: rect.w,
                h: 1,
            };

            let line = match rect.w {
                1 => Line::from("["),
                2 => Line::from("[]"),
                _ => {
                    let inner_w = rect.w.saturating_sub(2);

                    let pad_left = padding.left.min(inner_w);
                    let pad_right = padding.right.min(inner_w.saturating_sub(pad_left));

                    let label_space = inner_w.saturating_sub(pad_left.saturating_add(pad_right));
                    let content_spans = build_button_spans(
                        label,
                        icon,
                        shortcut,
                        label_space,
                        ButtonSpansCtx {
                            align,
                            label_style: s,
                            icon_style: finalize_style(
                                s.patch(icon_style),
                                base_backdrop,
                                contrast_policy,
                            ),
                            shortcut_style: finalize_style(
                                s.patch(shortcut_style),
                                base_backdrop,
                                contrast_policy,
                            ),
                            icon_gap,
                            shortcut_gap,
                        },
                    );

                    let left_total = pad_left as usize;
                    let right_total = pad_right as usize;

                    let mut spans = Vec::new();
                    spans.push(Span::styled("[".to_string(), to_ratatui_style(s)));
                    if left_total > 0 {
                        spans.push(Span::styled(" ".repeat(left_total), to_ratatui_style(s)));
                    }
                    spans.extend(content_spans);
                    if right_total > 0 {
                        spans.push(Span::styled(" ".repeat(right_total), to_ratatui_style(s)));
                    }
                    spans.push(Span::styled("]".to_string(), to_ratatui_style(s)));
                    Line::from(spans)
                }
            };

            let p = Paragraph::new(line).style(to_ratatui_style(s));
            let r_label_rect = to_ratatui_rect(label_rect);
            let effective_label = r_label_rect.intersection(rrect);
            let dx = effective_label.x.saturating_sub(r_label_rect.x);
            let dy = effective_label.y.saturating_sub(r_label_rect.y);
            let p = p.scroll((dy, dx));
            f.render_widget(p, effective_label);
        }
    }
}

struct ButtonSpansCtx {
    align: Align,
    label_style: Style,
    icon_style: Style,
    shortcut_style: Style,
    icon_gap: u16,
    shortcut_gap: u16,
}

fn build_button_spans(
    label: &str,
    icon: Option<&str>,
    shortcut: Option<&str>,
    width: u16,
    ctx: ButtonSpansCtx,
) -> Vec<Span<'static>> {
    let ButtonSpansCtx {
        align,
        label_style,
        icon_style,
        shortcut_style,
        icon_gap,
        shortcut_gap,
    } = ctx;
    if width == 0 {
        return Vec::new();
    }

    let mut icon = icon.filter(|s| !s.is_empty());
    let mut shortcut = shortcut.filter(|s| !s.is_empty());

    let icon_w = icon.map(UnicodeWidthStr::width).unwrap_or(0) as u16;
    let shortcut_w = shortcut.map(UnicodeWidthStr::width).unwrap_or(0) as u16;
    let mut icon_gap = if icon_w > 0 { icon_gap } else { 0 };
    let mut shortcut_gap = if shortcut_w > 0 { shortcut_gap } else { 0 };

    let mut reserved = icon_w
        .saturating_add(icon_gap)
        .saturating_add(shortcut_w)
        .saturating_add(shortcut_gap);
    if reserved > width {
        shortcut = None;
        shortcut_gap = 0;
        reserved = icon_w.saturating_add(icon_gap);
    }
    if reserved > width {
        icon = None;
        icon_gap = 0;
        reserved = 0;
    }

    let available_label = width.saturating_sub(reserved);
    let label_visible = if available_label == 0 {
        "".into()
    } else {
        truncate_end_with_ellipsis(label, available_label)
    };
    let label_w = UnicodeWidthStr::width(label_visible.as_ref()).min(u16::MAX as usize) as u16;
    let content_w = reserved.saturating_add(label_w);

    let remaining = width.saturating_sub(content_w) as usize;
    let (left_pad, right_pad) = match align {
        Align::Start | Align::Stretch => (0usize, remaining),
        Align::Center => (remaining / 2, remaining - remaining / 2),
        Align::End => (remaining, 0usize),
    };

    let mut spans = Vec::new();
    if left_pad > 0 {
        spans.push(Span::styled(
            " ".repeat(left_pad),
            to_ratatui_style(label_style),
        ));
    }

    if let Some(icon) = icon {
        spans.push(Span::styled(icon.to_string(), to_ratatui_style(icon_style)));
    }
    if icon_gap > 0 {
        spans.push(Span::styled(
            " ".repeat(icon_gap as usize),
            to_ratatui_style(label_style),
        ));
    }

    spans.push(Span::styled(
        label_visible.to_string(),
        to_ratatui_style(label_style),
    ));

    if shortcut_gap > 0 {
        spans.push(Span::styled(
            " ".repeat(shortcut_gap as usize),
            to_ratatui_style(label_style),
        ));
    }
    if let Some(shortcut) = shortcut {
        spans.push(Span::styled(
            shortcut.to_string(),
            to_ratatui_style(shortcut_style),
        ));
    }

    if right_pad > 0 {
        spans.push(Span::styled(
            " ".repeat(right_pad),
            to_ratatui_style(label_style),
        ));
    }

    spans
}

pub(crate) fn render_button_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::ButtonNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused;
    let is_hovered = Some(node_id) == state.ctx.hovered;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = state.ctx.tree.node(node_id).active_theme();
    let hover_style = resolve_slot(theme, ThemeRole::Accent, &node.hover_style);
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let style = with_theme_primary(theme, node.style);
    let icon_style = with_theme_primary(theme, node.icon_style);
    let shortcut_style = with_theme_muted(theme, node.shortcut_style);
    let disabled_style = with_theme_muted(theme, node.disabled_style);
    render_button(
        state.f,
        &node.label,
        node.icon.as_deref(),
        node.shortcut.as_deref(),
        rect,
        rrect,
        ButtonRenderCtx {
            icon_style,
            icon_gap: node.icon_gap,
            shortcut_style,
            shortcut_gap: node.shortcut_gap,
            style,
            hover_style,
            focus_style,
            align: node.align,
            variant: node.variant,
            border_style: node.border_style,
            hover_border_style: node.hover_border_style,
            focus_border_style: node.focus_border_style,
            padding: node.padding,
            is_focused,
            is_hovered,
            disabled: node.disabled,
            disabled_style,
            contrast_policy,
            clip_rect: clip_bounds,
        },
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color as RColor;

    use super::{ButtonRenderCtx, render_button};
    use crate::app::ContrastPolicy;
    use crate::backend::ratatui_backend::common::blend_paint_over_ratatui;
    use crate::style::{Align, BorderStyle, Color, Padding, Paint, Rect, Style};
    use crate::widgets::ButtonVariant;

    #[test]
    fn filled_button_hover_lighten_recomputes_contrast_from_raw_style() {
        let base = Style::new()
            .fg(Color::rgb(120, 120, 120))
            .bg(Color::rgb(20, 20, 20));
        let hover = Style::new().transform_bg(crate::style::ColorTransform::Lighten(0.9));

        let draw = |is_hovered| {
            let backend = TestBackend::new(8, 1);
            let mut terminal = Terminal::new(backend).expect("terminal");

            terminal
                .draw(|f| {
                    let rrect = ratatui::layout::Rect {
                        x: 0,
                        y: 0,
                        width: 8,
                        height: 1,
                    };
                    render_button(
                        f,
                        "X",
                        None,
                        None,
                        Rect {
                            x: 0,
                            y: 0,
                            w: 8,
                            h: 1,
                        },
                        rrect,
                        ButtonRenderCtx {
                            icon_style: Style::default(),
                            icon_gap: 0,
                            shortcut_style: Style::default(),
                            shortcut_gap: 0,
                            style: base,
                            hover_style: hover,
                            focus_style: Style::default(),
                            align: Align::Start,
                            variant: ButtonVariant::Filled,
                            border_style: BorderStyle::Plain,
                            hover_border_style: None,
                            focus_border_style: None,
                            padding: Padding::default(),
                            is_focused: false,
                            is_hovered,
                            disabled: false,
                            disabled_style: Style::default(),
                            contrast_policy: ContrastPolicy::Wcag,
                            clip_rect: None,
                        },
                    );
                })
                .expect("draw");

            let cell = terminal.backend().buffer()[(0, 0)].clone();
            (cell.fg, cell.bg)
        };

        let (fg_idle, bg_idle) = draw(false);
        let (fg_hover, bg_hover) = draw(true);

        assert_ne!(bg_hover, bg_idle);
        assert_ne!(fg_hover, fg_idle);
        assert_ne!(bg_hover, RColor::Reset);
    }

    #[test]
    fn filled_button_hover_alpha_can_opt_out_of_auto_contrast() {
        let panel_bg = Color::Rgb(0x15, 0x15, 0x19);
        let panel_bg_rt = RColor::Rgb(0x15, 0x15, 0x19);
        let alpha_fg = Paint::rgba(0xff, 0xff, 0xff, 0x40);
        let base = Style::new().fg(Color::White).bg(panel_bg);
        let hover = Style::new()
            .fg(alpha_fg)
            .contrast_policy(ContrastPolicy::Off);
        let backend = TestBackend::new(10, 1);
        let mut terminal = Terminal::new(backend).expect("terminal");

        terminal
            .draw(|f| {
                render_button(
                    f,
                    "Hover",
                    None,
                    None,
                    Rect {
                        x: 0,
                        y: 0,
                        w: 10,
                        h: 1,
                    },
                    ratatui::layout::Rect {
                        x: 0,
                        y: 0,
                        width: 10,
                        height: 1,
                    },
                    ButtonRenderCtx {
                        icon_style: Style::default(),
                        icon_gap: 0,
                        shortcut_style: Style::default(),
                        shortcut_gap: 0,
                        style: base,
                        hover_style: hover,
                        focus_style: Style::default(),
                        align: Align::Start,
                        variant: ButtonVariant::Filled,
                        border_style: BorderStyle::Plain,
                        hover_border_style: None,
                        focus_border_style: None,
                        padding: Padding::default(),
                        is_focused: false,
                        is_hovered: true,
                        disabled: false,
                        disabled_style: Style::default(),
                        contrast_policy: ContrastPolicy::Wcag,
                        clip_rect: None,
                    },
                );
            })
            .expect("draw");

        let cell = terminal.backend().buffer()[(0, 0)].clone();
        assert_eq!(cell.symbol(), "H");
        assert_eq!(cell.bg, panel_bg_rt);
        assert_eq!(
            Some(cell.fg),
            blend_paint_over_ratatui(alpha_fg, panel_bg_rt)
        );
    }
}
