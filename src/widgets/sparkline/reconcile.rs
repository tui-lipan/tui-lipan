use std::cmp::Ordering;
use std::sync::Arc;

use crate::style::{Length, Span, Style};
use crate::utils::braille::{braille_char, braille_fill_mask};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::Overflow;

use super::node::{SparklineCacheKey, SparklineNode, SparklineRenderOutput, SparklineWidgetKey};
use super::{
    DEFAULT_BARS, LINE_EAST, LINE_NORTH, LINE_POINT, LINE_SOUTH, LINE_WEST, PointTrend, Sparkline,
    SparklineAggregation, SparklineLineGlyphs, SparklineVariant, SparklineZeroPolicy,
};

pub fn reconcile_sparkline(
    spark: &Sparkline,
    node: &mut SparklineNode,
    allocated_width: Option<u16>,
) -> bool {
    let plan = plan_data_preparation(spark, allocated_width);
    let cache_key = SparklineCacheKey::new_with_plan(spark, plan);
    let widget_key = SparklineWidgetKey::new(spark, cache_key);
    if widget_key == node.widget_key {
        return false;
    }

    let output = if cache_key == node.cache_key {
        node.output.clone()
    } else if let Some(cached) = super::get_cached_output(&cache_key) {
        cached
    } else {
        let rendered = Arc::new(render_sparkline_with_plan(spark, plan));
        super::insert_cached_output(cache_key, rendered.clone());
        rendered
    };

    node.data = spark.data.clone();
    node.min = spark.min;
    node.max = spark.max;
    node.bars = spark.bars.clone();
    node.variant = spark.variant;
    node.max_points = spark.max_points;
    node.aggregation = spark.aggregation;
    node.zero_policy = spark.zero_policy;
    node.line_glyphs = spark.line_glyphs;
    node.chart_height = spark.chart_height;
    node.mirror_x = spark.mirror_x;
    node.mirror_y = spark.mirror_y;
    node.style = spark.style;
    node.rising_style = spark.rising_style;
    node.falling_style = spark.falling_style;
    node.flat_style = spark.flat_style;
    node.turn_style = spark.turn_style;
    node.gradient = spark.gradient;
    node.height_gradient = spark.height_gradient;
    node.gradient_range = spark.gradient_range;
    node.width = spark.width;
    node.height = spark.height;
    node.overflow = spark.overflow;
    node.output = output;
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}

/// Strategy for reducing the raw data buffer to a renderable slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PrepareStrategy {
    /// Keep the last `target` samples - newest end stays visible (scrolling).
    Tail,
    /// Keep the first `target` samples - oldest end stays visible.
    Head,
    /// Bucket-aggregate the full buffer into `target` points.
    Downsample,
}

/// Plan derived from spark + allocated width. `None` target = render the raw buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PreparePlan {
    pub(crate) target: Option<usize>,
    pub(crate) strategy: PrepareStrategy,
}

/// Convert a column width to a sample count (braille packs two samples per cell).
fn samples_per_width(variant: SparklineVariant, cols: u16) -> usize {
    match variant {
        SparklineVariant::Braille => (cols as usize) * 2,
        _ => cols as usize,
    }
}

/// Decide how to reduce the raw buffer to the target sample count.
pub(crate) fn plan_data_preparation(
    spark: &Sparkline,
    allocated_width: Option<u16>,
) -> PreparePlan {
    let width_target = match spark.width {
        Length::Flex(_) | Length::Percent(_) => {
            allocated_width.map(|w| samples_per_width(spark.variant, w))
        }
        Length::Px(_) | Length::Auto => None,
    };

    // Explicit max_points always means "downsample the whole buffer to fit".
    if let Some(mp) = spark.max_points {
        let target = match width_target {
            Some(w) => mp.min(w),
            None => mp,
        };
        return PreparePlan {
            target: Some(target),
            strategy: PrepareStrategy::Downsample,
        };
    }

    let Some(target) = width_target else {
        return PreparePlan {
            target: None,
            strategy: PrepareStrategy::Tail,
        };
    };

    let strategy = match spark.overflow {
        Overflow::Wrap => PrepareStrategy::Downsample,
        Overflow::Clip | Overflow::Ellipsis => PrepareStrategy::Head,
        Overflow::Auto | Overflow::ClipStart => PrepareStrategy::Tail,
    };
    PreparePlan {
        target: Some(target),
        strategy,
    }
}

