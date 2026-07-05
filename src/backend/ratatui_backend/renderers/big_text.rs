use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::backend::ratatui_backend::common::{to_ratatui_color, to_ratatui_rect, to_ratatui_span};
use crate::style::{Rect, Style};
use crate::utils::gradient::{ColorGradient, GradientDirection};
use crate::widgets::Shadow;

pub(crate) struct BigTextRenderCtx {
    pub rrect: ratatui::layout::Rect,
    pub clip_rect: Option<Rect>,
    pub base_style: Style,
    pub gradient: Option<(ColorGradient, GradientDirection)>,
    pub shadow: Option<Shadow>,
}

pub(crate) fn render_big_text(
    f: &mut ratatui::Frame<'_>,
    lines: &[Vec<crate::style::Span>],
    content_width: u16,
    rect: Rect,
    ctx: BigTextRenderCtx,
) {
    let BigTextRenderCtx {
        rrect,
        clip_rect,
        base_style,
        gradient,
        shadow,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let r_clip = clip_rect.map(to_ratatui_rect);
    let effective_rrect = if let Some(clip) = r_clip {
        rrect.intersection(clip)
    } else {
        rrect
    };

    if effective_rrect.is_empty() {
        return;
    }

    let visible_start = (effective_rrect.y as i32)
        .saturating_sub(rect.y as i32)
        .max(0) as usize;
    let max_lines = effective_rrect.height as usize;

    let dx = (effective_rrect.x as i32)
        .saturating_sub(rect.x as i32)
        .max(0) as u16;

    if let Some((grad, dir)) = gradient {
        // Gradient path - precompute a ratatui color LUT once, then index per
        // row or column.  All spans are owned so there are no lifetime issues.
        let lut: Vec<ratatui::style::Color> = match dir {
            GradientDirection::Vertical => grad
                .precompute(lines.len())
                .into_iter()
                .map(to_ratatui_color)
                .collect(),
            GradientDirection::Horizontal => grad
                .precompute(content_width as usize)
                .into_iter()
                .map(to_ratatui_color)
                .collect(),
        };

        let output_lines: Vec<Line<'static>> = lines
            .iter()
            .skip(visible_start)
            .take(max_lines)
            .enumerate()
            .map(|(offset, line)| match dir {
                GradientDirection::Vertical => {
                    let actual_row = visible_start + offset;
                    let color = lut[actual_row.min(lut.len().saturating_sub(1))];
                    let spans: Vec<ratatui::text::Span<'static>> = line
                        .iter()
                        .map(|s| {
                            let mut themed = s.clone();
                            themed.style = base_style.patch(themed.style);
                            let base = to_ratatui_span(&themed, Style::default()).style;
                            let is_shadow = shadow.is_some_and(|sh| sh.style == s.style);
                            ratatui::text::Span::styled(
                                s.content.to_string(),
                                if is_shadow { base } else { base.fg(color) },
                            )
                        })
                        .collect();
                    Line::from(spans)
                }
                GradientDirection::Horizontal => {
                    Line::from(apply_horizontal_gradient(line, &lut, base_style, shadow))
                }
            })
            .collect();

        let p = Paragraph::new(output_lines).scroll((0, dx));
        f.render_widget(p, effective_rrect);
        return;
    }

    // Non-gradient path - unchanged, uses borrowed spans for zero-copy.
    let mut output_lines = Vec::new();
    for line in lines.iter().skip(visible_start).take(max_lines) {
        let spans: Vec<ratatui::text::Span<'_>> = line
            .iter()
            .map(|s| {
                let mut themed = s.clone();
                themed.style = base_style.patch(themed.style);
                ratatui::text::Span::styled(
                    themed.content.to_string(),
                    to_ratatui_span(&themed, Style::default()).style,
                )
            })
            .collect();
        output_lines.push(Line::from(spans));
    }
    let p = Paragraph::new(output_lines).scroll((0, dx));
    f.render_widget(p, effective_rrect);
}

/// Apply a horizontal gradient LUT to one row of spans, producing owned spans.
///
/// Adjacent characters that map to the same ratatui color are merged into a
/// single span to minimise allocations (happens naturally for smooth gradients
/// over short character runs).
fn apply_horizontal_gradient(
    line: &[crate::style::Span],
    col_lut: &[ratatui::style::Color],
    base_style: Style,
    shadow: Option<Shadow>,
) -> Vec<ratatui::text::Span<'static>> {
    let lut_last = col_lut.len().saturating_sub(1);
    let mut spans: Vec<ratatui::text::Span<'static>> = Vec::new();
    let mut col = 0usize;

    for src_span in line {
        let is_shadow = shadow.is_some_and(|sh| sh.style == src_span.style);
        let mut themed = src_span.clone();
        themed.style = base_style.patch(themed.style);
        let span_style = to_ratatui_span(&themed, Style::default()).style;

        if is_shadow {
            // Shadow span: keep original style, advance column counter so
            // subsequent non-shadow spans keep the correct gradient position.
            col += src_span.content.chars().count();
            spans.push(ratatui::text::Span::styled(
                src_span.content.to_string(),
                span_style,
            ));
            continue;
        }

        let mut batch = String::new();
        let mut batch_color = ratatui::style::Color::Reset;

        for ch in src_span.content.chars() {
            let grad_color = col_lut[col.min(lut_last)];
            col += 1;

            if batch.is_empty() {
                batch.push(ch);
                batch_color = grad_color;
            } else if grad_color == batch_color {
                // Same color - extend current batch, no new span needed.
                batch.push(ch);
            } else {
                // Color changed - flush current batch.
                spans.push(ratatui::text::Span::styled(
                    std::mem::take(&mut batch),
                    span_style.fg(batch_color),
                ));
                batch.push(ch);
                batch_color = grad_color;
            }
        }

        if !batch.is_empty() {
            spans.push(ratatui::text::Span::styled(
                batch,
                span_style.fg(batch_color),
            ));
        }
    }

    spans
}
