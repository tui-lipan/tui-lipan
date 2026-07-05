use ratatui::layout::Rect as RRect;
use ratatui::style::{Color as RColor, Modifier as RMod, Style as RStyle};
use ratatui::symbols;
use ratatui::text::Span;
use ratatui::widgets::{BorderType, Borders};

use crate::app::ContrastPolicy;
use crate::style::{BorderStyle, Color, Paint, Rect, Style};

use super::colors::{from_ratatui_color, to_ratatui_color};
use super::style_resolve::{current_render_terminal_bg, finalize_style};

pub(crate) fn to_ratatui_rect(r: Rect) -> RRect {
    let x1 = r.x;
    let y1 = r.y;
    let x2 = r.x.saturating_add(r.w as i16);
    let y2 = r.y.saturating_add(r.h as i16);

    let rx1 = x1.max(0);
    let ry1 = y1.max(0);
    let rx2 = x2.max(0);
    let ry2 = y2.max(0);

    RRect {
        x: rx1 as u16,
        y: ry1 as u16,
        width: rx2.saturating_sub(rx1) as u16,
        height: ry2.saturating_sub(ry1) as u16,
    }
}

pub(crate) fn to_ratatui_style(style: Style) -> RStyle {
    to_ratatui_style_with_terminal_bg(style, current_render_terminal_bg().map(from_ratatui_color))
}

/// Like [`to_ratatui_style`] but uses `terminal_bg` as a fallback fg color when
/// `Color::Transparent` fg could not be resolved to a concrete background (i.e.
/// the bg is also transparent or unset).  Pass the value from
/// `RenderContext::terminal_bg` converted via [`from_ratatui_color`].
pub(crate) fn to_ratatui_style_with_terminal_bg(
    style: Style,
    terminal_bg: Option<Color>,
) -> RStyle {
    let style = finalize_style(style, terminal_bg, ContrastPolicy::Off);
    let mut out = RStyle::default();

    match style.fg {
        Some(fg) if fg.is_transparent_sentinel() => {
            if let Some(tbg) = terminal_bg {
                let resolved = if let Some(t) = style.fg_transform {
                    t.apply_paint_with_backdrop(Paint::Solid(tbg), style.bg)
                } else {
                    Paint::Solid(tbg)
                };
                if let Some(color) = paint_to_ratatui_fg(resolved, terminal_bg) {
                    out = out.fg(color);
                }
            }
        }
        Some(fg) if !fg.is_backdrop_sentinel() => {
            if let Some(color) = paint_to_ratatui_fg(fg, style.bg.map(Paint::color).or(terminal_bg))
            {
                out = out.fg(color);
            }
        }
        _ => {}
    }
    if let Some(bg) = style.bg
        && let Some(color) = paint_to_ratatui_bg(bg, terminal_bg)
    {
        out = out.bg(color);
    }

    match style.bold {
        Some(true) => out = out.add_modifier(RMod::BOLD),
        Some(false) => out = out.remove_modifier(RMod::BOLD),
        None => {}
    }

    match style.dim {
        Some(true) => out = out.add_modifier(RMod::DIM),
        Some(false) => out = out.remove_modifier(RMod::DIM),
        None => {}
    }

    match style.italic {
        Some(true) => out = out.add_modifier(RMod::ITALIC),
        Some(false) => out = out.remove_modifier(RMod::ITALIC),
        None => {}
    }

    match style.underline {
        Some(true) => out = out.add_modifier(RMod::UNDERLINED),
        Some(false) => out = out.remove_modifier(RMod::UNDERLINED),
        None => {}
    }

    match style.reverse {
        Some(true) => out = out.add_modifier(RMod::REVERSED),
        Some(false) => out = out.remove_modifier(RMod::REVERSED),
        None => {}
    }

    match style.strikethrough {
        Some(true) => out = out.add_modifier(RMod::CROSSED_OUT),
        Some(false) => out = out.remove_modifier(RMod::CROSSED_OUT),
        None => {}
    }

    if let Some(color) = style.underline_color
        && let Some(color) = paint_to_ratatui_fg(color, style.bg.map(Paint::color).or(terminal_bg))
    {
        out = out.underline_color(color);
    }

    out
}

pub(crate) fn style_paints_bg(style: Style) -> bool {
    style
        .resolve_color_transforms()
        .bg
        .is_some_and(|bg| !bg.is_transparent_paint() && !bg.is_backdrop_sentinel())
}

pub(crate) fn style_uses_backdrop_bg(style: Style) -> bool {
    style
        .resolve_color_transforms()
        .bg
        .is_some_and(Paint::is_backdrop_sentinel)
}

pub(crate) fn style_has_alpha_paint(style: Style) -> bool {
    let style = style.resolve_color_transforms();
    [style.fg, style.bg, style.underline_color]
        .into_iter()
        .flatten()
        .any(|paint| matches!(paint, Paint::Alpha { alpha: 1..=254, .. }))
}

pub(crate) fn paint_to_ratatui_bg(paint: Paint, backdrop: Option<Color>) -> Option<RColor> {
    if paint.is_transparent_paint() || paint.is_backdrop_sentinel() {
        return None;
    }
    match paint {
        Paint::Solid(color) => Some(to_ratatui_color(color)),
        Paint::Alpha { alpha: 255, color } => Some(to_ratatui_color(color)),
        Paint::Alpha { .. } => blend_paint_over_ratatui(
            paint,
            backdrop.map(to_ratatui_color).unwrap_or(RColor::Reset),
        ),
    }
}