fn prepare_data(spark: &Sparkline, plan: PreparePlan) -> Vec<u64> {
    let Some(target) = plan.target else {
        return spark.data.to_vec();
    };
    if target == 0 || spark.data.is_empty() {
        return Vec::new();
    }
    if spark.data.len() <= target {
        return spark.data.to_vec();
    }
    match plan.strategy {
        PrepareStrategy::Tail => spark.data[spark.data.len() - target..].to_vec(),
        PrepareStrategy::Head => spark.data[..target].to_vec(),
        PrepareStrategy::Downsample => {
            downsample_data(&spark.data, Some(target), spark.aggregation)
        }
    }
}

fn render_sparkline_with_plan(spark: &Sparkline, plan: PreparePlan) -> SparklineRenderOutput {
    let mut data = prepare_data(spark, plan);
    if spark.mirror_x {
        data.reverse();
    }

    if data.is_empty() {
        return SparklineRenderOutput { rows: Vec::new() };
    }

    let (min, max) = resolve_bounds(&data, spark.min, spark.max);
    let trends = classify_trends(&data);
    let point_styles = build_point_styles(spark, &data, &trends, (min, max));
    let styled =
        point_styles.iter().any(|style| !style.is_empty()) || spark.height_gradient.is_some();

    let rows = match spark.variant {
        SparklineVariant::Bars => {
            let bars = effective_bars(spark);
            if spark.chart_height > 1 {
                let rows = render_bars_rows(
                    &data,
                    min,
                    max,
                    &bars,
                    spark.chart_height as usize,
                    spark.mirror_y,
                    spark.zero_policy,
                );
                if styled {
                    build_styled_multiline_spans(&rows, &point_styles, spark.height_gradient)
                } else {
                    rows_to_unstyled_spans(&rows)
                }
            } else {
                let chars =
                    render_bars_chars(&data, min, max, &bars, spark.mirror_y, spark.zero_policy);
                if styled {
                    let row_styles =
                        styles_with_height_gradient(&point_styles, spark.height_gradient, 0, 1);
                    vec![build_styled_row_spans(&chars, &row_styles)]
                } else {
                    vec![vec![Span::new(chars.into_iter().collect::<String>())]]
                }
            }
        }
        SparklineVariant::Braille => {
            let rows = render_braille_rows(
                &data,
                min,
                max,
                spark.chart_height as usize,
                spark.mirror_y,
                spark.zero_policy,
            );
            if styled {
                let cell_styles = braille_cell_styles(&point_styles);
                build_styled_multiline_spans(&rows, &cell_styles, spark.height_gradient)
            } else {
                rows_to_unstyled_spans(&rows)
            }
        }
        SparklineVariant::Line => {
            if spark.chart_height > 1 {
                let rows = render_line_rows(spark, &data, min, max, spark.chart_height as usize);
                if styled {
                    build_styled_multiline_spans(&rows, &point_styles, spark.height_gradient)
                } else {
                    rows_to_unstyled_spans(&rows)
                }
            } else {
                let chars = render_line_chars(spark, &data, &trends);
                if styled {
                    let row_styles =
                        styles_with_height_gradient(&point_styles, spark.height_gradient, 0, 1);
                    vec![build_styled_row_spans(&chars, &row_styles)]
                } else {
                    vec![vec![Span::new(chars.into_iter().collect::<String>())]]
                }
            }
        }
    };

    SparklineRenderOutput { rows }
}

// Logic functions (copied from sparkline.rs and adapted)

fn effective_bars(spark: &Sparkline) -> Vec<char> {
    if spark.bars.len() < 2 {
        DEFAULT_BARS.to_vec()
    } else {
        spark.bars.clone()
    }
}

