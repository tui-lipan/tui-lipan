//! QR code widget.

use std::sync::Arc;

use crate::core::element::{Element, IntoElement};
use crate::style::{Color, Length, Style};
use crate::widgets::{Overflow, Spacer, Text};

/// Largest quiet zone accepted by [`QrCode::quiet_zone`], in modules.
///
/// The ISO/IEC 18004 minimum is 4; anything past a handful of modules only
/// wastes cells, so the setter saturates here rather than letting a stray
/// value inflate the symbol past the terminal.
const MAX_QUIET_ZONE: u16 = 32;

/// Error correction level for a [`QrCode`].
///
/// Higher levels survive more damage (and more terminal rendering artifacts)
/// but need a larger symbol for the same payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QrEcc {
    /// Recovers roughly 7% of the symbol. Smallest output.
    Low,
    /// Recovers roughly 15% of the symbol.
    #[default]
    Medium,
    /// Recovers roughly 25% of the symbol.
    Quartile,
    /// Recovers roughly 30% of the symbol. Largest output.
    High,
}

impl QrEcc {
    fn to_ec_level(self) -> qrcode::EcLevel {
        match self {
            Self::Low => qrcode::EcLevel::L,
            Self::Medium => qrcode::EcLevel::M,
            Self::Quartile => qrcode::EcLevel::Q,
            Self::High => qrcode::EcLevel::H,
        }
    }
}

/// How QR modules are mapped onto terminal cells.
///
/// Terminal cells are roughly twice as tall as they are wide, so a naive
/// one-cell-per-module symbol comes out at a 1:2 aspect ratio that most
/// scanners reject. Both variants here correct for that; they differ only in
/// how much space they trade for module size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum QrRender {
    /// One cell per module wide, two module rows per cell row, drawn with
    /// half-block glyphs (`▀`, `▄`, `█`).
    ///
    /// The compact option: a symbol of `n` modules occupies `n` columns and
    /// `n / 2` rows.
    #[default]
    HalfBlock,
    /// Two cells per module wide, one module row per cell row.
    ///
    /// Physically twice the size of [`HalfBlock`](Self::HalfBlock), which reads
    /// more reliably on low-resolution cameras, at the cost of `2 * n` columns.
    Wide,
}

/// A scannable QR code rendered as terminal cells.
///
/// Encodes `data` into a QR symbol and paints it with block glyphs. The common
/// uses are handing something off to a phone: device-flow login URLs, WiFi
/// credentials, TOTP enrollment secrets, or a link the user wants on another
/// screen.
///
/// # Sizing
///
/// Unlike every other widget, a QR symbol cannot reflow: its size is fixed by
/// the payload length and error correction level. A symbol that gets clipped
/// still *looks* like a QR code but will not scan, so check [`QrCode::size`]
/// against the viewport and substitute a fallback when the terminal is too
/// small:
///
/// ```no_run
/// # use tui_lipan::prelude::*;
/// # fn example(ctx: &Context<impl Component>) -> Element {
/// let qr = QrCode::new("https://tui-lipan.dev");
/// let viewport = ctx.viewport();
///
/// match qr.size() {
///     Some((w, h)) if w <= viewport.w && h <= viewport.h => qr.into(),
///     _ => Text::new("https://tui-lipan.dev").into(),
/// }
/// # }
/// ```
///
/// # Contrast
///
/// Scanners expect dark modules on a light background, so the default styling
/// paints explicit [`Color::Black`] on [`Color::White`] rather than inheriting
/// the terminal palette — a symbol rendered on a dark background is inverted
/// and many readers will not decode it. Use [`QrCode::invert`] only when you
/// know the target scanner handles it.
///
/// ```
/// # use tui_lipan::prelude::*;
/// QrCode::new("https://tui-lipan.dev")
///     .ecc(QrEcc::Quartile)
///     .render(QrRender::Wide);
/// ```
#[derive(Clone)]
pub struct QrCode {
    data: Arc<str>,
    ecc: QrEcc,
    render: QrRender,
    quiet_zone: u16,
    dark: Color,
    light: Color,
    fallback: Option<Element>,
}

impl QrCode {
    /// Create a QR code for the given payload.
    pub fn new(data: impl Into<Arc<str>>) -> Self {
        Self {
            data: data.into(),
            ecc: QrEcc::default(),
            render: QrRender::default(),
            quiet_zone: 4,
            dark: Color::Black,
            light: Color::White,
            fallback: None,
        }
    }

    /// Set the error correction level.
    pub fn ecc(mut self, ecc: QrEcc) -> Self {
        self.ecc = ecc;
        self
    }

    /// Set how modules map onto terminal cells.
    pub fn render(mut self, render: QrRender) -> Self {
        self.render = render;
        self
    }

