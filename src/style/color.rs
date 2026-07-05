use core::fmt;

/// Terminal colors with support for ANSI 16, 256, and true color.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// Terminal default (ANSI reset for that attribute).
    Reset,
    /// Preserve the existing background behind this surface while allowing the
    /// surface to clear or repaint foreground content.
    ///
    /// This is primarily meaningful for background-style surfaces like modal or
    /// frame fills. Unlike [`Color::Transparent`], which keeps both foreground
    /// and background beneath untouched, `Backdrop` is intended for the old
    /// overlay behavior where blank areas clear text but keep the underlying
    /// background color.
    Backdrop,
    /// Do not paint this channel: keep the existing cell / parent style underneath.
    ///
    /// Unlike [`Color::Reset`], which selects the terminal's default palette color,
    /// transparent skips setting foreground or background when converting to
    /// ratatui styles so lower layers show through. For [`crate::style::Style::patch`], a
    /// transparent overlay leaves the resolved base color unchanged for that
    /// channel.
    Transparent,
    /// Black.
    Black,
    /// Red.
    Red,
    /// Green.
    Green,
    /// Yellow.
    Yellow,
    /// Blue.
    Blue,
    /// Magenta.
    Magenta,
    /// Cyan.
    Cyan,
    /// Gray.
    Gray,
    /// Dark gray.
    DarkGray,
    /// Light red.
    LightRed,
    /// Light green.
    LightGreen,
    /// Light yellow.
    LightYellow,
    /// Light blue.
    LightBlue,
    /// Light magenta.
    LightMagenta,
    /// Light cyan.
    LightCyan,
    /// White.
    White,

    /// 256-color palette (0-255).
    Indexed(u8),
    /// True color RGB.
    Rgb(u8, u8, u8),
}