fn render_bars_chars(
    data: &[u64],
    min: u64,
    max: u64,
    bars: &[char],
    mirror_y: bool,
    zero_policy: SparklineZeroPolicy,
) -> Vec<char> {
    let range = max.saturating_sub(min).max(1) as f64;
    let max_index = bars.len().saturating_sub(1) as f64;

    let mut out = Vec::with_capacity(data.len());
    for &value in data {
        let clamped = value.clamp(min, max);
        let mut normalized = clamped.saturating_sub(min) as f64 / range;
        if mirror_y {
            normalized = 1.0 - normalized;
        }
        let normalized = normalized * max_index;
        let mut idx = normalized.round() as usize;
        if zero_policy == SparklineZeroPolicy::MinGlyph && value == 0 {
            idx = idx.max(1);
        }
        out.push(bars[idx.min(bars.len().saturating_sub(1))]);
    }

    out
}

fn render_bars_rows(
    data: &[u64],
    min: u64,
    max: u64,
    bars: &[char],
    rows: usize,
    mirror_y: bool,
    zero_policy: SparklineZeroPolicy,
) -> Vec<Vec<char>> {
    let rows = rows.max(1);
    let levels_per_row = bars.len().saturating_sub(1).max(1);
    let total_levels = levels_per_row * rows;
    let range = max.saturating_sub(min).max(1) as f64;

    let mut matrix = vec![vec![' '; data.len()]; rows];

    for (x, &value) in data.iter().enumerate() {
        let clamped = value.clamp(min, max);
        let normalized = clamped.saturating_sub(min) as f64 / range;
        let mut levels = (normalized * total_levels as f64).round() as usize;
        if zero_policy == SparklineZeroPolicy::MinGlyph && value == 0 {
            levels = levels.max(1);
        }
        let levels = levels.min(total_levels);

        for (row_idx, row) in matrix.iter_mut().enumerate() {
            let level_floor = if mirror_y {
                row_idx * levels_per_row
            } else {
                (rows - 1 - row_idx) * levels_per_row
            };
            let level_in_row = levels.saturating_sub(level_floor).min(levels_per_row);
            row[x] = bars[level_in_row];
        }
    }

    matrix
}

fn render_braille_rows(
    data: &[u64],
    min: u64,
    max: u64,
    rows: usize,
    mirror_y: bool,
    zero_policy: SparklineZeroPolicy,
) -> Vec<Vec<char>> {
    let rows = rows.max(1);
    if data.is_empty() {
        return vec![Vec::new(); rows];
    }

    let total_dot_rows = rows * 4;
    let heights = quantize_braille_heights(data, min, max, total_dot_rows, zero_policy);
    let cell_count = heights.len().div_ceil(2);
    let mut matrix = vec![vec![' '; cell_count]; rows];

    for cell in 0..cell_count {
        let left_height = heights[cell * 2];
        let right_height = heights.get(cell * 2 + 1).copied().unwrap_or(0);

        for (row_idx, row) in matrix.iter_mut().enumerate() {
            let left_fill = row_fill_level(left_height, row_idx, rows, mirror_y);
            let right_fill = row_fill_level(right_height, row_idx, rows, mirror_y);
            let mask = braille_fill_mask(left_fill, true, mirror_y)
                | braille_fill_mask(right_fill, false, mirror_y);
            row[cell] = braille_char(mask);
        }
    }

    matrix
}

fn quantize_braille_heights(
    data: &[u64],
    min: u64,
    max: u64,
    total_dot_rows: usize,
    zero_policy: SparklineZeroPolicy,
) -> Vec<usize> {
    let range = max.saturating_sub(min).max(1) as f64;
    let max_level = total_dot_rows as f64;

    data.iter()
        .map(|&value| {
            let clamped = value.clamp(min, max);
            let normalized = clamped.saturating_sub(min) as f64 / range;
            (normalized * max_level).round() as usize
        })
        .zip(data.iter().copied())
        .map(|(mut level, value)| {
            if zero_policy == SparklineZeroPolicy::MinGlyph && value == 0 {
                level = level.max(1);
            }
            level.min(total_dot_rows)
        })
        .collect()
}

fn row_fill_level(height: usize, row_idx: usize, rows: usize, mirror_y: bool) -> usize {
    let row_floor = if mirror_y {
        row_idx * 4
    } else {
        (rows - 1 - row_idx) * 4
    };
    height.saturating_sub(row_floor).min(4)
}

