use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Color, Padding, Span, Style};
use crate::utils::gradient::ColorGradient;
use unicode_width::UnicodeWidthStr;

use super::{Heatmap, HeatmapCellMode, HeatmapLegendWidth};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeatmapCacheKey {
    pub(crate) hash: u64,
}

impl HeatmapCacheKey {
    pub(crate) fn new(heatmap: &Heatmap) -> Self {
        Self {
            hash: render_hash(heatmap),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeatmapWidgetKey {
    pub(crate) hash: u64,
}

impl HeatmapWidgetKey {
    pub(crate) fn new(heatmap: &Heatmap, cache_key: HeatmapCacheKey) -> Self {
        Self {
            hash: widget_hash(heatmap, cache_key),
        }
    }
}

/// Precomputed render output.
#[derive(Clone, Debug)]
pub(crate) struct HeatmapRenderOutput {
    pub data_lines: Vec<Vec<Span>>,
    pub header_line: Option<Vec<Span>>,
    pub total_data_rows: usize,
    pub value_min: f64,
    pub value_max: f64,
}

/// Runtime node for a heatmap widget.
#[derive(Clone)]
pub struct HeatmapNode {
    pub row_labels: Vec<Arc<str>>,
    pub column_labels: Vec<Arc<str>>,
    pub gradient: ColorGradient,
    pub cell_mode: HeatmapCellMode,
    pub cell_width: u16,
    pub gap_x: u16,
    pub gap_y: u16,
    pub legend_gap: u16,
    pub legend_spacing: u16,
    pub legend_width: HeatmapLegendWidth,
    pub show_values: bool,
    pub show_legend: bool,
    pub style: Style,
    pub label_style: Style,
    pub legend_style: Style,
    pub padding: Padding,
    pub border: bool,
    pub border_style: BorderStyle,
    pub(crate) output: Arc<HeatmapRenderOutput>,
    pub(crate) cache_key: HeatmapCacheKey,
    pub(crate) widget_key: HeatmapWidgetKey,
}

impl Default for HeatmapNode {
    fn default() -> Self {
        Self {
            row_labels: Vec::new(),
            column_labels: Vec::new(),
            gradient: ColorGradient::new(Color::Rgb(60, 179, 113), Color::Rgb(226, 82, 87)),
            cell_mode: HeatmapCellMode::Background,
            cell_width: 4,
            gap_x: 0,
            gap_y: 0,
            legend_gap: 0,
            legend_spacing: 0,
            legend_width: HeatmapLegendWidth::Grid,
            show_values: false,
            show_legend: false,
            style: Style::default(),
            label_style: Style::default(),
            legend_style: Style::default(),
            padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            output: Arc::new(HeatmapRenderOutput {
                data_lines: Vec::new(),
                header_line: None,
                total_data_rows: 0,
                value_min: 0.0,
                value_max: 1.0,
            }),
            cache_key: HeatmapCacheKey { hash: 0 },
            widget_key: HeatmapWidgetKey { hash: 0 },
        }
    }
}

impl From<Heatmap> for HeatmapNode {
    fn from(heatmap: Heatmap) -> Self {
        let mut node = Self::default();
        super::reconcile_heatmap_node(&heatmap, &mut node);
        node
    }
}

impl WidgetNode for HeatmapNode {}

pub(crate) fn build_render_output(heatmap: &Heatmap) -> HeatmapRenderOutput {
    let rows = heatmap.data.len();
    let cols = heatmap.data.iter().map(|row| row.len()).max().unwrap_or(0);

    if rows == 0 || cols == 0 {
        return HeatmapRenderOutput {
            data_lines: Vec::new(),
            header_line: None,
            total_data_rows: 0,
            value_min: 0.0,
            value_max: 1.0,
        };
    }

    // Resolve range from data or explicit bounds.
    let (value_min, value_max) = resolve_range(heatmap);
    let cell_width = heatmap.effective_cell_width();
    let row_stride = heatmap.gap_y as usize + 1;
    let mut data_lines = Vec::with_capacity(rows.saturating_mul(row_stride));
    for (row_idx, row) in heatmap.data.iter().enumerate() {
        let mut spans = Vec::with_capacity(cols.saturating_mul(2));
        for col_idx in 0..cols {
            let value = row.get(col_idx).copied().unwrap_or(0.0);
            let t = normalize(value, value_min, value_max);
            let color = heatmap.gradient.color_at(t);
            spans.push(render_cell_span(heatmap, value, color, cell_width));
            if col_idx + 1 < cols && heatmap.gap_x > 0 {
                spans.push(Span::new(" ".repeat(heatmap.gap_x as usize)));
            }
        }
        data_lines.push(spans);
        if row_idx + 1 < rows {
            for _ in 0..heatmap.gap_y {
                data_lines.push(Vec::new());
            }
        }
    }
    let header_line = build_header_line(&heatmap.column_labels, cell_width, cols, heatmap.gap_x);
    let total_data_rows = rows
        .saturating_mul(row_stride)
        .saturating_sub(heatmap.gap_y as usize);

    HeatmapRenderOutput {
        data_lines,
        header_line,
        total_data_rows,
        value_min,
        value_max,
    }
}

pub(crate) fn render_hash(heatmap: &Heatmap) -> u64 {
    let mut hasher = DefaultHasher::new();
    heatmap.cell_mode.hash(&mut hasher);
    heatmap.cell_width.hash(&mut hasher);
    heatmap.gap_x.hash(&mut hasher);
    heatmap.gap_y.hash(&mut hasher);
    heatmap.legend_gap.hash(&mut hasher);
    heatmap.legend_spacing.hash(&mut hasher);
    heatmap.legend_width.hash(&mut hasher);
    heatmap.gradient.hash(&mut hasher);
    heatmap.range_min.map(f64::to_bits).hash(&mut hasher);
    heatmap.range_max.map(f64::to_bits).hash(&mut hasher);
    heatmap.data.len().hash(&mut hasher);
    for row in &heatmap.data {
        row.len().hash(&mut hasher);
        for value in row {
            value.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub(crate) fn widget_hash(heatmap: &Heatmap, cache_key: HeatmapCacheKey) -> u64 {
    let mut hasher = DefaultHasher::new();
    cache_key.hash.hash(&mut hasher);
    heatmap.row_labels.hash(&mut hasher);
    heatmap.column_labels.hash(&mut hasher);
    heatmap.show_values.hash(&mut hasher);
    heatmap.show_legend.hash(&mut hasher);
    heatmap.style.hash(&mut hasher);
    heatmap.label_style.hash(&mut hasher);
    heatmap.legend_style.hash(&mut hasher);
    heatmap.padding.hash(&mut hasher);
    heatmap.border.hash(&mut hasher);
    heatmap.border_style.hash(&mut hasher);
    heatmap.legend_gap.hash(&mut hasher);
    heatmap.legend_spacing.hash(&mut hasher);
    heatmap.legend_width.hash(&mut hasher);
    heatmap.width.hash(&mut hasher);
    heatmap.height.hash(&mut hasher);
    hasher.finish()
}

fn resolve_range(heatmap: &Heatmap) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for row in &heatmap.data {
        for &value in row {
            if value.is_finite() {
                min = min.min(value);
                max = max.max(value);
            }
        }
    }
    if let Some(explicit_min) = heatmap.range_min {
        min = explicit_min;
    }
    if let Some(explicit_max) = heatmap.range_max {
        max = explicit_max;
    }
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    if (max - min).abs() < f64::EPSILON {
        (min, min + 1.0)
    } else {
        (min, max)
    }
}

fn normalize(value: f64, min: f64, max: f64) -> f64 {
    if (max - min).abs() < f64::EPSILON {
        0.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}

fn format_cell_value(value: f64, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let formatted = if value.abs() >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };

    if formatted.len() > width {
        formatted.chars().take(width).collect()
    } else {
        format!("{:>width$}", formatted, width = width)
    }
}

fn render_cell_span(heatmap: &Heatmap, value: f64, color: Color, cell_width: u16) -> Span {
    match &heatmap.cell_mode {
        HeatmapCellMode::Background => Span {
            content: if heatmap.show_values {
                Arc::from(format_cell_value(value, heatmap.cell_width))
            } else {
                Arc::from(" ".repeat(cell_width as usize))
            },
            style: Style {
                bg: Some(color.into()),
                ..Style::default()
            },
            allow_row_style: true,
        },
        HeatmapCellMode::Glyph(glyph) => Span {
            content: Arc::from(center_glyph(glyph.as_ref(), cell_width)),
            style: Style {
                bg: Some(color.into()),
                ..Style::default()
            },
            allow_row_style: true,
        },
        HeatmapCellMode::GlyphForeground(glyph) => Span {
            content: Arc::from(center_glyph(glyph.as_ref(), cell_width)),
            style: Style {
                fg: Some(color.into()),
                ..Style::default()
            },
            allow_row_style: true,
        },
    }
}

fn center_glyph(glyph: &str, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let glyph_width = UnicodeWidthStr::width(glyph).max(1);
    let glyph_width = glyph_width.min(width);
    let left = (width.saturating_sub(glyph_width)) / 2;
    let right = width.saturating_sub(left + glyph_width);

    let mut out = String::with_capacity(width);
    out.push_str(&" ".repeat(left));
    out.push_str(glyph);
    out.push_str(&" ".repeat(right));
    out
}

fn build_header_line(
    labels: &[Arc<str>],
    cell_width: u16,
    cols: usize,
    gap_x: u16,
) -> Option<Vec<Span>> {
    if labels.is_empty() {
        return None;
    }

    let mut spans = Vec::with_capacity(cols.saturating_mul(2));
    for col_idx in 0..cols {
        let label = labels.get(col_idx).map(Arc::as_ref).unwrap_or("");
        spans.push(Span::new(fit_cell_text(label, cell_width)));
        if col_idx + 1 < cols && gap_x > 0 {
            spans.push(Span::new(" ".repeat(gap_x as usize)));
        }
    }
    Some(spans)
}

fn fit_cell_text(text: &str, width: u16) -> String {
    let width = width as usize;
    if width == 0 {
        return String::new();
    }

    let content: String = text.chars().take(width).collect();
    format!("{content:<width$}", width = width)
}
