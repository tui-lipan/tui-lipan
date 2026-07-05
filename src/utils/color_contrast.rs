//! WCAG 2.1 color contrast utilities for readable TUI text.
//!
//! # Terminal color limitation
//!
//! The 16 ANSI named colors (`Color::Black`, `Color::Blue`, etc.) are mapped to
//! **approximate xterm-style RGB values** for contrast calculations. The *actual*
//! rendered color depends on the user's terminal theme - "Blue" in Solarized
//! looks nothing like "Blue" in Dracula.
//!
//! There is **no reliable, portable way** to query a terminal for its current
//! palette at runtime. Some terminals support OSC 4/10/11 queries, but support
//! is inconsistent and the response is asynchronous, making it impractical for
//! synchronous layout/render passes.
//!
//! **Recommendation:** Use `Color::Rgb(r, g, b)` for precise contrast control.
//! The ANSI approximations are a reasonable default, but users with heavily
//! customized terminal palettes may see suboptimal results for ANSI colors.

use crate::style::{Color, Paint, Style};

/// Minimum WCAG 2.1 contrast ratio for normal-sized readable text (AA level).
pub const MIN_READABLE_CONTRAST: f32 = 4.5;

/// Minimum WCAG 2.1 contrast ratio for large text (AA level).
pub const MIN_LARGE_TEXT_CONTRAST: f32 = 3.0;

// ---------------------------------------------------------------------------
// WCAG 2.1 luminance & contrast
// ---------------------------------------------------------------------------

/// Compute WCAG 2.1 relative luminance for a color.
///
/// Returns `0.0` for colors that cannot be resolved to RGB (e.g. `Color::Reset`).
///
/// Reference: <https://www.w3.org/TR/WCAG21/#dfn-relative-luminance>
pub fn relative_luminance(color: Color) -> f32 {
    let Some((r, g, b)) = color.to_rgb() else {
        return 0.0;
    };
    luminance_from_rgb(r, g, b)
}

/// Compute WCAG 2.1 contrast ratio between two colors (range 1.0–21.0).
///
/// Reference: <https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio>
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

// ---------------------------------------------------------------------------
// Color transforms (general-purpose utilities)
// ---------------------------------------------------------------------------

/// RGB inverse: `(255 - r, 255 - g, 255 - b)`.
pub fn inverse_color(color: Color) -> Option<Color> {
    let (r, g, b) = color.to_rgb()?;
    Some(Color::rgb(255 - r, 255 - g, 255 - b))
}

/// Hue-complementary color (180° hue rotation in HSL space).
pub fn complementary_color(color: Color) -> Option<Color> {
    let (r, g, b) = color.to_rgb()?;
    let (h, s, l) = rgb_to_hsl(r, g, b);
    Some(hsl_to_color((h + 180.0) % 360.0, s, l))
}

// ---------------------------------------------------------------------------
// Readable text color selection
// ---------------------------------------------------------------------------

/// Choose a readable foreground color for text on `bg`.
///
/// 1. If `preferred` already meets [`MIN_READABLE_CONTRAST`], keep it.
/// 2. Otherwise try to adjust `preferred` by shifting its lightness
///    (preserving hue and saturation).
/// 3. If adjustment fails (or no preferred was given), fall back to black or
///    white - whichever has better contrast with `bg`.
///
/// For `Color::Reset` backgrounds the actual color depends on the terminal
/// theme, so no adjustment is possible and `preferred` is returned as-is.
pub fn readable_text_color(preferred: Option<Color>, bg: Color) -> Color {
    // Cannot determine contrast when bg is terminal-default.
    if bg.to_rgb().is_none() {
        return preferred.unwrap_or(Color::White);
    }

    if let Some(pref) = preferred {
        // Cannot evaluate a Reset foreground - pass through.
        if pref.to_rgb().is_none() {
            return pref;
        }
        if contrast_ratio(pref, bg) >= MIN_READABLE_CONTRAST {
            return pref;
        }
        // Try to keep the hue by adjusting lightness.
        if let Some(adjusted) = adjust_for_contrast(pref, bg, MIN_READABLE_CONTRAST) {
            return adjusted;
        }
    }

    black_or_white(bg)
}

