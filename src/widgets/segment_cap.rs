//! Composable segment caps used by widgets such as [`crate::widgets::Badge`].

use crate::app::ContrastPolicy;
use crate::core::element::Element;
use crate::style::{Color, Length, Style};
use crate::widgets::{HStack, Text};

/// Visual treatment used for the two ends of a segmented element.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CapStyle {
    /// Keep the segment padded without drawing cap glyphs.
    #[default]
    Padded,
    /// Use half-block caps; this is safe with ordinary terminal fonts.
    Half,
    /// Use rounded Powerline caps.
    Round,
    /// Use pointed Powerline caps.
    Arrow,
}

impl CapStyle {
    /// Return the `(left, right)` cap glyphs, or `None` for [`CapStyle::Padded`].
    pub const fn glyphs(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Padded => None,
            // The left cap points right and the right cap points left, matching the workbar
            // composition used by hyprmux.
            Self::Half => Some(("\u{2590}", "\u{258c}")),
            Self::Round => Some(("\u{e0b6}", "\u{e0b4}")),
            Self::Arrow => Some(("\u{e0b2}", "\u{e0b0}")),
        }
    }

    /// Return whether this style requires a Powerline/Nerd Font.
    pub const fn requires_nerd_font(self) -> bool {
        matches!(self, Self::Round | Self::Arrow)
    }

    /// Degrade a Nerd Font style to the undecorated padded style.
    ///
    /// [`CapStyle::Half`] remains unchanged when selected explicitly.
    pub const fn font_safe(self) -> Self {
        if self.requires_nerd_font() {
            Self::Padded
        } else {
            self
        }
    }

    /// Return the canonical appearance cycle.
    pub const fn all() -> &'static [Self] {
        &[Self::Padded, Self::Half, Self::Round, Self::Arrow]
    }
}

/// Which sides of a segmented element receive caps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CapSides {
    /// Draw both caps.
    #[default]
    Both,
    /// Draw only the left cap.
    Left,
    /// Draw only the right cap.
    Right,
    /// Draw neither cap.
    None,
}

impl CapSides {
    pub(crate) const fn has_left(self) -> bool {
        matches!(self, Self::Both | Self::Left)
    }

    pub(crate) const fn has_right(self) -> bool {
        matches!(self, Self::Both | Self::Right)
    }
}

/// Compose a child with optional leading and trailing cap glyphs.
pub(crate) fn segment_cap(
    child: Element,
    cap_style: CapStyle,
    cap_sides: CapSides,
    badge_bg: Color,
    cap_behind: Color,
    same_color_left: bool,
) -> Element {
    let glyphs = cap_style.glyphs();
    if glyphs.is_none() && !(same_color_left && cap_sides.has_left()) {
        return child;
    }

    let cap = |glyph: &'static str, same_color: bool| {
        let contrast = if same_color {
            ContrastPolicy::BlackOrWhite
        } else {
            ContrastPolicy::Off
        };
        Text::new(glyph)
            .style(
                Style::new()
                    .fg(badge_bg)
                    .bg(cap_behind)
                    .contrast_policy(contrast),
            )
            .width(Length::Px(1))
            .height(Length::Px(1))
    };

    let mut row = HStack::new().width(Length::Auto).height(Length::Auto);
    if cap_sides.has_left() {
        if same_color_left {
            row = row.child(cap(same_color_separator(cap_style), true));
        } else if let Some((left, _)) = glyphs {
            row = row.child(cap(left, false));
        }
    }
    row = row.child(child);
    if cap_sides.has_right()
        && let Some((_, right)) = glyphs
    {
        row = row.child(cap(right, false));
    }
    row.into()
}

/// Keep equal-color neighbors distinct without changing the seam width.
const fn same_color_separator(cap_style: CapStyle) -> &'static str {
    match cap_style {
        CapStyle::Arrow => "\u{e0b3}",
        CapStyle::Padded | CapStyle::Half | CapStyle::Round => "▏",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_style_glyphs_match_workbar_cap_sets() {
        assert_eq!(CapStyle::Padded.glyphs(), None);
        assert_eq!(CapStyle::Half.glyphs(), Some(("\u{2590}", "\u{258c}")));
        assert_eq!(CapStyle::Round.glyphs(), Some(("\u{e0b6}", "\u{e0b4}")));
        assert_eq!(CapStyle::Arrow.glyphs(), Some(("\u{e0b2}", "\u{e0b0}")));
    }

    #[test]
    fn nerd_font_detection_and_safe_degradation_are_exact() {
        assert!(!CapStyle::Padded.requires_nerd_font());
        assert!(!CapStyle::Half.requires_nerd_font());
        assert!(CapStyle::Round.requires_nerd_font());
        assert!(CapStyle::Arrow.requires_nerd_font());
        assert_eq!(CapStyle::Padded.font_safe(), CapStyle::Padded);
        assert_eq!(CapStyle::Half.font_safe(), CapStyle::Half);
        assert_eq!(CapStyle::Round.font_safe(), CapStyle::Padded);
        assert_eq!(CapStyle::Arrow.font_safe(), CapStyle::Padded);
    }

    #[test]
    fn all_returns_the_canonical_cap_style_cycle() {
        assert_eq!(
            CapStyle::all(),
            &[
                CapStyle::Padded,
                CapStyle::Half,
                CapStyle::Round,
                CapStyle::Arrow
            ]
        );
    }

    #[test]
    fn cap_sides_have_only_the_requested_edges() {
        assert!(CapSides::Both.has_left());
        assert!(CapSides::Both.has_right());
        assert!(CapSides::Left.has_left());
        assert!(!CapSides::Left.has_right());
        assert!(!CapSides::Right.has_left());
        assert!(CapSides::Right.has_right());
        assert!(!CapSides::None.has_left());
        assert!(!CapSides::None.has_right());
    }

    #[test]
    fn same_color_separators_match_hyprmux_workbar_behavior() {
        assert_eq!(same_color_separator(CapStyle::Arrow), "\u{e0b3}");
        assert_eq!(same_color_separator(CapStyle::Round), "▏");
        assert_eq!(same_color_separator(CapStyle::Half), "▏");
        assert_eq!(same_color_separator(CapStyle::Padded), "▏");
    }
}