    /// Set the light margin around the symbol, in modules.
    ///
    /// Defaults to the spec-mandated 4. Scanners need this border to locate the
    /// symbol; dropping it below 4 trades reliability for space. Values are
    /// capped at 32.
    pub fn quiet_zone(mut self, modules: u16) -> Self {
        self.quiet_zone = modules.min(MAX_QUIET_ZONE);
        self
    }

    /// Set the color of dark modules.
    pub fn dark(mut self, color: Color) -> Self {
        self.dark = color;
        self
    }

    /// Set the color of light modules and the quiet zone.
    pub fn light(mut self, color: Color) -> Self {
        self.light = color;
        self
    }

    /// Swap the dark and light colors.
    pub fn invert(mut self) -> Self {
        std::mem::swap(&mut self.dark, &mut self.light);
        self
    }

    /// Set the element rendered when the payload is too long to encode.
    ///
    /// Without one, a payload that exceeds the largest QR version renders
    /// nothing.
    pub fn fallback(mut self, fallback: impl IntoElement) -> Self {
        self.fallback = Some(fallback.into());
        self
    }

    /// Symbol width in modules, excluding the quiet zone.
    ///
    /// Returns `None` when the payload is too long to encode.
    pub fn module_count(&self) -> Option<u16> {
        encode(&self.data, self.ecc).map(|(modules, _)| modules)
    }

    /// Cell footprint as `(width, height)`, including the quiet zone.
    ///
    /// Returns `None` when the payload is too long to encode. Compare this
    /// against the available space before rendering — a clipped symbol does not
    /// scan.
    pub fn size(&self) -> Option<(u16, u16)> {
        let total = self.module_count()?.saturating_add(self.quiet_zone * 2);
        Some(match self.render {
            QrRender::HalfBlock => (total, total.div_ceil(2)),
            QrRender::Wide => (total.saturating_mul(2), total),
        })
    }

    fn fallback_element(self) -> Element {
        self.fallback.unwrap_or_else(|| {
            Spacer::new()
                .width(Length::Px(0))
                .height(Length::Px(0))
                .into()
        })
    }
}

/// Encode `data` into `(module_count, dark_flags)`, row-major.
fn encode(data: &str, ecc: QrEcc) -> Option<(u16, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(data, ecc.to_ec_level()).ok()?;
    let modules = u16::try_from(code.width()).ok()?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    Some((modules, dark))
}

/// Paint the symbol as newline-separated rows of block glyphs.
///
/// Every glyph draws dark on light, so a single foreground/background pair
/// styles the whole symbol: `█` is two dark modules, `▀` and `▄` are one of
/// each, and a space is two light modules.
fn paint(modules: u16, dark: &[bool], quiet_zone: u16, render: QrRender) -> String {
    let n = modules as usize;
    let quiet = quiet_zone as usize;
    let total = n + quiet * 2;

    // Coordinates outside the symbol land in the quiet zone, which is light.
    let is_dark = |x: usize, y: usize| -> bool {
        if x < quiet || y < quiet || x >= quiet + n || y >= quiet + n {
            return false;
        }
        dark[(y - quiet) * n + (x - quiet)]
    };

    match render {
        QrRender::HalfBlock => {
            let rows = total.div_ceil(2);
            let mut out = String::with_capacity(rows * (total + 1));
            for row in 0..rows {
                if row > 0 {
                    out.push('\n');
                }
                let (top_y, bottom_y) = (row * 2, row * 2 + 1);
                for x in 0..total {
                    // An odd `total` leaves the final bottom row unpaired; it
                    // reads as light, which only widens the quiet zone.
                    let top = is_dark(x, top_y);
                    let bottom = bottom_y < total && is_dark(x, bottom_y);
                    out.push(match (top, bottom) {
                        (true, true) => '█',
                        (true, false) => '▀',
                        (false, true) => '▄',
                        (false, false) => ' ',
                    });
                }
            }
            out
        }
        QrRender::Wide => {
            let mut out = String::with_capacity(total * (total * 2 + 1));
            for y in 0..total {
                if y > 0 {
                    out.push('\n');
                }
                for x in 0..total {
                    out.push_str(if is_dark(x, y) { "██" } else { "  " });
                }
            }
            out
        }
    }
}