/// Choose a readable foreground by keeping `preferred` when it already meets
/// WCAG AA contrast, otherwise snapping directly to black or white.
///
/// Unlike [`readable_text_color`], this does not attempt hue-preserving
/// lightness adjustment. It is useful for accent surfaces and badges where a
/// simple black/white result is preferred over a tinted foreground.
pub fn readable_text_color_black_or_white(preferred: Option<Color>, bg: Color) -> Color {
    if bg.to_rgb().is_none() {
        return preferred.unwrap_or(Color::White);
    }

    if let Some(pref) = preferred {
        if pref.to_rgb().is_none() {
            return pref;
        }
        if contrast_ratio(pref, bg) >= MIN_READABLE_CONTRAST {
            return pref;
        }
    }

    black_or_white(bg)
}

/// Pick black or white - whichever has more contrast with `bg`.
///
/// For ANSI named backgrounds this returns ANSI `Color::Black` / `Color::White`.
/// For indexed backgrounds it returns indexed `Color::Indexed(0)` /
/// `Color::Indexed(15)`.
/// For truecolor backgrounds it returns truecolor `Rgb(0,0,0)` /
/// `Rgb(255,255,255)` so the fallback is explicit and not terminal-theme
/// dependent.
///
/// This is the approach used by Material Design, Apple HIG, and most major
/// design systems for automatic text color selection.
pub fn black_or_white(bg: Color) -> Color {
    if contrast_ratio(Color::White, bg) >= contrast_ratio(Color::Black, bg) {
        white_for_bg_family(bg)
    } else {
        black_for_bg_family(bg)
    }
}

fn black_for_bg_family(bg: Color) -> Color {
    match bg {
        Color::Rgb(..) => Color::rgb(0, 0, 0),
        Color::Indexed(_) => Color::indexed(0),
        _ => Color::Black,
    }
}

fn white_for_bg_family(bg: Color) -> Color {
    match bg {
        Color::Rgb(..) => Color::rgb(255, 255, 255),
        Color::Indexed(_) => Color::indexed(15),
        _ => Color::White,
    }
}

/// Adjust `fg` lightness to meet `target_ratio` against `bg`.
///
/// Preserves the hue and saturation of `fg`, only shifting lightness in HSL
/// space. Returns `None` if no adjustment within the valid lightness range
/// can satisfy the target ratio.
pub fn adjust_for_contrast(fg: Color, bg: Color, target_ratio: f32) -> Option<Color> {
    let (r, g, b) = fg.to_rgb()?;
    let (h, s, orig_l) = rgb_to_hsl(r, g, b);

    let light_extreme = hsl_to_color(h, s, 1.0);
    let dark_extreme = hsl_to_color(h, s, 0.0);

    let light_candidate = if contrast_ratio(light_extreme, bg) >= target_ratio {
        let mut lo = orig_l;
        let mut hi = 1.0;

        // 16 iterations → precision ≈ 1.0 / 2^16 in lightness.
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            let candidate = hsl_to_color(h, s, mid);
            if contrast_ratio(candidate, bg) >= target_ratio {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        let result = hsl_to_color(h, s, hi);
        (contrast_ratio(result, bg) >= target_ratio).then_some((hi, result))
    } else {
        None
    };

    let dark_candidate = if contrast_ratio(dark_extreme, bg) >= target_ratio {
        let mut lo = 0.0;
        let mut hi = orig_l;

        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            let candidate = hsl_to_color(h, s, mid);
            if contrast_ratio(candidate, bg) >= target_ratio {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        let result = hsl_to_color(h, s, lo);
        (contrast_ratio(result, bg) >= target_ratio).then_some((lo, result))
    } else {
        None
    };

    match (light_candidate, dark_candidate) {
        (Some((light_l, light)), Some((dark_l, dark))) => {
            let light_delta = (light_l - orig_l).abs();
            let dark_delta = (dark_l - orig_l).abs();
            if light_delta <= dark_delta {
                Some(light)
            } else {
                Some(dark)
            }
        }
        (Some((_, light)), None) => Some(light),
        (None, Some((_, dark))) => Some(dark),
        (None, None) => None,
    }
}

/// Ensure a style has readable text against its own background.
///
/// Resolves color transforms first. When both `fg` and `bg` are set, adjusts
/// `fg` for readability.
pub fn readable_style(mut style: Style) -> Style {
    style = style.resolve_color_transforms();
    style.contrast_policy = None;
    if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
        style.fg = Some(Paint::from(readable_text_color(
            Some(fg.color()),
            bg.color(),
        )));
    }
    style
}

/// Ensure a style has readable text by preserving the current foreground when
/// it already passes WCAG AA and otherwise snapping to black or white.
///
/// This is a more predictable alternative to [`readable_style`] for accent
/// surfaces where hue-preserving foreground adjustment is undesirable.
pub fn readable_style_black_or_white(mut style: Style) -> Style {
    style = style.resolve_color_transforms();
    style.contrast_policy = None;
    if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
        style.fg = Some(Paint::from(readable_text_color_black_or_white(
            Some(fg.color()),
            bg.color(),
        )));
    }
    style
}