fn rows_to_unstyled_spans(rows: &[Vec<char>]) -> Vec<Vec<Span>> {
    rows.iter()
        .map(|row| vec![Span::new(row.iter().collect::<String>())])
        .collect()
}

fn braille_cell_styles(styles: &[Style]) -> Vec<Style> {
    if styles.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(styles.len().div_ceil(2));
    for idx in (0..styles.len()).step_by(2) {
        let left = styles[idx];
        let right = styles.get(idx + 1).copied().unwrap_or_default();
        out.push(if right.is_empty() { left } else { right });
    }
    out
}

fn render_line_chars(spark: &Sparkline, data: &[u64], trends: &[PointTrend]) -> Vec<char> {
    let mut out = Vec::with_capacity(trends.len());
    let glyphs = resolved_line_glyphs(spark);

    for (idx, trend) in trends.iter().enumerate() {
        let left = if idx > 0 {
            sign(data[idx], data[idx - 1])
        } else {
            0
        };
        let right = if idx + 1 < data.len() {
            sign(data[idx + 1], data[idx])
        } else {
            0
        };
        let ch = match trend {
            PointTrend::Rising => {
                if spark.mirror_y {
                    glyphs.falling
                } else {
                    glyphs.rising
                }
            }
            PointTrend::Falling => {
                if spark.mirror_y {
                    glyphs.rising
                } else {
                    glyphs.falling
                }
            }
            PointTrend::Flat => glyphs.flat,
            PointTrend::Turn => {
                if left > 0 && right < 0 {
                    if spark.mirror_y {
                        glyphs.valley
                    } else {
                        glyphs.peak
                    }
                } else if left < 0 && right > 0 {
                    if spark.mirror_y {
                        glyphs.peak
                    } else {
                        glyphs.valley
                    }
                } else {
                    glyphs.flat
                }
            }
        };
        out.push(ch);
    }

    out
}

fn render_line_rows(
    spark: &Sparkline,
    data: &[u64],
    min: u64,
    max: u64,
    rows: usize,
) -> Vec<Vec<char>> {
    let rows = rows.max(1);
    if data.is_empty() {
        return vec![Vec::new(); rows];
    }

    let levels = quantize_line_rows(data, min, max, rows, spark.mirror_y);
    let mut masks = vec![vec![0u8; data.len()]; rows];

    if let Some(&y) = levels.first() {
        masks[y][0] |= LINE_POINT;
    }

    for x in 1..levels.len() {
        draw_line_step(&mut masks, x - 1, levels[x - 1], x, levels[x]);
    }

    let ascii = spark.line_glyphs == SparklineLineGlyphs::ASCII;
    masks
        .into_iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let is_top = row_idx == 0;
            let is_bottom = row_idx + 1 == rows;
            row.into_iter()
                .map(|mask| line_cell_char(mask, ascii, is_top, is_bottom))
                .collect()
        })
        .collect()
}

fn resolved_line_glyphs(spark: &Sparkline) -> SparklineLineGlyphs {
    spark.line_glyphs
}

fn quantize_line_rows(data: &[u64], min: u64, max: u64, rows: usize, mirror_y: bool) -> Vec<usize> {
    let rows = rows.max(1);
    if rows == 1 {
        return vec![0; data.len()];
    }

    let range = max.saturating_sub(min).max(1) as f64;
    let max_row = (rows - 1) as f64;

    data.iter()
        .map(|&value| {
            let clamped = value.clamp(min, max);
            let normalized = clamped.saturating_sub(min) as f64 / range;
            let y = if mirror_y {
                normalized * max_row
            } else {
                (1.0 - normalized) * max_row
            };
            y.round() as usize
        })
        .map(|y| y.min(rows.saturating_sub(1)))
        .collect()
}

