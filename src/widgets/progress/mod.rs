//! ProgressBar widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_progress_bar;
pub use node::ProgressNode;
pub use reconcile::reconcile_progress_bar;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::style::{Length, Padding, Style, StyleSlot};
use crate::utils::gradient::ColorGradient;

/// Event emitted when progress bar value changes (via drag).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressEvent {
    /// New progress value (0.0 to 1.0).
    pub progress: f64,
}

/// Threshold styling zone for progress bar fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressZone {
    /// Upper bound for this zone in the [0.0, 1.0] range.
    pub upto: f64,
    /// Optional style patch for this zone.
    pub style: Style,
    /// Optional symbol override for this zone.
    pub symbol: Option<char>,
}

impl ProgressZone {
    /// Create a zone that applies up to the given normalized value.
    pub fn new(upto: f64) -> Self {
        Self {
            upto: upto.clamp(0.0, 1.0),
            style: Style::default(),
            symbol: None,
        }
    }

    /// Set style for this zone.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set symbol override for this zone.
    pub fn symbol(mut self, symbol: char) -> Self {
        self.symbol = Some(symbol);
        self
    }
}

/// Visual style for a [`ProgressBar`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProgressStyle {
    /// Block style using `█` and `░`.
    #[default]
    Block,
    /// Line style using `─` and `━`.
    Line,
    /// Line style with dots: `━` and `┄`.
    LineDotted,
    /// Dots style using `●` and `○`.
    Dots,
    /// Arrow style using `►` and `─`.
    Arrow,
    /// Rect style using `▮` and `▯`.
    Rect,
    /// Custom style with user-defined characters.
    Custom {
        /// Character for the filled portion.
        filled: char,
        /// Character for the empty portion.
        empty: char,
    },
    /// Braille pattern for smooth animation.
    Braille,
}

/// Text placement for [`ProgressBar`] percentage and label text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ProgressTextPosition {
    /// Render text before the bar track.
    Left,
    /// Render text after the bar track.
    #[default]
    Right,
    /// Render text above the bar track.
    Above,
    /// Render text below the bar track.
    Below,
    /// Render text centered inside the bar track (Block style only).
    Middle,
}

impl ProgressStyle {
    /// Get the filled character for this style.
    pub fn filled_char(self) -> char {
        match self {
            Self::Block => '█',
            Self::Line | Self::LineDotted => '━',
            Self::Dots => '●',
            Self::Arrow => '►',
            Self::Rect => '▮',
            Self::Custom { filled, .. } => filled,
            Self::Braille => '⣿',
        }
    }

    /// Get the empty character for this style.
    pub fn empty_char(self) -> char {
        match self {
            Self::Block => '░',
            Self::Line => '─',
            Self::LineDotted => '┄',
            Self::Dots => '○',
            Self::Arrow => '─',
            Self::Rect => '▯',
            Self::Custom { empty, .. } => empty,
            Self::Braille => '⣀',
        }
    }

    /// Get the partial fill characters for smooth rendering (if available).
    pub fn partial_chars(self) -> Option<&'static [char]> {
        match self {
            // Disabled partials for Block to avoid "dark block" artifacts against shade char.
            // Self::Block => Some(&['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█']),
            Self::Braille => Some(&['⣀', '⣄', '⣤', '⣦', '⣶', '⣷', '⣿']),
            _ => None,
        }
    }
}

/// A progress bar widget.
#[derive(Clone)]
pub struct ProgressBar {
    /// Progress value (0.0 to 1.0).
    pub progress: f64,
    /// Visual style.
    pub progress_style: ProgressStyle,
    /// Whether to show the percentage text.
    pub show_percentage: bool,
    /// Placement for percentage text.
    pub percentage_position: ProgressTextPosition,
    /// Custom label to show in addition to percentage text.
    pub label: Option<String>,
    /// Placement for custom label text.
    pub label_position: ProgressTextPosition,
    /// Style for the filled portion.
    pub filled_style: Style,
    /// Optional gradient for the filled portion (left -> right).
    pub filled_gradient: Option<ColorGradient>,
    /// Style for the empty portion.
    pub empty_style: Style,
    /// Style for the percentage/label text.
    pub label_style: Style,
    /// Base style.
    pub style: Style,
    /// Padding.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Whether the progress bar is draggable.
    pub draggable: bool,
    /// Drag step size (e.g. 0.1). If set, drag values snap to this increment.
    pub step: Option<f64>,
    /// Whether to invert the progress bar (fill from right to left).
    pub inverted: bool,
    /// Callback fired when progress changes (via drag).
    pub on_change: Option<Callback<ProgressEvent>>,
    /// Mouse click handler.
    pub on_click: Option<Callback<MouseEvent>>,
    /// Whether the progress bar is focusable.
    pub focusable: bool,
    /// Style when focused (applied to filled portion).
    pub focus_style: StyleSlot,
    /// Style when hovered.
    pub hover_style: StyleSlot,
    /// Optional target marker position in the [0.0, 1.0] range.
    pub target: Option<f64>,
    /// Style for the target marker.
    pub target_style: Style,
    /// Symbol used for the target marker.
    pub target_symbol: char,
    /// Threshold zones for filled segment styling.
    pub zones: Vec<ProgressZone>,
    /// Block-mode dim amount for empty track background in `[0.0, 1.0]`.
    pub block_empty_bg_dim: f32,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            progress: 0.0,
            progress_style: ProgressStyle::Block,
            show_percentage: false,
            percentage_position: ProgressTextPosition::Right,
            label: None,
            label_position: ProgressTextPosition::Right,
            filled_style: Style::default(),
            filled_gradient: None,
            empty_style: Style::default(),
            label_style: Style::default(),
            style: Style::default(),
            padding: Padding::default(),
            width: Length::Flex(1),
            height: Length::Auto,
            draggable: false,
            step: None,
            inverted: false,
            on_change: None,
            on_click: None,
            focusable: false,
            focus_style: StyleSlot::Inherit,
            hover_style: StyleSlot::Inherit,
            target: None,
            target_style: Style::default(),
            target_symbol: '◆',
            zones: Vec::new(),
            block_empty_bg_dim: 0.85,
        }
    }
}

