/// Scroll axes enabled on a scroll container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollAxis {
    /// Vertical scrolling only (default).
    #[default]
    Vertical,
    /// Horizontal scrolling only.
    Horizontal,
    /// Both vertical and horizontal scrolling.
    Both,
}

impl ScrollAxis {
    /// Whether vertical scrolling is enabled for this axis configuration.
    pub const fn vertical_enabled(self) -> bool {
        matches!(self, Self::Vertical | Self::Both)
    }

    /// Whether horizontal scrolling is enabled for this axis configuration.
    pub const fn horizontal_enabled(self) -> bool {
        matches!(self, Self::Horizontal | Self::Both)
    }
}