// ---------------------------------------------------------------------------
// APCA (WCAG 3.0 draft) perceptual contrast
// ---------------------------------------------------------------------------

/// Minimum APCA lightness contrast for readable body text.
///
/// Based on APCA-W3 v0.1.9 - body text at 14–16px normal weight.
pub const MIN_APCA_BODY_CONTRAST: f32 = 60.0;

/// Compute APCA perceptual luminance (Y) for sRGB values.
///
/// Uses a simple power-curve transfer (exponent 2.4) without the piecewise
/// linearization used by WCAG 2.1. APCA coefficients are slightly different
/// from Rec. 709.
fn apca_luminance(r: u8, g: u8, b: u8) -> f32 {
    fn to_linear(c: u8) -> f32 {
        (c as f32 / 255.0).powf(2.4)
    }
    0.2126729 * to_linear(r) + 0.7151522 * to_linear(g) + 0.0721750 * to_linear(b)
}

/// Compute APCA lightness contrast (Lc) between text and background.
///
/// Returns a signed value:
/// - Positive: dark text on light background (normal polarity)
/// - Negative: light text on dark background (reverse polarity)
/// - `|Lc|` >= 60 is recommended for body text
///
/// Reference: <https://github.com/Myndex/SAPC-APCA>
pub fn apca_contrast(text: Color, bg: Color) -> f32 {
    let Some((tr, tg, tb)) = text.to_rgb() else {
        return 0.0;
    };
    let Some((br, bg_g, bb)) = bg.to_rgb() else {
        return 0.0;
    };

    let mut y_text = apca_luminance(tr, tg, tb);
    let mut y_bg = apca_luminance(br, bg_g, bb);

    // Soft-clamp near black
    if y_text < 0.022 {
        y_text += (0.022 - y_text).powf(1.414);
    }
    if y_bg < 0.022 {
        y_bg += (0.022 - y_bg).powf(1.414);
    }

    // SAPC-4 power curve
    const NORM_BG: f32 = 0.56;
    const NORM_TEXT: f32 = 0.57;
    const REV_BG: f32 = 0.65;
    const REV_TEXT: f32 = 0.62;
    const SCALE: f32 = 1.14;
    const THRESHOLD: f32 = 0.1;
    const OFFSET: f32 = 0.027;

    if y_bg > y_text {
        // Normal polarity: dark text on light background → positive Lc
        let s_apc = (y_bg.powf(NORM_BG) - y_text.powf(NORM_TEXT)) * SCALE;
        if s_apc < THRESHOLD {
            0.0
        } else {
            (s_apc - OFFSET) * 100.0
        }
    } else {
        // Reverse polarity: light text on dark background → negative Lc
        let s_apc = (y_bg.powf(REV_BG) - y_text.powf(REV_TEXT)) * SCALE;
        if s_apc > -THRESHOLD {
            0.0
        } else {
            (s_apc + OFFSET) * 100.0
        }
    }
}

