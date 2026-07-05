//! Heatmap widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_heatmap;
pub use node::HeatmapNode;
pub(crate) use reconcile::reconcile_heatmap_node;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::layout::hash::LayoutHash;
use crate::style::{BorderStyle, Color, Length, Padding, Style};
use crate::utils::gradient::ColorGradient;
use unicode_width::UnicodeWidthStr;

/// Rendering strategy for heatmap cells.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeatmapCellMode {
    /// Render each cell as a colored background block.
    #[default]
    Background,
    /// Render each cell with a centered glyph string over the colored background.
    Glyph(Arc<str>),
    /// Render only the glyph in the mapped color, leaving the cell background untouched.
    GlyphForeground(Arc<str>),
}

/// Horizontal layout mode for the heatmap legend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeatmapLegendWidth {
    /// Start the legend at the same x-position as the heatmap grid.
    #[default]
    Grid,
    /// Let the legend span the full inner width, ignoring the row-label gutter.
    Full,
}

/// A heatmap widget that visualizes a 2D matrix of values as colored cells.
#[derive(Clone)]
pub struct Heatmap {
    pub(crate) data: Vec<Vec<f64>>,
    pub(crate) row_labels: Vec<Arc<str>>,
    pub(crate) column_labels: Vec<Arc<str>>,
    pub(crate) gradient: ColorGradient,
    pub(crate) range_min: Option<f64>,
    pub(crate) range_max: Option<f64>,
    pub(crate) cell_mode: HeatmapCellMode,
    pub(crate) cell_width: u16,
    pub(crate) gap_x: u16,
    pub(crate) gap_y: u16,
    pub(crate) legend_gap: u16,
    pub(crate) legend_spacing: u16,
    pub(crate) legend_width: HeatmapLegendWidth,
    pub(crate) show_values: bool,
    pub(crate) show_legend: bool,
    pub(crate) style: Style,
    pub(crate) label_style: Style,
    pub(crate) legend_style: Style,
    pub(crate) padding: Padding,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for Heatmap {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            row_labels: Vec::new(),
            column_labels: Vec::new(),
            gradient: ColorGradient::new(Color::Rgb(60, 179, 113), Color::Rgb(226, 82, 87)),
            range_min: None,
            range_max: None,
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
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl Heatmap {
    /// Create a new heatmap from a 2D matrix of values.
    pub fn new(data: impl IntoIterator<Item = impl IntoIterator<Item = f64>>) -> Self {
        Self {
            data: data
                .into_iter()
                .map(|row| row.into_iter().collect())
                .collect(),
            ..Self::default()
        }
    }

    /// Set row labels displayed on the left.
    pub fn row_labels(mut self, labels: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.row_labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Set column labels displayed on top.
    pub fn column_labels(mut self, labels: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.column_labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Set the color gradient for mapping values to colors.
    pub fn gradient(mut self, gradient: ColorGradient) -> Self {
        self.gradient = gradient;
        self
    }

    /// Set the explicit value range for gradient mapping.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.range_min = Some(min);
        self.range_max = Some(max);
        self
    }

    /// Set how cells are rendered.
    pub fn cell_mode(mut self, cell_mode: HeatmapCellMode) -> Self {
        self.cell_mode = cell_mode;
        self
    }

    /// Set the width of each cell in characters.
    pub fn cell_width(mut self, cell_width: u16) -> Self {
        self.cell_width = cell_width.max(1);
        self
    }

    /// Set horizontal spacing between cells in characters.
    pub fn gap_x(mut self, gap_x: u16) -> Self {
        self.gap_x = gap_x;
        self
    }

    /// Set vertical spacing between rows in lines.
    pub fn gap_y(mut self, gap_y: u16) -> Self {
        self.gap_y = gap_y;
        self
    }

    /// Set spacing between legend color cells in characters.
    pub fn legend_gap(mut self, legend_gap: u16) -> Self {
        self.legend_gap = legend_gap;
        self
    }

    /// Set vertical spacing between the heatmap grid and legend in lines.
    pub fn legend_spacing(mut self, legend_spacing: u16) -> Self {
        self.legend_spacing = legend_spacing;
        self
    }

    /// Set whether the legend aligns with the grid or spans the full inner width.
    pub fn legend_width(mut self, legend_width: HeatmapLegendWidth) -> Self {
        self.legend_width = legend_width;
        self
    }

    /// Show numeric values inside cells.
    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    /// Show a gradient legend below the heatmap.
    pub fn show_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    /// Set the base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the label style for row/column labels.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set the legend style.
    pub fn legend_style(mut self, style: Style) -> Self {
        self.legend_style = style;
        self
    }

    /// Set inner padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Enable or disable border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub(crate) fn effective_cell_width(&self) -> u16 {
        let min_width = match &self.cell_mode {
            HeatmapCellMode::Background => 1,
            HeatmapCellMode::Glyph(glyph) | HeatmapCellMode::GlyphForeground(glyph) => {
                UnicodeWidthStr::width(glyph.as_ref())
                    .max(1)
                    .min(u16::MAX as usize) as u16
            }
        };

        self.cell_width.max(min_width)
    }
}

impl From<Heatmap> for Element {
    fn from(value: Heatmap) -> Self {
        Element::new(ElementKind::Heatmap(value))
    }
}

impl LayoutHash for Heatmap {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;

        self.width.hash(hasher);
        self.height.hash(hasher);
        self.row_labels.hash(hasher);
        self.column_labels.hash(hasher);
        self.cell_mode.hash(hasher);
        self.cell_width.hash(hasher);
        self.gap_x.hash(hasher);
        self.gap_y.hash(hasher);
        self.legend_gap.hash(hasher);
        self.legend_spacing.hash(hasher);
        self.legend_width.hash(hasher);
        self.show_legend.hash(hasher);
        self.padding.hash(hasher);
        self.border.hash(hasher);
        self.data.len().hash(hasher);
        for row in &self.data {
            row.len().hash(hasher);
        }

        Some(())
    }
}
