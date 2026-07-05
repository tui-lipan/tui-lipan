//! Chart widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_chart;
pub use node::ChartNode;
pub use reconcile::reconcile_chart;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};

/// Rendering mode for a chart series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ChartSeriesMode {
    /// Draw connected trend points.
    #[default]
    Line,
    /// Draw vertical bars.
    Bars,
}

/// Single data series rendered on a chart.
#[derive(Clone, Debug)]
pub struct ChartSeries {
    pub(crate) name: Arc<str>,
    pub(crate) data: Arc<[f64]>,
    pub(crate) mode: ChartSeriesMode,
    pub(crate) style: Style,
    pub(crate) point_char: char,
    pub(crate) line_char: char,
    pub(crate) bar_char: char,
}

impl ChartSeries {
    /// Create a line series with a display name and numeric samples.
    pub fn new(name: impl Into<Arc<str>>, data: impl IntoIterator<Item = f64>) -> Self {
        Self {
            name: name.into(),
            data: data.into_iter().collect::<Vec<_>>().into(),
            mode: ChartSeriesMode::Line,
            style: Style::default(),
            point_char: '●',
            line_char: '─',
            bar_char: '█',
        }
    }

    /// Set series rendering mode.
    pub fn mode(mut self, mode: ChartSeriesMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set style for this series.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Override the point glyph used for line mode.
    pub fn point_char(mut self, point_char: char) -> Self {
        self.point_char = point_char;
        self
    }

    /// Override the connector glyph used for line mode.
    pub fn line_char(mut self, line_char: char) -> Self {
        self.line_char = line_char;
        self
    }

    /// Override the bar glyph used for bar mode.
    pub fn bar_char(mut self, bar_char: char) -> Self {
        self.bar_char = bar_char;
        self
    }
}

/// Axis configuration.
#[derive(Clone, Debug)]
pub struct ChartAxis {
    pub(crate) show: bool,
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
    pub(crate) ticks: u16,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) style: Style,
}

impl Default for ChartAxis {
    fn default() -> Self {
        Self {
            show: true,
            min: None,
            max: None,
            ticks: 4,
            label: None,
            style: Style::default(),
        }
    }
}

impl ChartAxis {
    /// Create default axis configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle axis visibility.
    pub fn show(mut self, show: bool) -> Self {
        self.show = show;
        self
    }

    /// Set explicit numeric range.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Set preferred tick count.
    pub fn ticks(mut self, ticks: u16) -> Self {
        self.ticks = ticks.max(2);
        self
    }

    /// Set optional axis label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set axis style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// Horizontal threshold reference line.
#[derive(Clone, Debug)]
pub struct ChartThreshold {
    pub(crate) value: f64,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) style: Style,
    pub(crate) glyph: char,
}

impl ChartThreshold {
    /// Create a new threshold line at a numeric value.
    pub fn new(value: f64) -> Self {
        Self {
            value,
            label: None,
            style: Style::default(),
            glyph: '┈',
        }
    }

    /// Set threshold label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set threshold style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set glyph used for the threshold line.
    pub fn glyph(mut self, glyph: char) -> Self {
        self.glyph = glyph;
        self
    }
}

/// Multi-series chart with axes, grid, legend, and thresholds.
#[derive(Clone)]
pub struct Chart {
    pub(crate) series: Arc<[ChartSeries]>,
    pub(crate) thresholds: Arc<[ChartThreshold]>,
    pub(crate) x_axis: ChartAxis,
    pub(crate) y_axis: ChartAxis,
    pub(crate) style: Style,
    pub(crate) axis_style: Style,
    pub(crate) grid_style: Style,
    pub(crate) legend_style: Style,
    pub(crate) show_grid: bool,
    pub(crate) show_legend: bool,
    pub(crate) legend_separator: Arc<str>,
    pub(crate) viewport_start: usize,
    pub(crate) viewport_len: Option<usize>,
    /// Padding inside the chart frame.
    /// Default: `Padding::default()`.
    pub(crate) padding: Padding,
    pub(crate) border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub(crate) border_style: BorderStyle,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub(crate) width: Length,
    /// Requested height.
    /// Default: `Length::Px(10)`.
    pub(crate) height: Length,
}

impl Default for Chart {
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
        }
    }
}

impl Chart {
    /// Create an empty chart.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace all chart series.
    pub fn series(mut self, series: impl IntoIterator<Item = ChartSeries>) -> Self {
        self.series = series.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Add one chart series.
    pub fn add_series(mut self, series: ChartSeries) -> Self {
        let mut next = self.series.to_vec();
        next.push(series);
        self.series = next.into();
        self
    }

    /// Replace threshold definitions.
    pub fn thresholds(mut self, thresholds: impl IntoIterator<Item = ChartThreshold>) -> Self {
        self.thresholds = thresholds.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Set X axis config.
    pub fn x_axis(mut self, axis: ChartAxis) -> Self {
        self.x_axis = axis;
        self
    }

    /// Set Y axis config.
    pub fn y_axis(mut self, axis: ChartAxis) -> Self {
        self.y_axis = axis;
        self
    }

    /// Set base chart style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set axis style.
    pub fn axis_style(mut self, style: Style) -> Self {
        self.axis_style = style;
        self
    }

    /// Set grid style.
    pub fn grid_style(mut self, style: Style) -> Self {
        self.grid_style = style;
        self
    }

    /// Set legend style.
    pub fn legend_style(mut self, style: Style) -> Self {
        self.legend_style = style;
        self
    }

    /// Toggle plot grid rendering.
    pub fn show_grid(mut self, show_grid: bool) -> Self {
        self.show_grid = show_grid;
        self
    }

    /// Toggle legend rendering.
    pub fn show_legend(mut self, show_legend: bool) -> Self {
        self.show_legend = show_legend;
        self
    }

    /// Set separator between legend items.
    pub fn legend_separator(mut self, legend_separator: impl Into<Arc<str>>) -> Self {
        self.legend_separator = legend_separator.into();
        self
    }

    /// Set viewport start index.
    pub fn viewport_start(mut self, viewport_start: usize) -> Self {
        self.viewport_start = viewport_start;
        self
    }

    /// Set optional viewport sample length.
    pub fn viewport_len(mut self, viewport_len: Option<usize>) -> Self {
        self.viewport_len = viewport_len;
        self
    }

    /// Set chart padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Enable or disable chart border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set requested chart width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested chart height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<Chart> for Element {
    fn from(value: Chart) -> Self {
        Element::new(ElementKind::Chart(Box::new(value)))
    }
}