/// A color paint that may carry an alpha channel.
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Fully opaque paint using a terminal color.
    Solid(Color),
    /// Alpha-blended paint over the resolved background.
    Alpha {
        /// Base color to blend.
        color: Color,
        /// Alpha channel where `0` is fully transparent and `255` is fully opaque.
        alpha: u8,
    },
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Color {
    /// Create from a hex string (e.g., `"#FF5733"` or `"FF5733"`).
    ///
    /// Accepted formats (with or without leading `#`):
    /// - 6 chars: `RRGGBB`
    /// - 3 chars: `RGB` (each digit is doubled, e.g. `F80` → `FF8800`)
    ///
    /// Invalid input falls back to [`Color::Reset`].
    pub fn hex(s: &str) -> Self {
        Self::try_hex(s).unwrap_or(Self::Reset)
    }

    pub(crate) fn try_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.is_ascii() {
            return None;
        }

        match s.len() {
            6 => Some(Self::Rgb(
                parse_hex_byte(s, 0)?,
                parse_hex_byte(s, 2)?,
                parse_hex_byte(s, 4)?,
            )),
            3 => Some(Self::Rgb(
                parse_hex_nibble(s, 0)? * 17,
                parse_hex_nibble(s, 1)? * 17,
                parse_hex_nibble(s, 2)? * 17,
            )),
            _ => None,
        }
    }

    /// Create RGB color.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Rgb(r, g, b)
    }

    /// Create RGB color from a `0xRRGGBB` integer literal.
    pub const fn hex_u24(rgb: u32) -> Self {
        Self::Rgb(
            ((rgb >> 16) & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            (rgb & 0xFF) as u8,
        )
    }

    /// Create from 256-color palette index.
    pub const fn indexed(index: u8) -> Self {
        Self::Indexed(index)
    }

    /// Convert to RGB if possible.
    pub fn to_rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Reset | Color::Backdrop | Color::Transparent => None,
            Color::Rgb(r, g, b) => Some((r, g, b)),
            Color::Indexed(index) => Some(indexed_to_rgb(index)),
            Color::Black => Some((0, 0, 0)),
            Color::Red => Some((205, 0, 0)),
            Color::Green => Some((0, 205, 0)),
            Color::Yellow => Some((205, 205, 0)),
            Color::Blue => Some((0, 0, 238)),
            Color::Magenta => Some((205, 0, 205)),
            Color::Cyan => Some((0, 205, 205)),
            Color::Gray => Some((229, 229, 229)),
            Color::DarkGray => Some((127, 127, 127)),
            Color::LightRed => Some((255, 0, 0)),
            Color::LightGreen => Some((0, 255, 0)),
            Color::LightYellow => Some((255, 255, 0)),
            Color::LightBlue => Some((92, 92, 255)),
            Color::LightMagenta => Some((255, 0, 255)),
            Color::LightCyan => Some((0, 255, 255)),
            Color::White => Some((255, 255, 255)),
        }
    }

    /// Dim this color by the default amount.
    ///
    /// The dim level is applied uniformly to ANSI named colors, indexed colors,
    /// and truecolor values by converting to RGB and scaling channel intensity.
    pub fn dim(self) -> Self {
        self.dim_by(0.35)
    }

    /// Dim this color by an explicit amount in the `[0.0, 1.0]` range.
    ///
    /// - `0.0` keeps the color unchanged.
    /// - `1.0` yields black.
    ///
    /// `Color::Reset` is returned unchanged.
    pub fn dim_by(self, amount: f32) -> Self {
        let Some((r, g, b)) = self.to_rgb() else {
            return self;
        };

        let t = amount.clamp(0.0, 1.0);
        let scale = 1.0 - t;
        Self::Rgb(
            scale_channel(r, scale),
            scale_channel(g, scale),
            scale_channel(b, scale),
        )
    }

    /// Lighten this color by the default amount.
    ///
    /// The lightening level is applied uniformly to ANSI named colors, indexed
    /// colors, and truecolor values by converting to RGB and blending channels
    /// toward white.
    pub fn lighten(self) -> Self {
        self.lighten_by(0.35)
    }

    /// Lighten this color by an explicit amount in the `[0.0, 1.0]` range.
    ///
    /// - `0.0` keeps the color unchanged.
    /// - `1.0` yields white.
    ///
    /// `Color::Reset` is returned unchanged.
    pub fn lighten_by(self, amount: f32) -> Self {
        let Some((r, g, b)) = self.to_rgb() else {
            return self;
        };

        let t = amount.clamp(0.0, 1.0);
        Self::Rgb(
            blend_toward_white_channel(r, t),
            blend_toward_white_channel(g, t),
            blend_toward_white_channel(b, t),
        )
    }

    /// Blend this color toward `target` by `alpha` in `[0.0, 1.0]`.
    ///
    /// - `0.0` keeps this color unchanged.
    /// - `1.0` yields `target`.
    /// - `Color::Reset` is treated as black `(0, 0, 0)` for blending.
    ///
    /// Returns this color unchanged if `target` is [`Color::Reset`],
    /// [`Color::Backdrop`], or [`Color::Transparent`].
    pub fn blend_toward(self, target: Color, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        if matches!(target, Color::Reset | Color::Backdrop | Color::Transparent) {
            return self;
        }
        let (r1, g1, b1) = self.to_rgb().unwrap_or((0, 0, 0));
        let Some((r2, g2, b2)) = target.to_rgb() else {
            return self;
        };
        let blend_ch = |a: u8, b: u8| -> u8 {
            (a as f32 * (1.0 - alpha) + b as f32 * alpha)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self::Rgb(blend_ch(r1, r2), blend_ch(g1, g2), blend_ch(b1, b2))
    }

    /// Perceived relative luminance in `[0.0, 1.0]` using Rec. 601 weights.
    ///
    /// Colors that cannot be resolved to RGB (e.g. [`Color::Reset`]) are treated
    /// as black and return `0.0`.
    pub fn luminance(self) -> f32 {
        let (r, g, b) = self.to_rgb().unwrap_or((0, 0, 0));
        (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0
    }

    /// Whether this color reads as a dark surface (luminance below the midpoint).
    ///
    /// Unresolvable colors (e.g. [`Color::Reset`]) are treated as dark.
    pub fn is_dark(self) -> bool {
        self.luminance() < 0.5
    }

    /// Raise a surface off its own background by a perceptually even amount.
    ///
    /// Unlike [`Self::lighten_by`], this is luminance-aware: dark backgrounds are
    /// lightened toward white while light backgrounds are darkened toward black.
    /// This keeps layered surfaces (panels, inputs, menus) visible on both dark
    /// and light themes, where a one-directional lighten would collapse on a
    /// near-white background.
    ///
    /// `amount` is in `[0.0, 1.0]`; `Color::Reset` and similar are returned
    /// unchanged.
    pub fn elevate(self, amount: f32) -> Self {
        if self.to_rgb().is_none() {
            return self;
        }
        if self.is_dark() {
            self.lighten_by(amount)
        } else {
            self.dim_by(amount)
        }
    }
}

impl Paint {
    /// Create fully opaque paint from a terminal color.
    pub const fn solid(color: Color) -> Self {
        Self::Solid(color)
    }

    /// Create fully opaque RGB paint.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Solid(Color::Rgb(r, g, b))
    }

    /// Create RGB paint with an explicit `0..=255` alpha channel.
    pub const fn rgba(r: u8, g: u8, b: u8, alpha: u8) -> Self {
        Self::from_color_alpha_u8(Color::Rgb(r, g, b), alpha)
    }

    /// Create from a hex string, falling back to opaque [`Color::Reset`] on invalid input.
    pub fn hex(s: &str) -> Self {
        Self::try_hex(s).unwrap_or(Self::Solid(Color::Reset))
    }

    /// Create from a hex string.
    ///
    /// Accepted formats (with or without leading `#`):
    /// - 6 chars: `RRGGBB`
    /// - 8 chars: `RRGGBBAA`
    /// - 3 chars: `RGB` (each digit is doubled, e.g. `F80` → `FF8800`)
    /// - 4 chars: `RGBA` (each digit is doubled)
    pub fn try_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.is_ascii() {
            return None;
        }

        match s.len() {
            8 => Some(Self::from_color_alpha_u8(
                Color::Rgb(
                    parse_hex_byte(s, 0)?,
                    parse_hex_byte(s, 2)?,
                    parse_hex_byte(s, 4)?,
                ),
                parse_hex_byte(s, 6)?,
            )),
            6 => Some(Self::rgb(
                parse_hex_byte(s, 0)?,
                parse_hex_byte(s, 2)?,
                parse_hex_byte(s, 4)?,
            )),
            4 => Some(Self::from_color_alpha_u8(
                Color::Rgb(
                    parse_hex_nibble(s, 0)? * 17,
                    parse_hex_nibble(s, 1)? * 17,
                    parse_hex_nibble(s, 2)? * 17,
                ),
                parse_hex_nibble(s, 3)? * 17,
            )),
            3 => Some(Self::rgb(
                parse_hex_nibble(s, 0)? * 17,
                parse_hex_nibble(s, 1)? * 17,
                parse_hex_nibble(s, 2)? * 17,
            )),
            _ => None,
        }
    }

    pub(crate) const fn from_color_alpha_u8(color: Color, alpha: u8) -> Self {
        if alpha == 255 {
            Self::Solid(color)
        } else {
            Self::Alpha { color, alpha }
        }
    }

    pub(crate) fn from_color_alpha(color: Color, alpha: f32) -> Self {
        let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::from_color_alpha_u8(color, alpha)
    }

    /// Return the paint alpha as an exact `0..=255` byte.
    pub const fn alpha_u8(self) -> u8 {
        match self {
            Self::Solid(_) => 255,
            Self::Alpha { alpha, .. } => alpha,
        }
    }

    /// Return the paint alpha as a normalized `0.0..=1.0` value.
    pub fn alpha(self) -> f32 {
        self.alpha_u8() as f32 / 255.0
    }

    /// Return the base color for this paint.
    pub const fn color(self) -> Color {
        match self {
            Self::Solid(color) | Self::Alpha { color, .. } => color,
        }
    }

    /// Return whether this paint is fully opaque.
    pub const fn is_opaque(self) -> bool {
        match self {
            Self::Solid(_) => true,
            Self::Alpha { alpha, .. } => alpha == 255,
        }
    }

    /// Return whether this paint should leave the target channel transparent.
    pub const fn is_transparent_paint(self) -> bool {
        match self {
            Self::Solid(Color::Transparent) => true,
            Self::Solid(_) => false,
            Self::Alpha { alpha, .. } => alpha == 0,
        }
    }

    /// Resolve to an opaque [`Color`] by source-over compositing this paint
    /// against `backdrop`.
    ///
    /// - [`Paint::Solid`] returns its pigment unchanged.
    /// - [`Paint::Alpha`] blends the pigment over `backdrop` weighted by alpha.
    /// - If `backdrop` cannot be resolved to RGB (e.g. [`Color::Reset`],
    ///   [`Color::Transparent`], or [`Color::Backdrop`]), the pigment is
    ///   returned as a best-effort fallback so callers still receive a usable
    ///   color rather than the wrong one.
    pub fn flatten_over(self, backdrop: Color) -> Color {
        match self {
            Self::Solid(color) => color,
            Self::Alpha { color, alpha } => {
                if backdrop.to_rgb().is_none() {
                    return color;
                }
                let weight = alpha as f32 / 255.0;
                backdrop.blend_toward(color, weight)
            }
        }
    }

    pub(crate) const fn is_transparent_sentinel(self) -> bool {
        matches!(self, Self::Solid(Color::Transparent))
    }

    pub(crate) const fn is_backdrop_sentinel(self) -> bool {
        matches!(self, Self::Solid(Color::Backdrop))
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Self::Solid(color)
    }
}

