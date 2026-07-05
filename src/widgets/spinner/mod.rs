//! Spinner widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_spinner;
pub use node::SpinnerNode;
pub use reconcile::reconcile_spinner;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Style};

/// Animation style for a [`Spinner`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpinnerStyle {
    /// Dot spinner: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
    #[default]
    Dots,
    /// Line spinner: `|/-\`
    Line,
    /// Circle spinner: `◐◓◑◒`
    Circle,
    /// Arc spinner: `◜◠◝◞◡◟`
    Arc,
    /// Braille spinner: `⣾⣽⣻⢿⡿⣟⣯⣷`
    Braille,
    /// Moon spinner: `🌑🌒🌓🌔🌕🌖🌗🌘`
    Moon,
    /// Box spinner: `▖▘▝▗`
    Box,
    /// Vertical bar spinner: ` ▂▃▄▅▆▇█▇▆▅▄▃▂ `
    Bar,
    /// Arrow spinner: `←↖↑↗→↘↓↙`
    Arrow,
    /// Fading pulse spinner: `█▓▒░▒▓█`
    Fade,
    /// Moving trail spinner: `█▓▒░   `
    Trail,
    /// Earth spinner: `🌍🌎🌏`
    Earth,
    /// Claude-style mirrored star spinner: `·✢✳✶✻*✻✶✳✢`
    Claude,
    /// OpenCode spinner: `⬝⬝⬝■■■⬝⬝`
    OpenCode,
    /// Three dot moving: `∙∙∙` -> `●∙∙`
    ThreeDot,
    /// Three dot with trail: `∙∙∙` -> `●∙∙` -> `•●∙`
    ThreeDotFade,
    /// Square gradient: `▱▱▱` -> `▰▱▱`
    SquareFade,
    /// Lightsaber: `⁌==⁍════════════` with ignition and retraction
    Lightsaber,
}

/// Animation speed for a [`Spinner`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpinnerSpeed {
    /// Slow speed (approx 200ms per frame).
    Slow,
    /// Normal speed (approx 100ms per frame).
    #[default]
    Normal,
    /// Fast speed (approx 50ms per frame).
    Fast,
    /// Custom speed, in milliseconds per frame.
    Custom {
        /// Milliseconds to hold each spinner frame.
        frame_ms: u16,
    },
}

impl SpinnerStyle {
    /// Get the animation frames for this style.
    pub fn frames(self) -> &'static [&'static str] {
        match self {
            Self::Dots => &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            Self::Line => &["|", "/", "-", "\\"],
            Self::Circle => &["◐", "◓", "◑", "◒"],
            Self::Arc => &["◜", "◠", "◝", "◞", "◡", "◟"],
            Self::Braille => &["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"],
            Self::Moon => &["🌑", "🌒", "🌓", "🌔", "🌕", "🌖", "🌗", "🌘"],
            Self::Box => &["▖", "▘", "▝", "▗"],
            Self::Bar => &[
                " ", "▂", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃", "▂",
            ],
            Self::Arrow => &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"],
            Self::Fade => &["█", "▓", "▒", "░", "▒", "▓"],
            Self::Trail => &[
                "█    ",
                "▓█   ",
                "▒▓█  ",
                "░▒▓█ ",
                " ░▒▓█",
                "  ░▒▓",
                "   ░▒",
                "    ░",
            ],
            Self::Earth => &["🌍", "🌎", "🌏"],
            Self::Claude => &["·", "✢", "✳", "✶", "✻", "*", "✻", "✶", "✳", "✢"],
            // The frames for OpenCode are handled by a custom renderer with 30-frame cycle,
            // this array is a fallback for frame count queries.
            Self::OpenCode => &[
                "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝",
                "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝", "⬝",
            ],
            Self::ThreeDot => &["∙∙∙", "●∙∙", "∙●∙", "∙∙●"],
            Self::ThreeDotFade => &["∙∙∙", "●∙∙", "•●∙", "∙•●", "∙∙•"],
            Self::SquareFade => &["▱▱▱", "▰▱▱", "▰▰▱", "▰▰▰", "▰▰▱", "▰▱▱", "▱▱▱"],
            Self::Lightsaber => &[
                "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═",
                "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═",
                "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═",
                "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═",
                "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═", "═",
            ],
        }
    }

    /// Get the width of each frame (in cells).
    pub fn width(self) -> u16 {
        match self {
            Self::Moon | Self::Earth => 2,
            Self::Trail => 5,
            Self::OpenCode => 8,
            Self::Lightsaber => 16,
            Self::ThreeDot | Self::ThreeDotFade | Self::SquareFade => 3,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Spinner, SpinnerStyle};

    #[test]
    fn claude_spinner_uses_requested_mirrored_sequence() {
        assert_eq!(
            SpinnerStyle::Claude.frames(),
            &["·", "✢", "✳", "✶", "✻", "*", "✻", "✶", "✳", "✢"],
        );
    }

    #[test]
    fn claude_spinner_tick_wraps_after_one_cycle() {
        let mut spinner = Spinner::new().spinner_style(SpinnerStyle::Claude);
        for _ in 0..SpinnerStyle::Claude.frames().len() {
            spinner.tick();
        }
        assert_eq!(spinner.current_frame(), "·");
    }
}

/// An animated spinner widget.
#[derive(Clone, Debug)]
pub struct Spinner {
    /// Animation style.
    pub spinner_style: SpinnerStyle,
    /// Animation speed.
    pub speed: SpinnerSpeed,
    /// Current animation frame (0-indexed).
    pub frame: Option<usize>,
    /// Optional label displayed next to the spinner.
    pub label: Option<Arc<str>>,
    /// Gap between spinner and label.
    pub gap: u16,
    /// Style for the spinner.
    pub style: Style,
    /// Style for the label.
    pub label_style: Style,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            spinner_style: SpinnerStyle::Dots,
            speed: SpinnerSpeed::Normal,
            frame: None,
            label: None,
            gap: 1,
            style: Style::default(),
            label_style: Style::default(),
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl Spinner {
    /// Create a new spinner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the animation style.
    pub fn spinner_style(mut self, style: SpinnerStyle) -> Self {
        self.spinner_style = style;
        self
    }

    /// Set the animation speed.
    pub fn speed(mut self, speed: SpinnerSpeed) -> Self {
        self.speed = speed;
        self
    }

    /// Set the current animation frame.
    pub fn frame(mut self, frame: usize) -> Self {
        self.frame = Some(frame);
        self
    }

    /// Advance to the next frame (wraps around).
    pub fn tick(&mut self) {
        let frames = self.spinner_style.frames();
        let current = self.frame.unwrap_or(0);
        self.frame = Some((current + 1) % frames.len());
    }

    /// Get the current frame character.
    pub fn current_frame(&self) -> &'static str {
        let frames = self.spinner_style.frames();
        frames[self.frame.unwrap_or(0) % frames.len()]
    }

    /// Set the label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the gap between spinner and label.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set spinner style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set label style.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
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
}

impl From<Spinner> for Element {
    fn from(value: Spinner) -> Self {
        Element::new(ElementKind::Spinner(value))
    }
}

impl crate::layout::hash::LayoutHash for Spinner {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.spinner_style.hash(hasher);
        self.gap.hash(hasher);
        self.label.hash(hasher);
        Some(())
    }
}