pub(crate) fn paint_to_ratatui_fg(paint: Paint, backdrop: Option<Color>) -> Option<RColor> {
    if paint.is_transparent_paint() || paint.is_backdrop_sentinel() {
        return None;
    }
    match paint {
        Paint::Solid(color) => Some(to_ratatui_color(color)),
        Paint::Alpha { alpha: 255, color } => Some(to_ratatui_color(color)),
        Paint::Alpha { .. } => blend_paint_over_ratatui(
            paint,
            backdrop.map(to_ratatui_color).unwrap_or(RColor::Reset),
        ),
    }
}

pub(crate) fn blend_paint_over_ratatui(paint: Paint, backdrop: RColor) -> Option<RColor> {
    if paint.is_transparent_paint() {
        return None;
    }
    if paint.is_backdrop_sentinel() {
        return Some(backdrop);
    }
    let color = paint.color();
    if paint.alpha_u8() == 255 {
        return Some(to_ratatui_color(color));
    }
    let (sr, sg, sb) = color.to_rgb()?;
    let Some((br, bg, bb)) = from_ratatui_color(backdrop).to_rgb() else {
        return Some(to_ratatui_color(color));
    };
    let alpha = paint.alpha();
    let blend = |src: u8, dst: u8| -> u8 {
        (src as f32 * alpha + dst as f32 * (1.0 - alpha))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Some(RColor::Rgb(blend(sr, br), blend(sg, bg), blend(sb, bb)))
}

pub(crate) fn to_ratatui_span<'a>(span: &'a crate::style::Span, base_style: Style) -> Span<'a> {
    let style = base_style.patch(span.style);
    Span::styled(span.content.as_ref(), to_ratatui_style(style))
}

pub(crate) fn richtext_to_spans<'a>(
    rt: &'a crate::style::RichText,
    base_style: Style,
) -> Vec<Span<'a>> {
    rt.spans
        .iter()
        .map(|s| to_ratatui_span(s, base_style))
        .collect()
}

pub(crate) fn to_ratatui_border_type(style: BorderStyle) -> BorderType {
    match style {
        BorderStyle::Plain => BorderType::Plain,
        BorderStyle::Rounded => BorderType::Rounded,
        BorderStyle::Double => BorderType::Double,
        BorderStyle::Thick => BorderType::Thick,
        BorderStyle::LightDoubleDashed => BorderType::LightDoubleDashed,
        BorderStyle::HeavyDoubleDashed => BorderType::HeavyDoubleDashed,
        BorderStyle::LightTripleDashed => BorderType::LightTripleDashed,
        BorderStyle::HeavyTripleDashed => BorderType::HeavyTripleDashed,
        BorderStyle::LightQuadrupleDashed => BorderType::LightQuadrupleDashed,
        BorderStyle::HeavyQuadrupleDashed => BorderType::HeavyQuadrupleDashed,
        BorderStyle::Custom { .. } => BorderType::Plain,
    }
}

pub(crate) fn to_ratatui_border_set(style: BorderStyle) -> Option<symbols::border::Set<'static>> {
    match style {
        BorderStyle::Plain => Some(symbols::border::PLAIN),
        BorderStyle::Rounded => Some(symbols::border::ROUNDED),
        BorderStyle::Double => Some(symbols::border::DOUBLE),
        BorderStyle::Thick => Some(symbols::border::THICK),
        BorderStyle::LightDoubleDashed => Some(symbols::border::LIGHT_DOUBLE_DASHED),
        BorderStyle::HeavyDoubleDashed => Some(symbols::border::HEAVY_DOUBLE_DASHED),
        BorderStyle::LightTripleDashed => Some(symbols::border::LIGHT_TRIPLE_DASHED),
        BorderStyle::HeavyTripleDashed => Some(symbols::border::HEAVY_TRIPLE_DASHED),
        BorderStyle::LightQuadrupleDashed => Some(symbols::border::LIGHT_QUADRUPLE_DASHED),
        BorderStyle::HeavyQuadrupleDashed => Some(symbols::border::HEAVY_QUADRUPLE_DASHED),
        BorderStyle::Custom { glyphs: g } => Some(symbols::border::Set {
            top_left: g.top_left,
            top_right: g.top_right,
            bottom_left: g.bottom_left,
            bottom_right: g.bottom_right,
            vertical_left: g.left,
            vertical_right: g.right,
            horizontal_top: g.top,
            horizontal_bottom: g.bottom,
        }),
    }
}

pub(crate) fn calculate_visible_borders(rrect_unclipped: Rect, clip_rect: Option<Rect>) -> Borders {
    let Some(clip) = clip_rect else {
        return Borders::ALL;
    };

    let clipped = rrect_unclipped.intersection(&clip);

    if clipped.is_empty() {
        return Borders::empty();
    }

    let full_right = rrect_unclipped.x.saturating_add(rrect_unclipped.w as i16);
    let full_bottom = rrect_unclipped.y.saturating_add(rrect_unclipped.h as i16);
    let clip_right = clipped.x.saturating_add(clipped.w as i16);
    let clip_bottom = clipped.y.saturating_add(clipped.h as i16);

    let mut borders = Borders::empty();
    if clipped.y == rrect_unclipped.y {
        borders |= Borders::TOP;
    }
    if clip_bottom == full_bottom {
        borders |= Borders::BOTTOM;
    }
    if clipped.x == rrect_unclipped.x {
        borders |= Borders::LEFT;
    }
    if clip_right == full_right {
        borders |= Borders::RIGHT;
    }

    borders
}