#[cfg(all(test, feature = "terminal-serde"))]
mod terminal_serde_tests {
    use super::*;

    #[test]
    fn color_round_trips() {
        let color = Color::Rgb(12, 34, 56);
        let json = serde_json::to_string(&color).unwrap();
        assert_eq!(serde_json::from_str::<Color>(&json).unwrap(), color);
    }

    #[test]
    fn paint_round_trips() {
        let paint = Paint::Alpha {
            color: Color::Indexed(42),
            alpha: 128,
        };
        let json = serde_json::to_string(&paint).unwrap();
        assert_eq!(serde_json::from_str::<Paint>(&json).unwrap(), paint);
    }
}

impl PartialEq<Color> for Paint {
    fn eq(&self, other: &Color) -> bool {
        matches!(self, Self::Solid(color) if color == other)
    }
}

impl PartialEq<Paint> for Color {
    fn eq(&self, other: &Paint) -> bool {
        other == self
    }
}

fn parse_hex_byte(s: &str, start: usize) -> Option<u8> {
    u8::from_str_radix(&s[start..start + 2], 16).ok()
}

fn parse_hex_nibble(s: &str, start: usize) -> Option<u8> {
    u8::from_str_radix(&s[start..start + 1], 16).ok()
}

fn scale_channel(channel: u8, scale: f32) -> u8 {
    ((channel as f32 * scale).round()).clamp(0.0, 255.0) as u8
}