/// Choose a readable foreground using APCA perceptual contrast.
///
/// Follows the same fallback strategy as [`readable_text_color`] but uses
/// APCA `|Lc|` >= [`MIN_APCA_BODY_CONTRAST`] instead of WCAG 2.1 ratio.
pub fn readable_text_color_apca(preferred: Option<Color>, bg: Color) -> Color {
    if bg.to_rgb().is_none() {
        return preferred.unwrap_or(Color::White);
    }

    if let Some(pref) = preferred {
        if pref.to_rgb().is_none() {
            return pref;
        }
        if apca_contrast(pref, bg).abs() >= MIN_APCA_BODY_CONTRAST {
            return pref;
        }
        if let Some(adjusted) = adjust_for_apca_contrast(pref, bg, MIN_APCA_BODY_CONTRAST) {
            return adjusted;
        }
    }

    apca_black_or_white(bg)
}

/// Pick black or white using APCA (polarity-aware).
pub fn apca_black_or_white(bg: Color) -> Color {
    let white_lc = apca_contrast(Color::White, bg).abs();
    let black_lc = apca_contrast(Color::Black, bg).abs();
    if white_lc >= black_lc {
        white_for_bg_family(bg)
    } else {
        black_for_bg_family(bg)
    }
}

/// APCA version of [`adjust_for_contrast`] - same binary search, different
/// predicate.
pub fn adjust_for_apca_contrast(fg: Color, bg: Color, target_lc: f32) -> Option<Color> {
    let (r, g, b) = fg.to_rgb()?;
    let (h, s, orig_l) = rgb_to_hsl(r, g, b);

    let light_extreme = hsl_to_color(h, s, 1.0);
    let dark_extreme = hsl_to_color(h, s, 0.0);

    let light_candidate = if apca_contrast(light_extreme, bg).abs() >= target_lc {
        let mut lo = orig_l;
        let mut hi = 1.0;

        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            let candidate = hsl_to_color(h, s, mid);
            if apca_contrast(candidate, bg).abs() >= target_lc {
                hi = mid;
            } else {
                lo = mid;
            }
        }

        let result = hsl_to_color(h, s, hi);
        (apca_contrast(result, bg).abs() >= target_lc).then_some((hi, result))
    } else {
        None
    };

    let dark_candidate = if apca_contrast(dark_extreme, bg).abs() >= target_lc {
        let mut lo = 0.0;
        let mut hi = orig_l;

        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            let candidate = hsl_to_color(h, s, mid);
            if apca_contrast(candidate, bg).abs() >= target_lc {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        let result = hsl_to_color(h, s, lo);
        (apca_contrast(result, bg).abs() >= target_lc).then_some((lo, result))
    } else {
        None
    };

    match (light_candidate, dark_candidate) {
        (Some((light_l, light)), Some((dark_l, dark))) => {
            let light_delta = (light_l - orig_l).abs();
            let dark_delta = (dark_l - orig_l).abs();
            if light_delta <= dark_delta {
                Some(light)
            } else {
                Some(dark)
            }
        }
        (Some((_, light)), None) => Some(light),
        (None, Some((_, dark))) => Some(dark),
        (None, None) => None,
    }
}

/// Ensure a style has readable text using APCA perceptual contrast.
pub fn readable_style_apca(mut style: Style) -> Style {
    style = style.resolve_color_transforms();
    style.contrast_policy = None;
    if let (Some(fg), Some(bg)) = (style.fg, style.bg) {
        style.fg = Some(Paint::from(readable_text_color_apca(
            Some(fg.color()),
            bg.color(),
        )));
    }
    style
}

// ---------------------------------------------------------------------------
// Internal: sRGB linearization & luminance
// ---------------------------------------------------------------------------

