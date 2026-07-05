//! Public frame-capture types for snapshot and visual tests.

use std::fmt::Write;
#[cfg(feature = "ui-snapshot-png")]
use std::path::PathBuf;
#[cfg(feature = "ui-snapshot-png")]
use std::sync::Arc;

use crate::style::ansi::write_cell_style_sgr;
use crate::style::{Color, Rect, Style};

#[cfg(feature = "ui-snapshot-png")]
mod png;

/// Captured terminal cell data converted to crate-owned style primitives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedCell {
    /// Rendered symbol at this cell.
    pub symbol: String,
    /// Foreground color.
    pub fg: Color,
    /// Background color.
    pub bg: Color,
    /// Underline color.
    pub underline_color: Color,
    /// Text modifiers active on this cell.
    pub modifiers: CellModifiers,
}

/// Boolean modifier flags extracted from a rendered terminal cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellModifiers {
    /// Bold text.
    pub bold: bool,
    /// Dim text.
    pub dim: bool,
    /// Italic text.
    pub italic: bool,
    /// Underlined text.
    pub underline: bool,
    /// Reverse-video text.
    pub reverse: bool,
    /// Strikethrough text.
    pub strikethrough: bool,
}

/// Cursor metadata captured from a rendered frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorState {
    /// Cursor column.
    pub x: u16,
    /// Cursor row.
    pub y: u16,
    /// Whether the cursor should be shown.
    pub visible: bool,
}

/// Complete frame snapshot produced by [`crate::TestBackend::capture_frame`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedFrame {
    /// Viewport used for render/layout.
    pub viewport: Rect,
    /// Captured frame width.
    pub width: u16,
    /// Captured frame height.
    pub height: u16,
    /// Flattened row-major cell buffer (`width * height`).
    pub cells: Vec<CapturedCell>,
    /// Cursor state, when a widget requested cursor placement.
    pub cursor: Option<CursorState>,
}

/// Options for rendering a [`CapturedFrame`] as PNG bytes.
#[cfg(feature = "ui-snapshot-png")]
#[derive(Clone, Debug)]
pub struct PngOptions {
    /// Base width of each terminal cell before applying [`Self::scale`].
    ///
    /// Defaults to `8`; zero is clamped to `1` while rendering.
    pub cell_width: u16,
    /// Base height of each terminal cell before applying [`Self::scale`].
    ///
    /// Defaults to `16`; zero is clamped to `1` while rendering.
    pub cell_height: u16,
    /// Multiplier applied to cell dimensions while rendering.
    ///
    /// Defaults to `2`; zero is clamped to `1` while rendering.
    pub scale: u16,
    /// Foreground color used when a cell foreground resolves to reset or transparent.
    pub default_fg: Color,
    /// Background color used when a cell background resolves to reset, transparent, or backdrop.
    pub default_bg: Color,
    /// Whether to draw a cursor outline when the captured frame contains a visible cursor.
    ///
    /// Defaults to `true`.
    pub render_cursor: bool,
    /// Text renderer used for non-box/block glyphs.
    ///
    /// Defaults to [`PngTextRenderer::Auto`], which tries a system font and falls back to bitmap.
    pub text_renderer: PngTextRenderer,
    /// Preferred system font family for font-backed rendering.
    pub font_family: Option<Arc<str>>,
    /// Explicit font file path for font-backed rendering.
    pub font_path: Option<PathBuf>,
}

/// Text renderer used when converting a captured frame to PNG bytes.
#[cfg(feature = "ui-snapshot-png")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PngTextRenderer {
    /// Try font-backed rendering and fall back to bitmap rendering when no font is available.
    #[default]
    Auto,
    /// Try font-backed rendering and fall back to bitmap rendering when loading or glyph lookup fails.
    Font,
    /// Always use the built-in bitmap font renderer.
    Bitmap,
}

#[cfg(feature = "ui-snapshot-png")]
impl Default for PngOptions {
    fn default() -> Self {
        Self {
            cell_width: 8,
            cell_height: 16,
            scale: 2,
            default_fg: Color::White,
            default_bg: Color::Black,
            render_cursor: true,
            text_renderer: PngTextRenderer::Auto,
            font_family: None,
            font_path: None,
        }
    }
}

impl CapturedCell {
    fn style(&self) -> Style {
        Style {
            fg: Some(self.fg.into()),
            bg: Some(self.bg.into()),
            fg_transform: None,
            bg_transform: None,
            contrast_policy: None,
            bold: Some(self.modifiers.bold),
            dim: Some(self.modifiers.dim),
            italic: Some(self.modifiers.italic),
            underline: Some(self.modifiers.underline),
            reverse: Some(self.modifiers.reverse),
            strikethrough: Some(self.modifiers.strikethrough),
            underline_color: Some(self.underline_color.into()),
            dim_amount: None,
            tint: None,
        }
    }

    /// Write ANSI SGR sequences for this cell's style into `out`.
    pub fn write_ansi_style(&self, out: &mut String) {
        let m = &self.modifiers;
        write_cell_style_sgr(
            out,
            crate::style::ansi::CellStyleSgr {
                fg: self.fg,
                bg: self.bg,
                underline_color: self.underline_color,
                bold: m.bold,
                dim: m.dim,
                italic: m.italic,
                underline: m.underline,
                reverse: m.reverse,
                strikethrough: m.strikethrough,
            },
        );
    }