fn draw_line_step(masks: &mut [Vec<u8>], x0: usize, y0: usize, x1: usize, y1: usize) {
    if x1 <= x0 {
        return;
    }

    if y0 != y1 {
        let step: isize = if y1 > y0 { 1 } else { -1 };
        let mut y = y0 as isize;
        while y != y1 as isize {
            let next = y + step;
            if step > 0 {
                set_line_bit(masks, x0, y as usize, LINE_SOUTH);
                set_line_bit(masks, x0, next as usize, LINE_NORTH);
            } else {
                set_line_bit(masks, x0, y as usize, LINE_NORTH);
                set_line_bit(masks, x0, next as usize, LINE_SOUTH);
            }
            y = next;
        }
    }

    set_line_bit(masks, x0, y1, LINE_EAST);
    set_line_bit(masks, x1, y1, LINE_WEST);
}

fn set_line_bit(masks: &mut [Vec<u8>], x: usize, y: usize, bit: u8) {
    if let Some(row) = masks.get_mut(y)
        && let Some(cell) = row.get_mut(x)
    {
        *cell |= bit;
    }
}

fn line_cell_char(mask: u8, ascii: bool, is_top_row: bool, is_bottom_row: bool) -> char {
    let conn = mask & 0b1111;
    if conn == 0 {
        if mask & LINE_POINT != 0 {
            if ascii { '.' } else { '•' }
        } else {
            ' '
        }
    } else if ascii {
        match conn {
            0b0001 | 0b0100 | 0b0101 => '|',
            // Horizontals track cell position so plateaus read as baselines:
            // bottom row → `_`, top row → `‾` (OVERLINE, U+203E - widely
            // supported), middle rows → `-`. `‾` is the only non-7-bit-ASCII
            // char in the preset; see `SparklineLinePreset::Ascii` docs.
            0b0010 | 0b1000 | 0b1010 => {
                if is_bottom_row {
                    '_'
                } else if is_top_row {
                    '‾'
                } else {
                    '-'
                }
            }
            // Corners - use slashes so multi-row ASCII lines read as slopes
            // instead of boxy `+` junctions.
            // 0b0110 (SE) = top of a rise turning right - `/`
            // 0b1001 (WN) = bottom of a fall leveling out rightward - `/`
            0b0110 | 0b1001 => '/',
            // 0b0011 (NE) = bottom of a fall meeting horizontal - `\`
            // 0b1100 (WS) = top of a plateau dropping down - `\`
            0b0011 | 0b1100 => '\\',
            // T-junctions and crosses remain `+`.
            _ => '+',
        }
    } else {
        match conn {
            0b0001 => '╵',
            0b0010 => '╶',
            0b0011 => '╰',
            0b0100 => '╷',
            0b0101 => '│',
            0b0110 => '╭',
            0b0111 => '├',
            0b1000 => '╴',
            0b1001 => '╯',
            0b1010 => '─',
            0b1011 => '┴',
            0b1100 => '╮',
            0b1101 => '┤',
            0b1110 => '┬',
            0b1111 => '┼',
            _ => '·',
        }
    }
}

fn build_styled_row_spans(chars: &[char], point_styles: &[Style]) -> Vec<Span> {
    if chars.is_empty() {
        return Vec::new();
    }

    let mut spans = Vec::new();
    let mut current_style = point_styles[0];
    let mut current = String::new();

    for (ch, style) in chars.iter().zip(point_styles.iter()) {
        if *style == current_style {
            current.push(*ch);
        } else {
            spans.push(Span::new(std::mem::take(&mut current)).style(current_style));
            current.push(*ch);
            current_style = *style;
        }
    }

    spans.push(Span::new(current).style(current_style));
    spans
}

fn build_styled_multiline_spans(
    rows: &[Vec<char>],
    point_styles: &[Style],
    height_gradient: Option<ColorGradient>,
) -> Vec<Vec<Span>> {
    let mut result = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let row_styles =
            styles_with_height_gradient(point_styles, height_gradient, idx, rows.len());
        result.push(build_styled_row_spans(row, &row_styles));
    }
    result
}

