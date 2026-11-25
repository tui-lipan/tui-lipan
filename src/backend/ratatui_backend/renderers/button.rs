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
use crate::style::{Align, BorderStyle, Color, Padding, Rect, Style, ThemeRole, resolve_slot};
use crate::utils::text::end_at_width;
use crate::widgets::{ButtonVariant, CapStyle};

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
    pub cap: CapStyle,
    pub cap_behind: Color,
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
        cap,
        cap_behind,
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

            let draw_border = rect.h >= 3 && rect.w >= 3;
            let mut inner = rect;
            if draw_border {
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
            inner = inner.inset_preserving_cell(padding);
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
                    ellipsize_label: rect.w > 2,
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
            let content = content_width(label, icon, shortcut, icon_gap, shortcut_gap);
            let (rect, padding) = if cap.chars().is_some() {
                let rect = draw_caps(f, rect, rrect, cap, cap_behind, s, content);
                (rect, fit_padding(padding, rect.w, content))
            } else {
                (rect, padding)
            };
            let rrect = to_ratatui_rect(rect).intersection(rrect);
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
                    ellipsize_label: true,
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
                            ellipsize_label: true,
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
    ellipsize_label: bool,
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
        ellipsize_label,
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
    let label_visible = if ellipsize_label {
        truncate_end_with_ellipsis(label, available_label)
    } else {
        (&label[..end_at_width(label, 0, available_label.into())]).into()
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

/// Draw `cap` at both ends of a filled button and return the rect left for the body.
///
/// The caps take the fill the body shows: its background, or its foreground when the style is
/// reversed. A button with no opaque fill, or too narrow to keep `content` cells beside the caps,
/// keeps square ends.
fn draw_caps(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    cap: CapStyle,
    behind: Color,
    s: Style,
    content: u16,
) -> Rect {
    let Some((left, right)) = cap.chars() else {
        return rect;
    };
    let Some(fill) = visible_fill(s) else {
        return rect;
    };
    if rect.w < content.max(1).saturating_add(2) {
        return rect;
    }
    let cap_style = ratatui::style::Style {
        fg: Some(fill),
        bg: (behind != Color::Reset)
            .then(|| crate::backend::ratatui_backend::common::to_ratatui_color(behind)),
        ..Default::default()
    };
    let right_x = rect.x.saturating_add(rect.w as i16 - 1);
    for row in 0..rect.h {
        let y = rect.y.saturating_add(row as i16);
        for (x, glyph) in [(rect.x, left), (right_x, right)] {
            let (Ok(cx), Ok(cy)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let position = ratatui::layout::Position { x: cx, y: cy };
            if !rrect.contains(position) {
                continue;
            }
            if let Some(cell) = f.buffer_mut().cell_mut(position) {
                cell.reset();
                cell.set_char(glyph);
                cell.set_style(cap_style);
            }
        }
    }
    Rect {
        x: rect.x.saturating_add(1),
        w: rect.w - 2,
        ..rect
    }
}

/// The color a filled button visibly paints: its background, or its foreground when reversed.
/// `None` when that is transparent, the backdrop, or the terminal default.
fn visible_fill(s: Style) -> Option<ratatui::style::Color> {
    let resolved = to_ratatui_style(s);
    let reversed = resolved
        .add_modifier
        .contains(ratatui::style::Modifier::REVERSED)
        && !resolved
            .sub_modifier
            .contains(ratatui::style::Modifier::REVERSED);
    let fill = if reversed {
        resolved.fg
    } else if style_paints_bg(s) {
        resolved.bg
    } else {
        None
    };
    fill.filter(|color| *color != ratatui::style::Color::Reset)
}

/// Columns the button's icon, label and shortcut take with their gaps.
fn content_width(
    label: &str,
    icon: Option<&str>,
    shortcut: Option<&str>,
    icon_gap: u16,
    shortcut_gap: u16,
) -> u16 {
    let width = |text: &str| u16::try_from(UnicodeWidthStr::width(text)).unwrap_or(u16::MAX);
    let mut total = width(label);
    if let Some(icon) = icon.filter(|icon| !icon.is_empty()) {
        total = total.saturating_add(width(icon)).saturating_add(icon_gap);
    }
    if let Some(shortcut) = shortcut.filter(|shortcut| !shortcut.is_empty()) {
        total = total
            .saturating_add(width(shortcut))
            .saturating_add(shortcut_gap);
    }
    total
}

/// Horizontal padding that leaves `content` columns of `width` for the content: a capped button
/// gives up padding before its label. Right padding goes first, so a centered label stays put.
fn fit_padding(padding: Padding, width: u16, content: u16) -> Padding {
    let spare = width.saturating_sub(content);
    let left = padding
        .left
        .min(spare.div_ceil(2).max(spare.saturating_sub(padding.right)));
    let right = padding.right.min(spare.saturating_sub(left));
    Padding {
        left,
        right,
        ..padding
    }
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
            cap: node.cap,
            cap_behind: node.cap_behind,
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
    use ratatui::buffer::Buffer;
    use ratatui::style::Color as RColor;

    use super::{ButtonRenderCtx, render_button};
    use crate::app::ContrastPolicy;
    use crate::backend::ratatui_backend::common::blend_paint_over_ratatui;
    use crate::style::{Align, BorderStyle, Color, Padding, Paint, Rect, Style};
    use crate::widgets::{ButtonVariant, CapStyle};

    #[test]
    fn filled_button_hover_lighten_recomputes_contrast_from_raw_style() {
        let base = Style::new()
            .fg(Color::rgb(120, 120, 120))
            .bg(Color::rgb(20, 20, 20));
        let hover = Style::new().transform_bg(crate::style::ColorTransform::lighten(0.9));

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
                            cap: CapStyle::Padded,
                            cap_behind: Color::Reset,
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

    /// A round-capped filled button `label` in `width` cells with the default padding, over a
    /// `#0e0e0e` surface.
    fn capped(label: &str, width: u16, state: impl FnOnce(&mut ButtonRenderCtx)) -> Buffer {
        let mut ctx = ButtonRenderCtx {
            icon_style: Style::default(),
            icon_gap: 1,
            shortcut_style: Style::default(),
            shortcut_gap: 1,
            style: Style::new().fg(Color::White).bg(CAP_FILL),
            hover_style: Style::default(),
            focus_style: Style::default(),
            align: Align::Center,
            variant: ButtonVariant::Filled,
            border_style: BorderStyle::Plain,
            hover_border_style: None,
            focus_border_style: None,
            padding: Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            cap: CapStyle::Round,
            cap_behind: CAP_BEHIND,
            is_focused: false,
            is_hovered: false,
            disabled: false,
            disabled_style: Style::default(),
            contrast_policy: ContrastPolicy::Off,
            clip_rect: None,
        };
        state(&mut ctx);
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).expect("terminal");
        terminal
            .draw(|f| {
                let rect = Rect {
                    x: 0,
                    y: 0,
                    w: width,
                    h: 1,
                };
                render_button(
                    f,
                    label,
                    None,
                    None,
                    rect,
                    ratatui::layout::Rect::new(0, 0, width, 1),
                    ctx,
                );
            })
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    const CAP_FILL: Color = Color::Rgb(0x34, 0x6b, 0xf1);
    const CAP_BEHIND: Color = Color::Rgb(0x0e, 0x0e, 0x0e);

    fn row(buffer: &Buffer) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect()
    }

    fn has_caps(buffer: &Buffer) -> bool {
        let (left, right) = CapStyle::Round.chars().expect("round caps");
        let last = buffer.area.width - 1;
        buffer[(0, 0)].symbol() == left.to_string()
            && buffer[(last, 0)].symbol() == right.to_string()
    }

    #[test]
    fn capped_button_keeps_its_label_before_its_caps_and_padding() {
        // (label, width, caps expected): the label shows whenever it fits the width; caps take
        // two cells only once the label fits beside them; padding goes first.
        let cases = [
            ("↑", 1, false),
            ("↑", 2, false),
            ("↑", 3, true),
            ("↑", 4, true),
            ("↑", 5, true),
            ("中", 2, false),
            ("中", 3, false),
            ("中", 4, true),
            ("中", 5, true),
        ];
        for (label, width, caps) in cases {
            let buffer = capped(label, width, |_| {});
            assert!(
                row(&buffer).contains(label),
                "{label:?} in {width} cells shows {:?}",
                row(&buffer)
            );
            assert_eq!(has_caps(&buffer), caps, "{label:?} in {width} cells");
        }
        // A double-width label cannot fit one cell; the button stays square and empty-ended.
        assert!(!has_caps(&capped("中", 1, |_| {})));
    }

    #[test]
    fn caps_take_the_visible_fill() {
        let rgb = |color: Color| crate::backend::ratatui_backend::common::to_ratatui_color(color);
        let cap_fg = |buffer: &Buffer| buffer[(0, 0)].fg;

        // Reversed: the body shows the foreground as its fill, so the caps do too.
        let reversed = capped("Go", 6, |ctx| {
            ctx.style = Style::new().fg(CAP_FILL).bg(Color::White).reverse();
        });
        assert!(has_caps(&reversed));
        assert_eq!(cap_fg(&reversed), rgb(CAP_FILL));

        // No fill, or a transparent one: no caps in the terminal's default color.
        for style in [
            Style::new().fg(Color::White),
            Style::new().fg(Color::White).bg(Color::Transparent),
        ] {
            let buffer = capped("Go", 6, |ctx| ctx.style = style);
            assert!(!has_caps(&buffer), "{style:?}");
            assert!(row(&buffer).contains("Go"));
        }

        // Focus and disabled styles reach the caps like hover does.
        let focus = Color::Rgb(0x50, 0x80, 0xff);
        let focused = capped("Go", 6, |ctx| {
            ctx.focus_style = Style::new().bg(focus);
            ctx.is_focused = true;
        });
        assert_eq!(cap_fg(&focused), rgb(focus));
        let muted = Color::Rgb(0x2a, 0x2a, 0x2a);
        let disabled = capped("Go", 6, |ctx| {
            ctx.disabled_style = Style::new().bg(muted);
            ctx.disabled = true;
        });
        assert_eq!(cap_fg(&disabled), rgb(muted));
        for buffer in [&focused, &disabled] {
            assert_eq!(buffer[(0, 0)].bg, rgb(CAP_BEHIND));
        }
    }

    #[test]
    fn filled_button_caps_take_the_current_fill() {
        let idle = Color::rgb(0x34, 0x6b, 0xf1);
        let hover = Color::rgb(0x50, 0x80, 0xff);
        let behind = Color::rgb(0x0e, 0x0e, 0x0e);
        let draw = |is_hovered| {
            let mut terminal = Terminal::new(TestBackend::new(5, 1)).expect("terminal");
            terminal
                .draw(|f| {
                    let rect = Rect {
                        x: 0,
                        y: 0,
                        w: 5,
                        h: 1,
                    };
                    render_button(
                        f,
                        "↑",
                        None,
                        None,
                        rect,
                        ratatui::layout::Rect::new(0, 0, 5, 1),
                        ButtonRenderCtx {
                            icon_style: Style::default(),
                            icon_gap: 0,
                            shortcut_style: Style::default(),
                            shortcut_gap: 0,
                            style: Style::new().fg(Color::White).bg(idle),
                            hover_style: Style::new().bg(hover),
                            focus_style: Style::default(),
                            align: Align::Center,
                            variant: ButtonVariant::Filled,
                            border_style: BorderStyle::Plain,
                            hover_border_style: None,
                            focus_border_style: None,
                            padding: Padding {
                                left: 1,
                                right: 1,
                                top: 0,
                                bottom: 0,
                            },
                            cap: CapStyle::Round,
                            cap_behind: behind,
                            is_focused: false,
                            is_hovered,
                            disabled: false,
                            disabled_style: Style::default(),
                            contrast_policy: ContrastPolicy::Off,
                            clip_rect: None,
                        },
                    );
                })
                .expect("draw");
            terminal.backend().buffer().clone()
        };

        let rgb = |color: Color| crate::backend::ratatui_backend::common::to_ratatui_color(color);
        for (hovered, fill) in [(false, idle), (true, hover)] {
            let buffer = draw(hovered);
            let (left, right) = CapStyle::Round.chars().expect("round caps");
            assert_eq!(buffer[(0, 0)].symbol(), left.to_string());
            assert_eq!(buffer[(4, 0)].symbol(), right.to_string());
            for x in [0, 4] {
                assert_eq!(buffer[(x, 0)].fg, rgb(fill));
                assert_eq!(buffer[(x, 0)].bg, rgb(behind));
            }
            assert_eq!(buffer[(2, 0)].symbol(), "↑");
            assert_eq!(buffer[(2, 0)].bg, rgb(fill));
        }
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
                        cap: CapStyle::Padded,
                        cap_behind: Color::Reset,
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
