//! Composable segment caps used by widgets such as [`crate::widgets::Badge`].

use unicode_width::UnicodeWidthChar;

use crate::app::ContrastPolicy;
use crate::core::element::Element;
use crate::style::{Color, Length, Style};
use crate::widgets::{HStack, Text};

/// Visual treatment used for the two ends of a segmented element.
///
/// New named styles may be added in a minor-compatible release, so a `match` needs a wildcard arm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
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
    /// Use slanted triangle caps (`◢` / `◤`); safe with ordinary terminal fonts.
    Slant,
    /// Use triangle caps that widen toward the bottom (`◢` / `◣`); safe with ordinary terminal
    /// fonts.
    Trapezoid,
    /// Use any pair of single-width characters as the left and right caps.
    ///
    /// Each cap is drawn in the segment's background over the color behind it, so it should fill
    /// the side of its cell that faces the segment.
    Custom {
        /// Cap drawn before the segment.
        left: char,
        /// Cap drawn after the segment.
        right: char,
    },
}

impl CapStyle {
    /// Return the `(left, right)` cap characters, or `None` for [`CapStyle::Padded`].
    ///
    /// `Badge` draws them itself; pass the result to [`crate::widgets::Tabs::caps`] or
    /// [`crate::widgets::DraggableTabBar::caps`] for tabs.
    pub const fn chars(self) -> Option<(char, char)> {
        match self {
            Self::Padded => None,
            // The left cap points right and the right cap points left, so each cap faces inward
            // toward the segment it terminates.
            Self::Half => Some(('\u{2590}', '\u{258c}')),
            Self::Round => Some(('\u{e0b6}', '\u{e0b4}')),
            Self::Arrow => Some(('\u{e0b2}', '\u{e0b0}')),
            Self::Slant => Some(('\u{25e2}', '\u{25e4}')),
            Self::Trapezoid => Some(('\u{25e2}', '\u{25e3}')),
            Self::Custom { left, right } => Some((left, right)),
        }
    }

    /// Return whether this style requires a Powerline/Nerd Font: a named Powerline style, or a
    /// [`CapStyle::Custom`] pair with a Private Use Area character.
    pub const fn requires_nerd_font(self) -> bool {
        match self {
            Self::Round | Self::Arrow => true,
            Self::Custom { left, right } => is_private_use(left) || is_private_use(right),
            _ => false,
        }
    }

    /// Degrade a Nerd Font style to the undecorated padded style.
    ///
    /// [`CapStyle::Half`], [`CapStyle::Slant`], and [`CapStyle::Trapezoid`] remain unchanged
    /// when selected explicitly.
    pub const fn font_safe(self) -> Self {
        if self.requires_nerd_font() {
            Self::Padded
        } else {
            self
        }
    }

    /// Return the canonical cycle of named styles. [`CapStyle::Custom`] is not part of it.
    pub const fn all() -> &'static [Self] {
        &[
            Self::Padded,
            Self::Half,
            Self::Round,
            Self::Arrow,
            Self::Slant,
            Self::Trapezoid,
        ]
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

const fn is_private_use(ch: char) -> bool {
    matches!(
        ch,
        '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}'
    )
}

/// Whether both cap glyphs occupy exactly the one cell of padding they replace.
pub(crate) fn caps_fit_padding((left, right): (char, char)) -> bool {
    UnicodeWidthChar::width(left) == Some(1) && UnicodeWidthChar::width(right) == Some(1)
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
    let glyphs = cap_style.chars();
    if glyphs.is_none() && !(same_color_left && cap_sides.has_left()) {
        return child;
    }

    let cap = |glyph: char, same_color: bool| {
        let contrast = if same_color {
            ContrastPolicy::BlackOrWhite
        } else {
            ContrastPolicy::Off
        };
        Text::new(String::from(glyph))
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
const fn same_color_separator(cap_style: CapStyle) -> char {
    match cap_style {
        CapStyle::Arrow => '\u{e0b3}',
        CapStyle::Slant => '╱',
        _ => '▏',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_style_chars_match_workbar_cap_sets() {
        assert_eq!(CapStyle::Padded.chars(), None);
        assert_eq!(CapStyle::Half.chars(), Some(('\u{2590}', '\u{258c}')));
        assert_eq!(CapStyle::Round.chars(), Some(('\u{e0b6}', '\u{e0b4}')));
        assert_eq!(CapStyle::Arrow.chars(), Some(('\u{e0b2}', '\u{e0b0}')));
        assert_eq!(CapStyle::Slant.chars(), Some(('◢', '◤')));
        assert_eq!(CapStyle::Trapezoid.chars(), Some(('◢', '◣')));
        assert_eq!(
            CapStyle::Custom {
                left: '<',
                right: '>'
            }
            .chars(),
            Some(('<', '>'))
        );
        assert!(caps_fit_padding(('◢', '◤')));
        assert!(caps_fit_padding(('\u{e0b6}', '\u{e0b4}')));
        assert!(!caps_fit_padding(('【', '】')));
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
        assert_eq!(CapStyle::Slant.font_safe(), CapStyle::Slant);
        let powerline = CapStyle::Custom {
            left: '\u{e0ba}',
            right: '>',
        };
        assert_eq!(powerline.font_safe(), CapStyle::Padded);
        let plain = CapStyle::Custom {
            left: '◥',
            right: '◣',
        };
        assert_eq!(plain.font_safe(), plain);
        assert_eq!(CapStyle::Trapezoid.font_safe(), CapStyle::Trapezoid);
    }

    #[test]
    fn all_returns_the_canonical_cap_style_cycle() {
        assert_eq!(
            CapStyle::all(),
            &[
                CapStyle::Padded,
                CapStyle::Half,
                CapStyle::Round,
                CapStyle::Arrow,
                CapStyle::Slant,
                CapStyle::Trapezoid
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
    fn same_color_separators_keep_adjacent_segments_distinct() {
        assert_eq!(same_color_separator(CapStyle::Arrow), '\u{e0b3}');
        assert_eq!(same_color_separator(CapStyle::Slant), '╱');
        assert_eq!(same_color_separator(CapStyle::Round), '▏');
        assert_eq!(same_color_separator(CapStyle::Half), '▏');
        assert_eq!(same_color_separator(CapStyle::Padded), '▏');
    }
}