    /// Returns true when this cell's visual style matches `other`.
    pub fn ansi_style_matches(&self, other: &CapturedCell) -> bool {
        self.fg == other.fg
            && self.bg == other.bg
            && self.underline_color == other.underline_color
            && self.modifiers == other.modifiers
    }
}

impl CapturedFrame {
    /// Encode this frame as a PNG byte buffer.
    ///
    /// With [`PngTextRenderer::Auto`], text is rendered with a discovered system
    /// font when available and falls back to the built-in bitmap renderer.
    ///
    /// If PNG encoding fails, returns an empty buffer. Use [`Self::try_to_png`] when the
    /// encoding error should be surfaced to the caller.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn to_png(&self, options: &PngOptions) -> Vec<u8> {
        png::encode_frame(self, options)
    }

    /// Encode this frame as PNG bytes and return any encoder error.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn try_to_png(&self, options: &PngOptions) -> crate::Result<Vec<u8>> {
        png::try_encode_frame(self, options)
            .map_err(|err| std::io::Error::other(err.to_string()).into())
    }

    /// Returns captured text as newline-joined rows with trailing spaces trimmed.
    pub fn plain_text(&self) -> String {
        self.to_lines().join("\n")
    }

    /// Returns captured rows at full viewport width without trimming trailing spaces.
    pub fn to_fixed_grid(&self) -> String {
        self.to_fixed_grid_lines().join("\n")
    }

    /// Returns captured rows at full viewport width without trimming trailing spaces.
    pub fn to_fixed_grid_lines(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| {
                let mut line = String::new();
                for cell in self.row(y) {
                    line.push_str(&cell.symbol);
                }
                line
            })
            .collect()
    }

    /// Returns the frame rendered as a static ANSI string (full terminal repaint prelude).
    pub fn to_ansi(&self) -> String {
        self.to_ansi_diff(None)
    }

    /// Returns an ANSI string updating `prev` to this frame.
    ///
    /// When `prev` is `None` or dimensions differ, emits a full clear + home cursor prelude
    /// suitable for terminal backends (including xterm.js).
    pub fn to_ansi_diff(&self, prev: Option<&CapturedFrame>) -> String {
        let full_repaint = prev
            .map(|p| p.width != self.width || p.height != self.height)
            .unwrap_or(true);
        let mut out = String::with_capacity(if full_repaint {
            usize::from(self.width) * usize::from(self.height) * 12 + 64
        } else {
            usize::from(self.width) * usize::from(self.height) * 3 + 512
        });

        if full_repaint {
            out.push_str("\x1b[2J\x1b[3J\x1b[H\x1b[?25l");
        }

        let w = usize::from(self.width);
        let mut last_written: Option<(u16, u16)> = None;
        let mut last_styled_cell: Option<&CapturedCell> = None;
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = usize::from(y) * w + usize::from(x);
                let cell = &self.cells[idx];
                if !full_repaint
                    && let Some(prev_frame) = prev
                    && prev_frame.cells[idx] == *cell
                {
                    continue;
                }
                let contiguous = matches!(last_written, Some((lx, ly)) if ly == y && lx + 1 == x);
                if !contiguous {
                    let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
                }
                if !(contiguous
                    && last_styled_cell.is_some_and(|prev_cell| prev_cell.ansi_style_matches(cell)))
                {
                    cell.write_ansi_style(&mut out);
                    last_styled_cell = Some(cell);
                }
                out.push_str(&cell.symbol);
                last_written = Some((x, y));
            }
        }
        if let Some(c) = self.cursor.as_ref().filter(|c| c.visible) {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", c.y + 1, c.x + 1);
        } else {
            out.push_str("\x1b[?25l");
        }
        out
    }

    /// Returns captured rows with trailing spaces trimmed per row.
    pub fn to_lines(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| {
                let mut line = String::new();
                for cell in self.row(y) {
                    line.push_str(&cell.symbol);
                }
                line.trim_end_matches(' ').to_owned()
            })
            .collect()
    }

    /// Returns all cells for row `y`.
    ///
    /// Panics if `y >= self.height`.
    pub fn row(&self, y: u16) -> &[CapturedCell] {
        assert!(y < self.height, "row y out of bounds");
        let start = usize::from(y) * usize::from(self.width);
        let end = start + usize::from(self.width);
        &self.cells[start..end]
    }

    /// Returns a single cell at `(x, y)`.
    ///
    /// Panics if `x >= self.width` or `y >= self.height`.
    pub fn cell(&self, x: u16, y: u16) -> &CapturedCell {
        assert!(x < self.width, "cell x out of bounds");
        &self.row(y)[usize::from(x)]
    }

    /// Returns each row grouped into contiguous `(text, style)` runs.
    pub fn styled_lines(&self) -> Vec<Vec<(String, Style)>> {
        (0..self.height)
            .map(|y| {
                let mut runs: Vec<(String, Style)> = Vec::new();

                for cell in self.row(y) {
                    let style = cell.style();
                    if let Some((text, run_style)) = runs.last_mut()
                        && *run_style == style
                    {
                        text.push_str(&cell.symbol);
                    } else {
                        runs.push((cell.symbol.clone(), style));
                    }
                }

                runs
            })
            .collect()
    }
}