impl From<QrCode> for Element {
    fn from(qr: QrCode) -> Self {
        let Some((modules, dark)) = encode(&qr.data, qr.ecc) else {
            return qr.fallback_element();
        };

        let content = paint(modules, &dark, qr.quiet_zone, qr.render);

        Text::new(content)
            .style(Style::new().fg(qr.dark).bg(qr.light))
            // Wrapping would shear the symbol into unscannable fragments.
            .overflow(Overflow::Clip)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rebuild the module grid from painted half-block output.
    fn grid_from_half_block(painted: &str, total: usize) -> Vec<Vec<bool>> {
        let mut grid = vec![vec![false; total]; total];
        for (row, line) in painted.lines().enumerate() {
            for (x, glyph) in line.chars().enumerate() {
                let (top, bottom) = match glyph {
                    '█' => (true, true),
                    '▀' => (true, false),
                    '▄' => (false, true),
                    ' ' => (false, false),
                    other => panic!("unexpected glyph {other:?}"),
                };
                if let Some(cell) = grid.get_mut(row * 2).and_then(|r| r.get_mut(x)) {
                    *cell = top;
                }
                if let Some(cell) = grid.get_mut(row * 2 + 1).and_then(|r| r.get_mut(x)) {
                    *cell = bottom;
                }
            }
        }
        grid
    }

    #[test]
    fn half_block_size_is_half_as_tall_as_wide() {
        let qr = QrCode::new("https://tui-lipan.dev");
        let modules = qr.module_count().expect("encodes");
        let total = modules + 8;

        assert_eq!(qr.size(), Some((total, total.div_ceil(2))));
    }

    #[test]
    fn wide_size_is_twice_as_wide_as_tall() {
        let qr = QrCode::new("https://tui-lipan.dev").render(QrRender::Wide);
        let modules = qr.module_count().expect("encodes");
        let total = modules + 8;

        assert_eq!(qr.size(), Some((total * 2, total)));
    }

    #[test]
    fn painted_output_matches_reported_size() {
        for render in [QrRender::HalfBlock, QrRender::Wide] {
            let qr = QrCode::new("https://tui-lipan.dev").render(render);
            let (modules, dark) = encode(&qr.data, qr.ecc).expect("encodes");
            let painted = paint(modules, &dark, qr.quiet_zone, render);
            let (w, h) = qr.size().expect("encodes");

            assert_eq!(painted.lines().count(), h as usize, "{render:?} height");
            for line in painted.lines() {
                assert_eq!(line.chars().count(), w as usize, "{render:?} width");
            }
        }
    }

    #[test]
    fn painted_modules_round_trip_through_half_blocks() {
        let qr = QrCode::new("https://tui-lipan.dev");
        let (modules, dark) = encode(&qr.data, qr.ecc).expect("encodes");
        let quiet = qr.quiet_zone as usize;
        let n = modules as usize;
        let total = n + quiet * 2;

        let grid = grid_from_half_block(&paint(modules, &dark, qr.quiet_zone, qr.render), total);

        for y in 0..n {
            for x in 0..n {
                assert_eq!(
                    grid[y + quiet][x + quiet],
                    dark[y * n + x],
                    "module ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn quiet_zone_stays_light_on_every_edge() {
        let qr = QrCode::new("https://tui-lipan.dev");
        let (modules, dark) = encode(&qr.data, qr.ecc).expect("encodes");
        let quiet = qr.quiet_zone as usize;
        let total = modules as usize + quiet * 2;

        let grid = grid_from_half_block(&paint(modules, &dark, qr.quiet_zone, qr.render), total);

        for (y, row) in grid.iter().enumerate() {
            for (x, &cell) in row.iter().enumerate() {
                let inside = x >= quiet && y >= quiet && x < total - quiet && y < total - quiet;
                assert!(inside || !cell, "quiet zone dark at ({x}, {y})");
            }
        }
    }

    #[test]
    fn odd_total_keeps_the_unpaired_row_light() {
        // A quiet zone of 3 makes `total` odd for any odd module count.
        let qr = QrCode::new("https://tui-lipan.dev").quiet_zone(3);
        let (modules, dark) = encode(&qr.data, qr.ecc).expect("encodes");
        let total = modules as usize + 6;
        assert_eq!(total % 2, 1, "expected an odd total for this fixture");

        let painted = paint(modules, &dark, qr.quiet_zone, qr.render);
        let last = painted.lines().next_back().expect("has rows");

        assert!(
            last.chars().all(|glyph| matches!(glyph, '▀' | ' ')),
            "unpaired bottom row painted dark: {last:?}"
        );
    }

    #[test]
    fn higher_error_correction_grows_the_symbol() {
        let low = QrCode::new("https://tui-lipan.dev").ecc(QrEcc::Low);
        let high = QrCode::new("https://tui-lipan.dev").ecc(QrEcc::High);

        assert!(high.module_count() > low.module_count());
    }

    #[test]
    fn oversized_payload_reports_no_size() {
        let qr = QrCode::new("x".repeat(8000));

        assert_eq!(qr.module_count(), None);
        assert_eq!(qr.size(), None);
    }

    #[test]
    fn quiet_zone_saturates() {
        let qr = QrCode::new("https://tui-lipan.dev").quiet_zone(u16::MAX);

        assert_eq!(qr.quiet_zone, MAX_QUIET_ZONE);
    }

    #[test]
    fn invert_swaps_colors() {
        let qr = QrCode::new("https://tui-lipan.dev").invert();

        assert_eq!(qr.dark, Color::White);
        assert_eq!(qr.light, Color::Black);
    }
}