impl ProgressBar {
    /// Create a new progress bar with the given progress (0.0 to 1.0).
    pub fn new(progress: f64) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            ..Self::default()
        }
    }

    /// Set the progress value (0.0 to 1.0).
    pub fn progress(mut self, progress: f64) -> Self {
        self.progress = progress.clamp(0.0, 1.0);
        self
    }

    /// Set whether to invert the progress bar (fill from right to left).
    pub fn inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    /// Set the visual style.
    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.progress_style = style;
        self
    }

    /// Show percentage text.
    pub fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Set percentage text placement.
    pub fn percentage_position(mut self, position: ProgressTextPosition) -> Self {
        self.percentage_position = position;
        self
    }

    /// Set custom label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set custom label placement.
    pub fn label_position(mut self, position: ProgressTextPosition) -> Self {
        self.label_position = position;
        self
    }

    /// Set style for the filled portion.
    pub fn filled_style(mut self, style: Style) -> Self {
        self.filled_style = style;
        self
    }

    /// Set gradient for the filled portion.
    pub fn filled_gradient(mut self, gradient: ColorGradient) -> Self {
        self.filled_gradient = Some(gradient);
        self
    }

    /// Set style for the empty portion.
    pub fn empty_style(mut self, style: Style) -> Self {
        self.empty_style = style;
        self
    }

    /// Set style for the label text.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
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

    /// Make the progress bar draggable.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    /// Set drag step size.
    pub fn step(mut self, step: f64) -> Self {
        self.step = Some(step);
        self
    }

    /// Set callback for progress changes (via drag).
    pub fn on_change(mut self, cb: Callback<ProgressEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set mouse click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set whether the progress bar is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set style when focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focus style with the given style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set the focus style slot directly.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set style when hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style with the given style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set the hover style slot directly.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set optional target marker position in the [0.0, 1.0] range.
    pub fn target(mut self, target: f64) -> Self {
        self.target = Some(target.clamp(0.0, 1.0));
        self
    }

    /// Clear target marker.
    pub fn clear_target(mut self) -> Self {
        self.target = None;
        self
    }

    /// Set style for target marker.
    pub fn target_style(mut self, style: Style) -> Self {
        self.target_style = style;
        self
    }

    /// Set symbol for target marker.
    pub fn target_symbol(mut self, symbol: char) -> Self {
        self.target_symbol = symbol;
        self
    }

    /// Replace threshold zones.
    pub fn zones(mut self, zones: impl IntoIterator<Item = ProgressZone>) -> Self {
        self.zones = zones.into_iter().collect();
        self
    }

    /// Add one threshold zone.
    pub fn add_zone(mut self, zone: ProgressZone) -> Self {
        self.zones.push(zone);
        self
    }

    /// Set empty-track background dim amount for `ProgressStyle::Block`.
    pub fn block_empty_bg_dim(mut self, amount: f32) -> Self {
        self.block_empty_bg_dim = amount.clamp(0.0, 1.0);
        self
    }
}

impl From<ProgressBar> for Element {
    fn from(value: ProgressBar) -> Self {
        Element::new(ElementKind::ProgressBar(value))
    }
}

impl crate::layout::hash::LayoutHash for ProgressBar {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.show_percentage.hash(hasher);
        self.percentage_position.hash(hasher);
        self.label_position.hash(hasher);
        self.padding.hash(hasher);
        self.label.hash(hasher);
        Some(())
    }
}
