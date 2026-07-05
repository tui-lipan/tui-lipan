//! Status bar widget.

use std::sync::Arc;

use crate::core::element::{Element, IntoElement};
use crate::style::{Justify, Length, Padding, Style, Theme};
use crate::widgets::status_bar_layout::StatusBarLayout;
use crate::widgets::{HStack, Spacer, Spinner, SpinnerSpeed, SpinnerStyle, ThemeProvider};

/// A status bar widget.
#[derive(Clone)]
pub struct StatusBar {
    left: Vec<Element>,
    center: Vec<Element>,
    right: Vec<Element>,
    style: Style,
    left_style: Style,
    center_style: Style,
    right_style: Style,
    padding: Padding,
    gap: u16,
    width: Length,
    height: Length,
    reserve_center_space: bool,
    loading: bool,
    loading_label: Arc<str>,
    loading_style: Style,
    loading_spinner_style: SpinnerStyle,
    loading_spinner_speed: SpinnerSpeed,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self {
            left: Vec::new(),
            center: Vec::new(),
            right: Vec::new(),
            style: Style::default(),
            left_style: Style::default(),
            center_style: Style::default(),
            right_style: Style::default(),
            padding: (0, 1).into(),
            gap: 1,
            width: Length::Flex(1),
            height: Length::Px(1),
            reserve_center_space: false,
            loading: false,
            loading_label: "Loading".into(),
            loading_style: Style::default(),
            loading_spinner_style: SpinnerStyle::Dots,
            loading_spinner_speed: SpinnerSpeed::Normal,
        }
    }
}

impl StatusBar {
    /// Create a new status bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a child to the left section (alias for `left`).
    pub fn child(self, item: impl IntoElement) -> Self {
        self.left(item)
    }

    /// Add an item to the left section.
    pub fn left(mut self, item: impl IntoElement) -> Self {
        self.left.push(item.into());
        self
    }

    /// Add an item to the center section.
    pub fn center(mut self, item: impl IntoElement) -> Self {
        self.center.push(item.into());
        self
    }

    /// Add an item to the right section.
    pub fn right(mut self, item: impl IntoElement) -> Self {
        self.right.push(item.into());
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style for the left section.
    pub fn left_style(mut self, style: Style) -> Self {
        self.left_style = style;
        self
    }

    /// Set style for the center section.
    pub fn center_style(mut self, style: Style) -> Self {
        self.center_style = style;
        self
    }

    /// Set style for the right section.
    pub fn right_style(mut self, style: Style) -> Self {
        self.right_style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set gap between items.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Keep a reserved center lane even when no center items are set.
    ///
    /// When `false` (default), the bar collapses to left + right sections if the
    /// center section is empty.
    pub fn reserve_center_space(mut self, reserve: bool) -> Self {
        self.reserve_center_space = reserve;
        self
    }

    /// Show a loading indicator in the status bar.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Set loading label.
    pub fn loading_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.loading_label = label.into();
        self
    }

    /// Set loading style.
    pub fn loading_style(mut self, style: Style) -> Self {
        self.loading_style = style;
        self
    }

    /// Set loading spinner style.
    pub fn loading_spinner_style(mut self, style: SpinnerStyle) -> Self {
        self.loading_spinner_style = style;
        self
    }

    /// Set loading spinner speed.
    pub fn loading_spinner_speed(mut self, speed: SpinnerSpeed) -> Self {
        self.loading_spinner_speed = speed;
        self
    }
}

impl From<StatusBar> for Element {
    fn from(bar: StatusBar) -> Self {
        let left_items = bar.left;
        let center_items = bar.center;
        let mut right_items = bar.right;

        if bar.loading {
            let spinner = Spinner::new()
                .spinner_style(bar.loading_spinner_style)
                .speed(bar.loading_spinner_speed)
                .style(bar.loading_style)
                .label(bar.loading_label)
                .label_style(bar.loading_style);
            right_items.push(spinner.into());
        }

        let center_has_content = !center_items.is_empty();
        let left_has_content = !left_items.is_empty();
        let right_has_content = !right_items.is_empty();

        let left_theme_style = bar.style.patch(bar.left_style);
        let center_theme_style = bar.style.patch(bar.center_style);
        let right_theme_style = bar.style.patch(bar.right_style);

        let content: Element = if center_has_content || bar.reserve_center_space {
            StatusBarLayout {
                left: Box::new(build_section(
                    left_items,
                    bar.gap,
                    Justify::Start,
                    left_theme_style,
                )),
                center: Box::new(build_section(
                    center_items,
                    bar.gap,
                    Justify::Center,
                    center_theme_style,
                )),
                right: Box::new(build_section(
                    right_items,
                    bar.gap,
                    Justify::End,
                    right_theme_style,
                )),
                style: bar.style,
                padding: bar.padding,
                gap: bar.gap,
                width: bar.width,
                height: bar.height,
            }
            .into()
        } else {
            // Two-lane layout: no center reservation when empty.
            let left_section = build_section(left_items, bar.gap, Justify::Start, left_theme_style);
            let right_section =
                build_section(right_items, bar.gap, Justify::End, right_theme_style);

            let base = HStack::new()
                .height(bar.height)
                .width(bar.width)
                .style(bar.style)
                .padding(bar.padding);

            match (left_has_content, right_has_content) {
                (true, true) => base
                    .child(left_section)
                    .child(Spacer::new())
                    .child(right_section)
                    .into(),
                (true, false) => base.child(left_section).into(),
                (false, true) => base.child(Spacer::new()).child(right_section).into(),
                (false, false) => base.into(),
            }
        };

        content
    }
}

fn build_section(items: Vec<Element>, gap: u16, justify: Justify, style: Style) -> Element {
    let mut section = HStack::new()
        .width(Length::Auto)
        .gap(gap)
        .justify(justify)
        .style(style);

    for item in items {
        section = section.child(item);
    }

    let local_theme = Theme {
        primary: style,
        ..Theme::default()
    };

    ThemeProvider::new(local_theme).child(section).into()
}