fn blend_toward_white_channel(channel: u8, t: f32) -> u8 {
    let channel = channel as f32;
    (channel + (255.0 - channel) * t).round().clamp(0.0, 255.0) as u8
}

fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];

    if index < 16 {
        return ANSI16[index as usize];
    }

    if index >= 232 {
        let gray = 8u8.saturating_add((index - 232).saturating_mul(10));
        return (gray, gray, gray);
    }

    let idx = index - 16;
    let r = idx / 36;
    let g = (idx % 36) / 6;
    let b = idx % 6;
    let to_level = |v: u8| match v {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    };
    (to_level(r), to_level(g), to_level(b))
}

#[cfg(test)]
mod tests {
    use super::{Color, Paint};

    #[test]
    fn dim_by_zero_keeps_color() {
        assert_eq!(
            Color::rgb(100, 120, 140).dim_by(0.0),
            Color::Rgb(100, 120, 140)
        );
    }

    #[test]
    fn dim_by_one_makes_black() {
        assert_eq!(Color::rgb(100, 120, 140).dim_by(1.0), Color::Rgb(0, 0, 0));
    }

    #[test]
    fn dim_handles_ansi_and_indexed() {
        assert_ne!(Color::LightBlue.dim(), Color::LightBlue);
        assert_ne!(Color::Indexed(214).dim(), Color::Indexed(214));
    }