fn styles_with_height_gradient(
    point_styles: &[Style],
    height_gradient: Option<ColorGradient>,
    row_idx: usize,
    row_count: usize,
) -> Vec<Style> {
    let Some(gradient) = height_gradient else {
        return point_styles.to_vec();
    };

    let t = if row_count <= 1 {
        0.0
    } else {
        row_idx as f64 / (row_count - 1) as f64
    };
    let row_color = gradient.color_at(t);
    let row_patch = Style::new().fg(row_color);

    point_styles
        .iter()
        .copied()
        .map(|style| style.patch(row_patch))
        .collect()
}

fn build_point_styles(
    spark: &Sparkline,
    data: &[u64],
    trends: &[PointTrend],
    bounds: (u64, u64),
) -> Vec<Style> {
    let gradient_range = spark
        .gradient_range
        .unwrap_or(GradientRange::new(bounds.0, bounds.1));

    data.iter()
        .zip(trends.iter())
        .map(|(&value, &trend)| {
            let mut style = trend_style(trend, spark);
            if let Some(gradient) = spark.gradient {
                let color = gradient.color_for(value, gradient_range);
                style = style.patch(Style::new().fg(color));
            }
            style
        })
        .collect()
}

fn trend_style(trend: PointTrend, spark: &Sparkline) -> Style {
    match trend {
        PointTrend::Rising => spark.rising_style,
        PointTrend::Falling => spark.falling_style,
        PointTrend::Flat => spark.flat_style,
        PointTrend::Turn => spark.turn_style,
    }
}

fn classify_trends(data: &[u64]) -> Vec<PointTrend> {
    let mut out = Vec::with_capacity(data.len());
    if data.is_empty() {
        return out;
    }

    for i in 0..data.len() {
        let left = if i > 0 { sign(data[i], data[i - 1]) } else { 0 };
        let right = if i + 1 < data.len() {
            sign(data[i + 1], data[i])
        } else {
            0
        };

        let trend = if left != 0 && right != 0 && left != right {
            PointTrend::Turn
        } else {
            let dir = if right != 0 { right } else { left };
            match dir.cmp(&0) {
                Ordering::Greater => PointTrend::Rising,
                Ordering::Less => PointTrend::Falling,
                Ordering::Equal => PointTrend::Flat,
            }
        };

        out.push(trend);
    }

    out
}

fn resolve_bounds(data: &[u64], min: Option<u64>, max: Option<u64>) -> (u64, u64) {
    let mut lo = min.unwrap_or_else(|| *data.iter().min().unwrap_or(&0));
    let mut hi = max.unwrap_or_else(|| *data.iter().max().unwrap_or(&1));

    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if lo == hi {
        hi = lo.saturating_add(1);
    }

    (lo, hi)
}

fn sign(a: u64, b: u64) -> i8 {
    match a.cmp(&b) {
        Ordering::Greater => 1,
        Ordering::Less => -1,
        Ordering::Equal => 0,
    }
}

fn downsample_data(
    data: &[u64],
    max_points: Option<usize>,
    aggregation: SparklineAggregation,
) -> Vec<u64> {
    let Some(max_points) = max_points else {
        return data.to_vec();
    };
    if max_points == 0 || data.len() <= max_points {
        return data.to_vec();
    }

    let mut out = Vec::with_capacity(max_points);
    let len = data.len();
    for bucket in 0..max_points {
        let start = bucket * len / max_points;
        let mut end = (bucket + 1) * len / max_points;
        if end <= start {
            end = (start + 1).min(len);
        }
        let slice = &data[start..end];
        out.push(aggregate_slice(slice, aggregation));
    }

    out
}

