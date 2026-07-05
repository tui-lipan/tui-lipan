use std::sync::Arc;

use super::node::{
    ChartCacheKey, ChartNode, ChartRenderOutput, ChartSeriesRender, ChartThresholdRender,
    ChartWidgetKey,
};
use super::{Chart, ChartSeries};

pub fn reconcile_chart(chart: &Chart, node: &mut ChartNode) -> bool {
    let cache_key = ChartCacheKey::new(chart);
    let widget_key = ChartWidgetKey::new(chart);
    if widget_key == node.widget_key {
        return false;
    }

    let output = if cache_key == node.cache_key {
        node.output.clone()
    } else {
        Arc::new(build_chart_output(chart))
    };

    node.series = chart.series.clone();
    node.thresholds = chart.thresholds.clone();
    node.x_axis = chart.x_axis.clone();
    node.y_axis = chart.y_axis.clone();
    node.style = chart.style;
    node.axis_style = chart.axis_style;
    node.grid_style = chart.grid_style;
    node.legend_style = chart.legend_style;
    node.show_grid = chart.show_grid;
    node.show_legend = chart.show_legend;
    node.legend_separator = chart.legend_separator.clone();
    node.viewport_start = chart.viewport_start;
    node.viewport_len = chart.viewport_len;
    node.padding = chart.padding;
    node.border = chart.border;
    node.border_style = chart.border_style;
    node.width = chart.width;
    node.height = chart.height;
    node.output = output;
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}

fn build_chart_output(chart: &Chart) -> ChartRenderOutput {
    let (y_min, y_max) = resolve_y_bounds(chart);
    let mut series_out = Vec::with_capacity(chart.series.len());
    let mut max_samples = 0usize;

    for series in chart.series.iter() {
        let visible = visible_series_slice(series, chart.viewport_start, chart.viewport_len);
        max_samples = max_samples.max(visible.len());
        let points = project_series_points(visible, y_min, y_max);
        series_out.push(ChartSeriesRender {
            name: series.name.clone(),
            mode: series.mode,
            points,
            style: series.style,
            point_char: series.point_char,
            line_char: series.line_char,
            bar_char: series.bar_char,
        });
    }

    let mut thresholds = Vec::with_capacity(chart.thresholds.len());
    for threshold in chart.thresholds.iter() {
        let norm = normalize_value(threshold.value, y_min, y_max);
        thresholds.push(ChartThresholdRender {
            y_norm: norm,
            value: threshold.value,
            label: threshold.label.clone(),
            style: threshold.style,
            glyph: threshold.glyph,
        });
    }

    ChartRenderOutput {
        series: series_out,
        thresholds,
        y_min,
        y_max,
        sample_count: max_samples,
    }
}

fn resolve_y_bounds(chart: &Chart) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;

    for series in chart.series.iter() {
        for value in visible_series_slice(series, chart.viewport_start, chart.viewport_len) {
            min = min.min(*value);
            max = max.max(*value);
        }
    }

    if let Some(explicit_min) = chart.y_axis.min {
        min = explicit_min;
    }
    if let Some(explicit_max) = chart.y_axis.max {
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

fn visible_series_slice(
    series: &ChartSeries,
    viewport_start: usize,
    viewport_len: Option<usize>,
) -> &[f64] {
    let len = series.data.len();
    if viewport_start >= len {
        return &[];
    }
    let end = viewport_len
        .map(|window| viewport_start.saturating_add(window))
        .unwrap_or(len)
        .min(len);
    &series.data[viewport_start..end]
}

fn project_series_points(values: &[f64], y_min: f64, y_max: f64) -> Vec<(f64, f64)> {
    if values.is_empty() {
        return Vec::new();
    }

    let width = values.len().saturating_sub(1).max(1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            let x = idx as f64 / width;
            let y = normalize_value(*value, y_min, y_max);
            (x, y)
        })
        .collect()
}

fn normalize_value(value: f64, min: f64, max: f64) -> f64 {
    if (max - min).abs() < f64::EPSILON {
        0.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}