    #[test]
    fn lighten_by_zero_keeps_color() {
        assert_eq!(
            Color::rgb(100, 120, 140).lighten_by(0.0),
            Color::Rgb(100, 120, 140)
        );
    }

    #[test]
    fn lighten_by_one_makes_white() {
        assert_eq!(
            Color::rgb(100, 120, 140).lighten_by(1.0),
            Color::Rgb(255, 255, 255)
        );
    }

    #[test]
    fn lighten_handles_ansi_and_indexed() {
        assert_ne!(Color::Blue.lighten(), Color::Blue);
        assert_ne!(Color::Indexed(214).lighten(), Color::Indexed(214));
    }

    #[test]
    fn is_dark_splits_on_luminance_midpoint() {
        assert!(Color::hex_u24(0x0B121F).is_dark());
        assert!(Color::Black.is_dark());
        assert!(!Color::White.is_dark());
        assert!(!Color::hex_u24(0xFDF6E3).is_dark());
    }

    #[test]
    fn elevate_lightens_dark_and_darkens_light() {
        // Dark surface elevates toward white (channels increase).
        let dark = Color::hex_u24(0x0B121F);
        let dark_up = dark.elevate(0.10);
        assert_eq!(dark_up, dark.lighten_by(0.10));
        assert!(dark_up.luminance() > dark.luminance());

        // Light surface elevates toward black (channels decrease).
        let light = Color::White;
        let light_up = light.elevate(0.10);
        assert_eq!(light_up, light.dim_by(0.10));
        assert!(light_up.luminance() < light.luminance());
    }

    #[test]
    fn elevate_leaves_non_rgb_colors_unchanged() {
        assert_eq!(Color::Reset.elevate(0.2), Color::Reset);
    }

    #[test]
    fn blend_toward_zero_keeps_color() {
        assert_eq!(
            Color::rgb(100, 120, 140).blend_toward(Color::rgb(200, 0, 0), 0.0),
            Color::Rgb(100, 120, 140)
        );
    }

    #[test]
    fn blend_toward_one_yields_target() {
        assert_eq!(
            Color::rgb(100, 120, 140).blend_toward(Color::rgb(200, 10, 30), 1.0),
            Color::Rgb(200, 10, 30)
        );
    }

    #[test]
    fn blend_toward_midpoint() {
        assert_eq!(
            Color::rgb(0, 0, 0).blend_toward(Color::rgb(100, 200, 50), 0.5),
            Color::Rgb(50, 100, 25)
        );
    }

    #[test]
    fn blend_toward_reset_source_treats_as_black() {
        // Reset is treated as black (0,0,0) for blending.
        assert_eq!(
            Color::Reset.blend_toward(Color::rgb(100, 0, 200), 0.5),
            Color::Rgb(50, 0, 100)
        );
    }

    #[test]
    fn blend_toward_reset_target_returns_unchanged() {
        // Cannot blend toward Reset - original color returned.
        assert_eq!(
            Color::rgb(100, 120, 140).blend_toward(Color::Reset, 0.5),
            Color::Rgb(100, 120, 140)
        );
    }