fn aggregate_slice(values: &[u64], aggregation: SparklineAggregation) -> u64 {
    match aggregation {
        SparklineAggregation::Average => {
            let sum: u128 = values.iter().map(|&v| v as u128).sum();
            (sum / values.len() as u128) as u64
        }
        SparklineAggregation::Min => values.iter().copied().min().unwrap_or(0),
        SparklineAggregation::Max => values.iter().copied().max().unwrap_or(0),
        SparklineAggregation::First => values.first().copied().unwrap_or(0),
        SparklineAggregation::Last => values.last().copied().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::reconcile_sparkline;
    use crate::style::Length;
    use crate::widgets::sparkline::Sparkline;

    #[test]
    fn unchanged_sparkline_reuses_output() {
        let spark = Sparkline::new([1, 2, 3, 4, 5]).chart_height(2);
        let mut node = super::SparklineNode::default();

        assert!(reconcile_sparkline(&spark, &mut node, None));
        let first_output = node.output.clone();

        assert!(!reconcile_sparkline(&spark, &mut node, None));
        assert!(Arc::ptr_eq(&first_output, &node.output));
    }

    #[test]
    fn width_change_keeps_render_output() {
        let spark = Sparkline::new([2, 4, 6, 8, 10]).chart_height(3);
        let mut node = super::SparklineNode::default();

        assert!(reconcile_sparkline(&spark, &mut node, None));
        let first_output = node.output.clone();

        let widened = spark.clone().width(Length::Px(40));
        assert!(reconcile_sparkline(&widened, &mut node, None));
        assert!(Arc::ptr_eq(&first_output, &node.output));
    }

    #[test]
    fn tail_overflow_keeps_newest_samples_under_flex_width() {
        use super::{PrepareStrategy, plan_data_preparation, prepare_data};
        use crate::widgets::Overflow;

        let spark = Sparkline::new(0..100u64)
            .width(Length::Flex(1))
            .overflow(Overflow::ClipStart);
        let plan = plan_data_preparation(&spark, Some(10));
        assert_eq!(plan.strategy, PrepareStrategy::Tail);
        assert_eq!(plan.target, Some(10));
        let data = prepare_data(&spark, plan);
        assert_eq!(data, (90..100u64).collect::<Vec<_>>());
    }

    #[test]
    fn head_overflow_keeps_oldest_samples() {
        use super::{PrepareStrategy, plan_data_preparation, prepare_data};
        use crate::widgets::Overflow;

        let spark = Sparkline::new(0..100u64)
            .width(Length::Flex(1))
            .overflow(Overflow::Ellipsis);
        let plan = plan_data_preparation(&spark, Some(8));
        assert_eq!(plan.strategy, PrepareStrategy::Head);
        let data = prepare_data(&spark, plan);
        assert_eq!(data, (0..8u64).collect::<Vec<_>>());
    }

    #[test]
    fn wrap_overflow_downsamples_full_buffer() {
        use super::{PrepareStrategy, plan_data_preparation, prepare_data};
        use crate::widgets::Overflow;

        let spark = Sparkline::new(0..100u64)
            .width(Length::Flex(1))
            .overflow(Overflow::Wrap);
        let plan = plan_data_preparation(&spark, Some(10));
        assert_eq!(plan.strategy, PrepareStrategy::Downsample);
        let data = prepare_data(&spark, plan);
        assert_eq!(data.len(), 10);
        // First bucket averages 0..10, last bucket averages 90..99.
        assert!(data[0] < 10);
        assert!(data[9] > 80);
    }

    #[test]
    fn explicit_max_points_still_downsamples_under_flex() {
        use super::{PrepareStrategy, plan_data_preparation};
        use crate::widgets::Overflow;

        let spark = Sparkline::new(0..100u64)
            .width(Length::Flex(1))
            .overflow(Overflow::ClipStart)
            .max_points(20);
        let plan = plan_data_preparation(&spark, Some(40));
        assert_eq!(plan.strategy, PrepareStrategy::Downsample);
        assert_eq!(plan.target, Some(20));
    }

    #[test]
    fn braille_target_doubles_for_pair_packing() {
        use super::plan_data_preparation;

        let spark = Sparkline::new(0..100u64)
            .width(Length::Flex(1))
            .variant(super::SparklineVariant::Braille);
        let plan = plan_data_preparation(&spark, Some(10));
        assert_eq!(plan.target, Some(20));
    }

    #[test]
    fn changed_render_key_invalidates_output() {
        let mut node = super::SparklineNode::default();

        let first = Sparkline::new([1, 2, 3, 4]).chart_height(2);
        assert!(reconcile_sparkline(&first, &mut node, None));
        let first_output = node.output.clone();

        let second = Sparkline::new([1, 3, 2, 4]).chart_height(2);
        assert!(reconcile_sparkline(&second, &mut node, None));
        assert!(!Arc::ptr_eq(&first_output, &node.output));
    }
}
