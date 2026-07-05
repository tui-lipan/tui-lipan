use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Length, Padding, Style};

use super::{Chart, ChartAxis, ChartSeries, ChartSeriesMode, ChartThreshold};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChartCacheKey {
    pub(crate) hash: u64,
}

impl ChartCacheKey {
    pub(crate) fn new(chart: &Chart) -> Self {
        Self {
            hash: render_hash(chart),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChartWidgetKey {
    pub(crate) hash: u64,
}

impl ChartWidgetKey {
    pub(crate) fn new(chart: &Chart) -> Self {
        Self {
            hash: widget_hash(chart),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChartSeriesRender {
    pub(crate) name: Arc<str>,
    pub(crate) mode: ChartSeriesMode,
    pub(crate) points: Vec<(f64, f64)>,
    pub(crate) style: Style,
    pub(crate) point_char: char,
    pub(crate) line_char: char,
    pub(crate) bar_char: char,
}

#[derive(Clone, Debug)]
pub(crate) struct ChartThresholdRender {
    pub(crate) y_norm: f64,
    pub(crate) value: f64,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) style: Style,
    pub(crate) glyph: char,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ChartRenderOutput {
    pub(crate) series: Vec<ChartSeriesRender>,
    pub(crate) thresholds: Vec<ChartThresholdRender>,
    pub(crate) y_min: f64,
    pub(crate) y_max: f64,
    pub(crate) sample_count: usize,
}

#[derive(Clone)]
pub struct ChartNode {
    pub series: Arc<[ChartSeries]>,
    pub thresholds: Arc<[ChartThreshold]>,
    pub x_axis: ChartAxis,
    pub y_axis: ChartAxis,
    pub style: Style,
    pub axis_style: Style,
    pub grid_style: Style,
    pub legend_style: Style,
    pub show_grid: bool,
    pub show_legend: bool,
    pub legend_separator: Arc<str>,
    pub viewport_start: usize,
    pub viewport_len: Option<usize>,
    pub padding: Padding,
    pub border: bool,
    pub border_style: BorderStyle,
    pub width: Length,
    pub height: Length,
    pub(crate) output: Arc<ChartRenderOutput>,
    pub(crate) cache_key: ChartCacheKey,
    pub(crate) widget_key: ChartWidgetKey,
}

impl Default for ChartNode {
    fn default() -> Self {
        Self {
            series: Arc::new([]),
            thresholds: Arc::new([]),
            x_axis: ChartAxis::default(),
            y_axis: ChartAxis::default(),
            style: Style::default(),
            axis_style: Style::default(),
            grid_style: Style::default(),
            legend_style: Style::default(),
            show_grid: true,
            show_legend: true,
            legend_separator: Arc::from("  "),
            viewport_start: 0,
            viewport_len: None,
            padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            width: Length::Flex(1),
            height: Length::Px(10),
            output: Arc::new(ChartRenderOutput::default()),
            cache_key: ChartCacheKey { hash: 0 },
            widget_key: ChartWidgetKey { hash: 0 },
        }
    }
}

impl From<Chart> for ChartNode {
    fn from(value: Chart) -> Self {
        let mut node = Self::default();
        super::reconcile_chart(&value, &mut node);
        node
    }
}

impl WidgetNode for ChartNode {}

fn render_hash(chart: &Chart) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_series_data(&chart.series, &mut hasher);
    hash_threshold_data(&chart.thresholds, &mut hasher);
    hash_axis_range(&chart.x_axis, &mut hasher);
    hash_axis_range(&chart.y_axis, &mut hasher);
    chart.viewport_start.hash(&mut hasher);
    chart.viewport_len.hash(&mut hasher);
    hasher.finish()
}

fn widget_hash(chart: &Chart) -> u64 {
    let mut hasher = DefaultHasher::new();
    render_hash(chart).hash(&mut hasher);
    hash_axis_style(&chart.x_axis, &mut hasher);
    hash_axis_style(&chart.y_axis, &mut hasher);
    chart.style.hash(&mut hasher);
    chart.axis_style.hash(&mut hasher);
    chart.grid_style.hash(&mut hasher);
    chart.legend_style.hash(&mut hasher);
    chart.show_grid.hash(&mut hasher);
    chart.show_legend.hash(&mut hasher);
    chart.legend_separator.hash(&mut hasher);
    chart.padding.hash(&mut hasher);
    chart.border.hash(&mut hasher);
    chart.border_style.hash(&mut hasher);
    chart.width.hash(&mut hasher);
    chart.height.hash(&mut hasher);
    hasher.finish()
}

fn hash_series_data(series: &[ChartSeries], hasher: &mut impl Hasher) {
    series.len().hash(hasher);
    for s in series {
        s.name.hash(hasher);
        s.mode.hash(hasher);
        s.point_char.hash(hasher);
        s.line_char.hash(hasher);
        s.bar_char.hash(hasher);
        for value in s.data.iter() {
            value.to_bits().hash(hasher);
        }
    }
}

fn hash_threshold_data(thresholds: &[ChartThreshold], hasher: &mut impl Hasher) {
    thresholds.len().hash(hasher);
    for threshold in thresholds {
        threshold.value.to_bits().hash(hasher);
        threshold.glyph.hash(hasher);
    }
}

fn hash_axis_range(axis: &ChartAxis, hasher: &mut impl Hasher) {
    axis.show.hash(hasher);
    axis.ticks.hash(hasher);
    axis.min.map(f64::to_bits).hash(hasher);
    axis.max.map(f64::to_bits).hash(hasher);
}

fn hash_axis_style(axis: &ChartAxis, hasher: &mut impl Hasher) {
    axis.label.hash(hasher);
    axis.style.hash(hasher);
}