    #[test]
    fn blend_toward_transparent_target_returns_unchanged() {
        assert_eq!(
            Color::rgb(100, 120, 140).blend_toward(Color::Transparent, 0.5),
            Color::Rgb(100, 120, 140)
        );
    }

    #[test]
    fn blend_toward_handles_ansi_and_indexed() {
        assert_ne!(
            Color::LightBlue.blend_toward(Color::rgb(200, 0, 0), 0.5),
            Color::LightBlue
        );
        assert_ne!(
            Color::Indexed(214).blend_toward(Color::rgb(0, 0, 200), 0.5),
            Color::Indexed(214)
        );
    }

    #[test]
    fn hex_6char() {
        assert_eq!(Color::hex("#FF5733"), Color::Rgb(0xFF, 0x57, 0x33));
        assert_eq!(Color::hex("a32525"), Color::Rgb(0xa3, 0x25, 0x25));
    }

    #[test]
    fn hex_u24_matches_rgb_channels() {
        assert_eq!(Color::hex_u24(0xFF_57_33), Color::Rgb(0xFF, 0x57, 0x33));
        assert_eq!(Color::hex_u24(0x00_2B_36), Color::Rgb(0x00, 0x2B, 0x36));
    }

    #[test]
    fn hex_8char_alpha_rejected() {
        assert_eq!(Color::hex("#a32525ff"), Color::Reset);
        assert_eq!(Color::hex("FF573380"), Color::Reset);
    }

    #[test]
    fn hex_3char_shorthand() {
        // #F80 → #FF8800
        assert_eq!(Color::hex("#F80"), Color::Rgb(0xFF, 0x88, 0x00));
        assert_eq!(Color::hex("abc"), Color::Rgb(0xAA, 0xBB, 0xCC));
    }

    #[test]
    fn hex_4char_shorthand_alpha_rejected() {
        assert_eq!(Color::hex("#F80A"), Color::Reset);
    }

    #[test]
    fn hex_invalid_input_falls_back_to_reset() {
        assert_eq!(Color::hex("#FF"), Color::Reset);
        assert_eq!(Color::hex("#FF573"), Color::Reset);
        assert_eq!(Color::hex("#FF57338"), Color::Reset);
        assert_eq!(Color::hex("#FF5733801"), Color::Reset);
        assert_eq!(Color::hex("#GG5733"), Color::Reset);
        assert_eq!(Color::hex("#caf🙂"), Color::Reset);
    }

    #[test]
    fn try_hex_stays_fallible_for_internal_parsers() {
        assert_eq!(
            Color::try_hex("#FF5733"),
            Some(Color::Rgb(0xFF, 0x57, 0x33))
        );
        assert_eq!(Color::try_hex("#FF"), None);
        assert_eq!(Color::try_hex("#FF573380"), None);
        assert_eq!(Color::try_hex("#F808"), None);
    }

    #[test]
    fn paint_from_color_preserves_special_colors() {
        assert_eq!(
            Paint::from(Color::Transparent),
            Paint::Solid(Color::Transparent)
        );
        assert_eq!(Paint::solid(Color::Backdrop), Paint::Solid(Color::Backdrop));
        assert!(Paint::from(Color::Transparent).is_transparent_paint());
        assert!(!Paint::solid(Color::Backdrop).is_transparent_paint());
    }

    #[test]
    fn paint_constructors_canonicalize_opaque_alpha_only() {
        assert_eq!(
            Paint::rgb(0xFF, 0x57, 0x33),
            Paint::Solid(Color::Rgb(0xFF, 0x57, 0x33))
        );
        assert_eq!(
            Paint::rgba(0xFF, 0x57, 0x33, 255),
            Paint::Solid(Color::Rgb(0xFF, 0x57, 0x33))
        );
        assert_eq!(
            Paint::rgba(0xFF, 0x57, 0x33, 0),
            Paint::Alpha {
                color: Color::Rgb(0xFF, 0x57, 0x33),
                alpha: 0,
            }
        );
    }