/// Linearize sRGB channels and compute luminance.
///
/// Threshold `0.04045` per WCAG 2.1 / IEC 61966-2-1 sRGB specification.
fn luminance_from_rgb(r: u8, g: u8, b: u8) -> f32 {
    fn linearize(c: u8) -> f32 {
        let v = c as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
}

// ---------------------------------------------------------------------------
// Internal: HSL conversions
// ---------------------------------------------------------------------------

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;

    if delta < f32::EPSILON {
        return (0.0, 0.0, l);
    }

    let s = if l <= 0.5 {
        delta / (max + min)
    } else {
        delta / (2.0 - max - min)
    };

    let h = if (max - r).abs() < f32::EPSILON {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    (h, s, l)
}

fn hsl_to_color(h: f32, s: f32, l: f32) -> Color {
    if s < f32::EPSILON {
        let v = (l * 255.0).round().clamp(0.0, 255.0) as u8;
        return Color::rgb(v, v, v);
    }

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;

    let (rp, gp, bp) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    let to_u8 = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::rgb(to_u8(rp), to_u8(gp), to_u8(bp))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Luminance ---------------------------------------------------------

    #[test]
    fn black_luminance_is_zero() {
        assert!((relative_luminance(Color::Black)).abs() < 1e-4);
    }

    #[test]
    fn white_luminance_is_one() {
        assert!((relative_luminance(Color::White) - 1.0).abs() < 1e-4);
    }

    // --- Contrast ratio ----------------------------------------------------

    #[test]
    fn black_white_maximum_contrast() {
        let ratio = contrast_ratio(Color::Black, Color::White);
        assert!(ratio > 20.0, "Expected >20, got {ratio}");
    }

    #[test]
    fn same_color_has_ratio_one() {
        let ratio = contrast_ratio(Color::Blue, Color::Blue);
        assert!((ratio - 1.0).abs() < 0.01);
    }

    // --- black_or_white ----------------------------------------------------

    #[test]
    fn white_text_on_dark_backgrounds() {
        for bg in [Color::Black, Color::Blue, Color::Red] {
            assert_eq!(
                black_or_white(bg),
                Color::White,
                "Expected ANSI white on {bg:?}"
            );
        }

        assert_eq!(
            black_or_white(Color::rgb(0, 0, 128)),
            Color::rgb(255, 255, 255),
            "Expected truecolor white on truecolor dark background"
        );
    }

    #[test]
    fn black_text_on_light_backgrounds() {
        for bg in [Color::White, Color::LightYellow] {
            assert_eq!(
                black_or_white(bg),
                Color::Black,
                "Expected ANSI black on {bg:?}"
            );
        }

        assert_eq!(
            black_or_white(Color::rgb(200, 220, 255)),
            Color::rgb(0, 0, 0),
            "Expected truecolor black on truecolor light background"
        );
    }

    // --- readable_text_color -----------------------------------------------

    #[test]
    fn preferred_kept_when_readable() {
        let chosen = readable_text_color(Some(Color::White), Color::rgb(0, 30, 120));
        assert_eq!(chosen, Color::White);
    }

    #[test]
    fn unreadable_preferred_adjusts_or_falls_back() {
        let bg = Color::rgb(255, 0, 0);
        let chosen = readable_text_color(Some(Color::Black), bg);
        assert!(
            contrast_ratio(chosen, bg) >= MIN_READABLE_CONTRAST,
            "Chosen {chosen:?} doesn't meet contrast on {bg:?}: {}",
            contrast_ratio(chosen, bg)
        );
    }

    #[test]
    fn black_on_blue_becomes_readable() {
        // Core bug scenario: black on blue must NOT become a colored variant.
        let bg = Color::Blue;
        let chosen = readable_text_color(Some(Color::Black), bg);
        let ratio = contrast_ratio(chosen, bg);
        assert!(
            ratio >= MIN_READABLE_CONTRAST,
            "Black on Blue adjusted to {chosen:?} with ratio {ratio}"
        );
    }

    #[test]
    fn black_on_rgb_blue_becomes_readable() {
        let bg = Color::rgb(0, 0, 238);
        let chosen = readable_text_color(Some(Color::Black), bg);
        let ratio = contrast_ratio(chosen, bg);
        assert!(
            ratio >= MIN_READABLE_CONTRAST,
            "Black on RGB blue adjusted to {chosen:?} with ratio {ratio}"
        );
    }

    #[test]
    fn reset_background_preserves_preferred() {
        let chosen = readable_text_color(Some(Color::Red), Color::Reset);
        assert_eq!(chosen, Color::Red);
    }

    #[test]
    fn reset_fg_preserved_as_is() {
        let chosen = readable_text_color(Some(Color::Reset), Color::rgb(0, 0, 0));
        assert_eq!(chosen, Color::Reset);
    }

    #[test]
    fn no_preferred_picks_black_or_white() {
        let on_dark = readable_text_color(None, Color::rgb(20, 20, 40));
        assert_eq!(on_dark, Color::rgb(255, 255, 255));

        let on_light = readable_text_color(None, Color::rgb(240, 240, 220));
        assert_eq!(on_light, Color::rgb(0, 0, 0));
    }

    #[test]
    fn no_preferred_on_ansi_bg_returns_ansi_black_or_white() {
        assert_eq!(readable_text_color(None, Color::Blue), Color::White);
        assert_eq!(readable_text_color(None, Color::White), Color::Black);
    }

    #[test]
    fn no_preferred_on_indexed_bg_returns_indexed_black_or_white() {
        assert_eq!(
            readable_text_color(None, Color::indexed(4)),
            Color::indexed(15)
        );
        assert_eq!(
            readable_text_color(None, Color::indexed(15)),
            Color::indexed(0)
        );
    }

    #[test]
    fn ansi_blue_bg_gets_readable_text() {
        let chosen = readable_text_color(Some(Color::Black), Color::Blue);
        let ratio = contrast_ratio(chosen, Color::Blue);
        assert!(ratio >= MIN_READABLE_CONTRAST, "ratio = {ratio}");
    }

    #[test]
    fn ansi_colors_are_contrast_checked() {
        // Same fg/bg must not be kept - previous code bypassed ANSI colors.
        let chosen = readable_text_color(Some(Color::Blue), Color::Blue);
        assert_ne!(chosen, Color::Blue, "Same fg/bg should not be kept");
    }

    #[test]
    fn black_or_white_policy_keeps_readable_preferred() {
        let bg = Color::rgb(157, 124, 216);
        let chosen = readable_text_color_black_or_white(Some(Color::Black), bg);
        assert_eq!(chosen, Color::Black);
    }

    #[test]
    fn black_or_white_policy_snaps_unreadable_preferred_to_binary_choice() {
        let bg = Color::rgb(157, 124, 216);
        let chosen = readable_text_color_black_or_white(Some(Color::White), bg);
        assert_eq!(chosen, Color::rgb(0, 0, 0));
    }

    #[test]
    fn black_or_white_policy_without_preferred_picks_binary_choice() {
        let on_dark = readable_text_color_black_or_white(None, Color::rgb(20, 20, 40));
        assert_eq!(on_dark, Color::rgb(255, 255, 255));

        let on_light = readable_text_color_black_or_white(None, Color::rgb(240, 240, 220));
        assert_eq!(on_light, Color::rgb(0, 0, 0));
    }

    // --- adjust_for_contrast -----------------------------------------------

    #[test]
    fn adjust_darkens_light_fg_on_light_bg() {
        let fg = Color::rgb(200, 200, 200);
        let bg = Color::rgb(240, 240, 240);
        let adjusted = adjust_for_contrast(fg, bg, MIN_READABLE_CONTRAST);
        assert!(adjusted.is_some());
        assert!(contrast_ratio(adjusted.unwrap(), bg) >= MIN_READABLE_CONTRAST);
    }

    #[test]
    fn adjust_lightens_dark_fg_on_dark_bg() {
        let fg = Color::rgb(30, 30, 60);
        let bg = Color::rgb(10, 10, 30);
        let adjusted = adjust_for_contrast(fg, bg, MIN_READABLE_CONTRAST);
        assert!(adjusted.is_some());
        assert!(contrast_ratio(adjusted.unwrap(), bg) >= MIN_READABLE_CONTRAST);
    }

    #[test]
    fn adjust_preserves_hue_approximately() {
        let fg = Color::rgb(100, 50, 50); // reddish
        let bg = Color::rgb(80, 30, 30); // dark reddish
        let adjusted = adjust_for_contrast(fg, bg, MIN_READABLE_CONTRAST).unwrap();
        let (orig_h, _, _) = rgb_to_hsl(100, 50, 50);
        let (adj_r, adj_g, adj_b) = adjusted.to_rgb().unwrap();
        let (adj_h, _, _) = rgb_to_hsl(adj_r, adj_g, adj_b);
        let hue_diff = (orig_h - adj_h).abs();
        assert!(
            !(5.0..=355.0).contains(&hue_diff),
            "Hue shifted too much: {orig_h} → {adj_h}"
        );
    }

    #[test]
    fn adjusts_near_threshold_without_jumping_to_extreme() {
        let bg = Color::rgb(0, 100, 0);
        let adjusted = adjust_for_contrast(Color::rgb(0, 180, 0), bg, MIN_READABLE_CONTRAST)
            .expect("green should need only a moderate adjustment on dark green");

        let Color::Rgb(r, g, b) = adjusted else {
            panic!("expected truecolor adjustment, got {adjusted:?}");
        };

        assert_eq!((r, b), (0, 0));
        assert!(g > 180, "expected some lightening, got {adjusted:?}");
        assert!(
            g < 255,
            "adjustment should not jump to max lightness: {adjusted:?}"
        );
        assert!(contrast_ratio(adjusted, bg) >= MIN_READABLE_CONTRAST);
    }

    // --- complementary_color -----------------------------------------------

    #[test]
    fn complementary_rotates_hue() {
        assert_eq!(
            complementary_color(Color::rgb(255, 0, 0)),
            Some(Color::rgb(0, 255, 255))
        );
    }

    // --- inverse_color -----------------------------------------------------

    #[test]
    fn inverse_of_black_is_white() {
        assert_eq!(inverse_color(Color::Black), Some(Color::rgb(255, 255, 255)));
    }

    // --- readable_style ----------------------------------------------------

    #[test]
    fn readable_style_adjusts_fg() {
        let style = Style::new().fg(Color::Black).bg(Color::rgb(0, 0, 200));
        let result = readable_style(style);
        assert!(
            contrast_ratio(result.fg.unwrap().color(), result.bg.unwrap().color())
                >= MIN_READABLE_CONTRAST
        );
    }

    #[test]
    fn readable_style_keeps_empty_bg_unchanged() {
        let style = Style::new().fg(Color::Yellow);
        assert_eq!(readable_style(style), style);
    }

    #[test]
    fn readable_style_black_or_white_snaps_to_binary_choice() {
        let style = Style::new().fg(Color::White).bg(Color::rgb(157, 124, 216));
        let result = readable_style_black_or_white(style);
        assert_eq!(result.fg, Some(Paint::from(Color::rgb(0, 0, 0))));
        assert_eq!(result.bg, Some(Paint::from(Color::rgb(157, 124, 216))));
    }

    #[test]
    fn readable_style_clears_bg_transform_when_bg_missing() {
        use crate::style::ColorTransform;
        // A hover_style-style overlay: transform without a concrete bg.
        // resolve_color_transforms wipes transform-only channels when color is None,
        // and readable_style should preserve that behavior.
        let overlay = Style::new()
            .fg(Color::White)
            .transform_bg(ColorTransform::Lighten(0.3));
        let result = readable_style(overlay);
        assert_eq!(result.bg_transform, None);

        let base = Style::new().fg(Color::Black).bg(Color::rgb(40, 60, 200));
        let merged = readable_style(base).patch(result);
        let expected_bg = Color::rgb(40, 60, 200);
        assert_eq!(merged.bg, Some(Paint::from(expected_bg)));
    }

    #[test]
    fn readable_style_does_not_invent_fg() {
        let style = Style::new().bg(Color::rgb(10, 20, 30));
        assert_eq!(readable_style(style).fg, None);
    }

    #[test]
    fn readable_style_reset_bg_preserves_fg() {
        let style = Style::new().fg(Color::Red).bg(Color::Reset);
        assert_eq!(readable_style(style).fg, Some(Paint::from(Color::Red)));

        let style = Style::new().fg(Color::Black).bg(Color::Reset);
        assert_eq!(readable_style(style).fg, Some(Paint::from(Color::Black)));
    }

    // --- HSL round-trip ----------------------------------------------------

    #[test]
    fn hsl_round_trip_primary_colors() {
        for (r, g, b) in [(255, 0, 0), (0, 255, 0), (0, 0, 255), (0, 0, 0)] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let Color::Rgb(rr, rg, rb) = hsl_to_color(h, s, l) else {
                panic!("expected Rgb");
            };
            assert!(
                (r as i16 - rr as i16).unsigned_abs() <= 1
                    && (g as i16 - rg as i16).unsigned_abs() <= 1
                    && (b as i16 - rb as i16).unsigned_abs() <= 1,
                "Round-trip failed: ({r},{g},{b}) → ({rr},{rg},{rb})"
            );
        }
    }

    // --- APCA contrast -----------------------------------------------------

    #[test]
    fn apca_black_white_has_high_contrast() {
        let lc = apca_contrast(Color::Black, Color::White);
        assert!(lc > 100.0, "Black on white should have Lc > 100, got {lc}");
    }

    #[test]
    fn apca_polarity_is_asymmetric() {
        let dark_on_light = apca_contrast(Color::Black, Color::White);
        let light_on_dark = apca_contrast(Color::White, Color::Black);
        assert!(dark_on_light > 0.0, "Dark-on-light should be positive");
        assert!(light_on_dark < 0.0, "Light-on-dark should be negative");
    }

    #[test]
    fn apca_same_color_is_zero() {
        let lc = apca_contrast(Color::Blue, Color::Blue);
        assert!(
            lc.abs() < 1.0,
            "Same color should have ~0 contrast, got {lc}"
        );
    }

    #[test]
    fn apca_readable_text_meets_threshold() {
        let bg = Color::rgb(0, 0, 128);
        let chosen = readable_text_color_apca(Some(Color::rgb(180, 180, 180)), bg);
        let lc = apca_contrast(chosen, bg).abs();
        assert!(
            lc >= MIN_APCA_BODY_CONTRAST,
            "Chosen {chosen:?} has Lc {lc}, need >= {MIN_APCA_BODY_CONTRAST}"
        );
    }

    #[test]
    fn apca_reset_bg_preserves_preferred() {
        let chosen = readable_text_color_apca(Some(Color::Red), Color::Reset);
        assert_eq!(chosen, Color::Red);
    }

    #[test]
    fn apca_no_preferred_picks_black_or_white() {
        let on_dark = readable_text_color_apca(None, Color::rgb(20, 20, 40));
        assert_eq!(on_dark, Color::rgb(255, 255, 255));
        let on_light = readable_text_color_apca(None, Color::rgb(240, 240, 220));
        assert_eq!(on_light, Color::rgb(0, 0, 0));
    }

    #[test]
    fn apca_no_preferred_on_ansi_bg_returns_ansi_black_or_white() {
        assert_eq!(readable_text_color_apca(None, Color::Blue), Color::White);
        assert_eq!(readable_text_color_apca(None, Color::White), Color::Black);
    }

    #[test]
    fn apca_no_preferred_on_indexed_bg_returns_indexed_black_or_white() {
        assert_eq!(
            readable_text_color_apca(None, Color::indexed(4)),
            Color::indexed(15)
        );
        assert_eq!(
            readable_text_color_apca(None, Color::indexed(15)),
            Color::indexed(0)
        );
    }

    #[test]
    fn apca_style_adjusts_fg() {
        let style = Style::new().fg(Color::Black).bg(Color::rgb(0, 0, 200));
        let result = readable_style_apca(style);
        let lc = apca_contrast(result.fg.unwrap().color(), result.bg.unwrap().color()).abs();
        assert!(lc >= MIN_APCA_BODY_CONTRAST, "Lc = {lc}");
    }

    #[test]
    fn apca_adjusts_near_threshold_without_jumping_to_extreme() {
        let bg = Color::rgb(20, 20, 40);
        let adjusted = adjust_for_apca_contrast(Color::Green, bg, MIN_APCA_BODY_CONTRAST)
            .expect("green should need only a slight adjustment on dark blue-gray");

        let Color::Rgb(r, g, b) = adjusted else {
            panic!("expected truecolor adjustment, got {adjusted:?}");
        };

        assert_eq!((r, b), (0, 0));
        assert!(g > 205, "expected a slight lightening, got {adjusted:?}");
        assert!(
            g < 255,
            "adjustment should not jump to max lightness: {adjusted:?}"
        );
        assert!(apca_contrast(adjusted, bg).abs() >= MIN_APCA_BODY_CONTRAST);
    }
}