    #[test]
    fn paint_alpha_helpers_and_accessors() {
        let paint = Paint::rgba(205, 0, 0, 128);
        assert_eq!(
            paint,
            Paint::Alpha {
                color: Color::Rgb(205, 0, 0),
                alpha: 128
            }
        );
        assert_eq!(paint.color(), Color::Rgb(205, 0, 0));
        assert_eq!(paint.alpha_u8(), 128);
        assert!((paint.alpha() - (128.0 / 255.0)).abs() < f32::EPSILON);
        assert!(!paint.is_opaque());
        assert!(!paint.is_transparent_paint());

        assert!(Paint::rgba(0, 0, 255, 255).is_opaque());
        assert!(Paint::rgba(0, 0, 255, 0).is_transparent_paint());
    }

    #[test]
    fn paint_hex_preserves_8char_alpha() {
        assert_eq!(
            Paint::hex("#a3252500"),
            Paint::Alpha {
                color: Color::Rgb(0xa3, 0x25, 0x25),
                alpha: 0x00,
            }
        );
        assert_eq!(
            Paint::hex("FF573380"),
            Paint::Alpha {
                color: Color::Rgb(0xFF, 0x57, 0x33),
                alpha: 0x80,
            }
        );
        assert_eq!(
            Paint::hex("#a32525ff"),
            Paint::Solid(Color::Rgb(0xa3, 0x25, 0x25))
        );
    }

    #[test]
    fn paint_hex_preserves_4char_shorthand_alpha() {
        assert_eq!(
            Paint::hex("#F80A"),
            Paint::Alpha {
                color: Color::Rgb(0xFF, 0x88, 0x00),
                alpha: 0xAA,
            }
        );
        assert_eq!(
            Paint::hex("#F80F"),
            Paint::Solid(Color::Rgb(0xFF, 0x88, 0x00))
        );
    }

    #[test]
    fn paint_flatten_over_solid_returns_pigment() {
        assert_eq!(
            Paint::Solid(Color::rgb(10, 20, 30)).flatten_over(Color::rgb(200, 200, 200)),
            Color::Rgb(10, 20, 30)
        );
    }

    #[test]
    fn paint_flatten_over_alpha_blends_against_backdrop() {
        // 50% red over blue → midpoint
        assert_eq!(
            Paint::rgba(200, 0, 0, 128).flatten_over(Color::rgb(0, 0, 200)),
            Color::Rgb(100, 0, 100)
        );
        // Fully transparent → preserves backdrop
        assert_eq!(
            Paint::rgba(200, 0, 0, 0).flatten_over(Color::rgb(0, 0, 200)),
            Color::Rgb(0, 0, 200)
        );
        // Fully opaque → pigment
        assert_eq!(
            Paint::rgba(200, 0, 0, 255).flatten_over(Color::rgb(0, 0, 200)),
            Color::Rgb(200, 0, 0)
        );
    }

    #[test]
    fn paint_flatten_over_unresolvable_backdrop_returns_pigment() {
        assert_eq!(
            Paint::rgba(200, 0, 0, 128).flatten_over(Color::Reset),
            Color::Rgb(200, 0, 0)
        );
        assert_eq!(
            Paint::rgba(200, 0, 0, 128).flatten_over(Color::Transparent),
            Color::Rgb(200, 0, 0)
        );
    }

    #[test]
    fn paint_try_hex_rejects_invalid_input() {
        assert_eq!(Paint::try_hex("#FF"), None);
        assert_eq!(Paint::try_hex("#FF573"), None);
        assert_eq!(Paint::try_hex("#FF57338"), None);
        assert_eq!(Paint::try_hex("#FF5733801"), None);
        assert_eq!(Paint::try_hex("#GG5733"), None);
        assert_eq!(Paint::try_hex("#caf🙂"), None);
    }
}
