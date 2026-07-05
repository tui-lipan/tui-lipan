use figlet_rs::FIGlet;
use font8x8::{BASIC_FONTS, UnicodeFonts};
use std::sync::{Arc, OnceLock};

use crate::core::element::{Element, ElementKind};
use crate::style::{RichText, Span, Style};
use crate::utils::gradient::{ColorGradient, GradientDirection};

pub(crate) use node::BigTextCacheKey;

mod font;
mod layout;
mod node;
mod reconcile;

pub use layout::{GlyphLayout, measure_big_text};
pub use node::BigTextNode;
pub use reconcile::reconcile_big_text;

use self::font::{
    ANSI_SHADOW_FONT, BLOODY_FONT, COLOSSAL_FONT, DOS_REBEL_FONT, NANCYJ_FONT, POISON_FONT,
    ROMAN_FONT, SLANT_FONT, SMALL_FONT, SMALL_POISON_FONT, STANDARD_FONT, SUB_ZERO_FONT,
};

// Cached parsed FIGlet objects to avoid expensive parsing on every render.
// Wrapped in Option so that load failures are cached too (no repeated retries).
static FIGFONT_STANDARD: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_SLANT: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_BLOODY: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_COLOSSAL: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_ROMAN: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_SUB_ZERO: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_POISON: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_NANCYJ: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_SMALL_POISON: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_DOS_REBEL: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_ANSI_SHADOW: OnceLock<Option<FIGlet>> = OnceLock::new();
static FIGFONT_SMALL: OnceLock<Option<FIGlet>> = OnceLock::new();

// Cached custom FIGlet fonts to avoid repeated parsing.
static CUSTOM_FIGFONT_CACHE: OnceLock<std::sync::Mutex<CustomFigletCache>> = OnceLock::new();

/// Font style for BigText.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BigFont {
    /// Standard FIGlet font.
    #[default]
    Standard,
    /// 8-bit Pixel Art font (using half-blocks).
    /// Renders text using `▀`, `▄`, `█` to simulate high-resolution pixels.
    Pixel,
    /// Blocky 8-bit font (algorithmic bold).
    PixelBold,
    /// High-resolution Quadrant font (2x2 blocks).
    Quadrant,
    /// Slant FIGlet font - italic style.
    Slant,
    /// Bloody FIGlet font - horror style with dripping effect.
    Bloody,
    /// Colossal FIGlet font - very large block letters.
    Colossal,
    /// Roman FIGlet font - classic roman style.
    Roman,
    /// Sub-Zero FIGlet font - clean geometric style.
    SubZero,
    /// Poison FIGlet font - stylized dripping text.
    Poison,
    /// Nancyj FIGlet font - decorative style.
    Nancyj,
    /// Small Poison FIGlet font - compact poison style.
    SmallPoison,
    /// DOS Rebel FIGlet font - retro DOS style.
    DosRebel,
    /// ANSI Shadow FIGlet font - shadow effect using box-drawing characters.
    AnsiShadow,
    /// Small FIGlet font - compact version of Standard.
    Small,
    /// Custom FIGlet font loaded from an `.flf` file.
    CustomFiglet,
}

/// Shadow configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shadow {
    /// Style of the shadow characters.
    pub style: Style,
    /// Horizontal offset.
    pub offset_x: i16,
    /// Vertical offset.
    pub offset_y: i16,
}

/// A widget that renders text using ASCII art.
#[derive(Clone)]
pub struct BigText {
    /// The text to render.
    pub text: RichText,
    /// The font to use.
    pub font: BigFont,
    /// Style of the main text.
    pub style: Style,
    /// Optional shadow configuration.
    pub shadow: Option<Shadow>,
    /// Optional custom FIGlet font content.
    pub custom_figlet: Option<Arc<str>>,
    /// Optional color gradient applied at render time (not cached).
    pub gradient: Option<(ColorGradient, GradientDirection)>,
}

// Cached FIGlet rendering results to avoid redundant expensive computations.
static BIG_TEXT_CACHE: OnceLock<std::sync::Mutex<BigTextVisualCache>> = OnceLock::new();

#[derive(Clone, Debug)]
pub(crate) struct BigTextVisualCache {
    entries: Vec<(BigTextCacheKey, Arc<BigTextRenderOutput>)>,
}

struct CustomFigletCache {
    entries: Vec<(Arc<str>, Arc<FIGlet>)>,
}

impl CustomFigletCache {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&self, content: &Arc<str>) -> Option<Arc<FIGlet>> {
        self.entries
            .iter()
            .find(|(k, _)| k.as_ref() == content.as_ref())
            .map(|(_, v)| Arc::clone(v))
    }

    fn insert(&mut self, content: Arc<str>, font: Arc<FIGlet>) {
        if let Some(idx) = self
            .entries
            .iter()
            .position(|(k, _)| k.as_ref() == content.as_ref())
        {
            self.entries.remove(idx);
        }
        self.entries.push((content, font));
        if self.entries.len() > 32 {
            self.entries.remove(0);
        }
    }
}

impl BigTextVisualCache {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&self, key: &BigTextCacheKey) -> Option<Arc<BigTextRenderOutput>> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| Arc::clone(v))
    }

    fn insert(&mut self, key: BigTextCacheKey, value: Arc<BigTextRenderOutput>) {
        if let Some(idx) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(idx);
        }
        self.entries.push((key, value));
        if self.entries.len() > 100 {
            self.entries.remove(0);
        }
    }
}

#[derive(Debug)]
pub(crate) struct BigTextRenderOutput {
    pub lines: Vec<Vec<Span>>,
    pub width: u16,
    pub height: u16,
}

impl Default for BigText {
    fn default() -> Self {
        Self::new()
    }
}

impl BigText {
    /// Create a new BigText widget with default settings.
    pub fn new() -> Self {
        Self {
            text: RichText::new(),
            font: BigFont::Standard,
            style: Style::default(),
            shadow: None,
            custom_figlet: None,
            gradient: None,
        }
    }

    /// Set the text content.
    pub fn text(mut self, text: impl Into<RichText>) -> Self {
        self.text = text.into();
        self
    }

    /// Set the font.
    pub fn font(mut self, font: BigFont) -> Self {
        self.font = font;
        self
    }

    /// Set the text style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the shadow.
    pub fn shadow(mut self, shadow: impl Into<Option<Shadow>>) -> Self {
        self.shadow = shadow.into();
        self
    }

    /// Set a custom FIGlet font from `.flf` content.
    pub fn custom_figlet(mut self, content: impl Into<Arc<str>>) -> Self {
        self.font = BigFont::CustomFiglet;
        self.custom_figlet = Some(content.into());
        self
    }

    /// Load a custom FIGlet font from a file path.
    pub fn custom_figlet_from_file(self, path: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(self.custom_figlet(content))
    }

    /// Helper to set simple shadow with offset (1, 1).
    pub fn with_shadow(mut self, shadow_style: Style) -> Self {
        self.shadow = Some(Shadow {
            style: shadow_style,
            offset_x: 1,
            offset_y: 1,
        });
        self
    }

    /// Apply a color gradient over the rendered output.
    ///
    /// The gradient is a render-time effect and does not affect the glyph cache.
    /// Use [`GradientDirection::Vertical`] for a top-to-bottom color wash across
    /// the font rows, or [`GradientDirection::Horizontal`] for a left-to-right
    /// wash across character columns.
    pub fn gradient(mut self, gradient: ColorGradient, direction: GradientDirection) -> Self {
        self.gradient = Some((gradient, direction));
        self
    }

    pub(crate) fn build_lines(&self) -> Arc<BigTextRenderOutput> {
        let cache_key = BigTextCacheKey::new(
            &self.text,
            self.font,
            self.style,
            self.shadow,
            self.custom_figlet.as_ref(),
        );

        let cache_mutex =
            BIG_TEXT_CACHE.get_or_init(|| std::sync::Mutex::new(BigTextVisualCache::new()));
        if let Ok(cache) = cache_mutex.lock()
            && let Some(cached) = cache.get(&cache_key)
        {
            return cached;
        }

        struct Segment {
            lines: Vec<String>,
            style: Style,
            width: usize,
        }

        let mut segments = Vec::new();
        for span in &self.text.spans {
            if span.content.is_empty() {
                continue;
            }

            let lines = self.render_text(span.content.as_ref());
            if lines.is_empty() {
                continue;
            }

            let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
            if width == 0 {
                continue;
            }

            let style = self.style.patch(span.style);
            segments.push(Segment {
                lines,
                style,
                width,
            });
        }

        if segments.is_empty() {
            let output = Arc::new(BigTextRenderOutput {
                lines: Vec::new(),
                width: 0,
                height: 0,
            });
            if let Ok(mut cache) = cache_mutex.lock() {
                cache.insert(cache_key, output.clone());
            }
            return output;
        }

        let raw_height = segments
            .iter()
            .map(|segment| segment.lines.len())
            .max()
            .unwrap_or(0);

        if raw_height == 0 {
            let output = Arc::new(BigTextRenderOutput {
                lines: Vec::new(),
                width: 0,
                height: 0,
            });
            if let Ok(mut cache) = cache_mutex.lock() {
                cache.insert(cache_key, output.clone());
            }
            return output;
        }

        let mut raw_grid: Vec<Vec<(char, Style)>> = vec![Vec::new(); raw_height];

        for segment in segments {
            for (row_idx, row) in raw_grid.iter_mut().enumerate().take(raw_height) {
                let line = segment.lines.get(row_idx).map(|s| s.as_str()).unwrap_or("");
                let mut line_len = 0usize;

                for c in line.chars() {
                    let style = if c == ' ' {
                        Style::default()
                    } else {
                        segment.style
                    };
                    row.push((c, style));
                    line_len += 1;
                }

                if line_len < segment.width {
                    row.extend(std::iter::repeat_n(
                        (' ', Style::default()),
                        segment.width - line_len,
                    ));
                }
            }
        }

        let raw_width = raw_grid.first().map(|row| row.len()).unwrap_or(0);
        if raw_width == 0 {
            let output = Arc::new(BigTextRenderOutput {
                lines: Vec::new(),
                width: 0,
                height: 0,
            });
            if let Ok(mut cache) = cache_mutex.lock() {
                cache.insert(cache_key, output.clone());
            }
            return output;
        }

        let shadow_cfg = self.shadow;

        let (final_w, final_h, offset_x, offset_y) = if let Some(s) = shadow_cfg {
            let min_x = 0.min(s.offset_x);
            let min_y = 0.min(s.offset_y);
            let max_x = (raw_width as i16).max(raw_width as i16 + s.offset_x);
            let max_y = (raw_height as i16).max(raw_height as i16 + s.offset_y);

            (
                (max_x - min_x) as usize,
                (max_y - min_y) as usize,
                -min_x,
                -min_y,
            )
        } else {
            (raw_width, raw_height, 0, 0)
        };

        let mut grid: Vec<Vec<(char, Style)>> =
            vec![vec![(' ', Style::default()); final_w]; final_h];

        let put_char =
            |x: i16, y: i16, c: char, style: Style, grid: &mut Vec<Vec<(char, Style)>>| {
                let gx = x + offset_x;
                let gy = y + offset_y;
                if gx >= 0 && gy >= 0 && (gx as usize) < final_w && (gy as usize) < final_h {
                    grid[gy as usize][gx as usize] = (c, style);
                }
            };

        if let Some(s) = shadow_cfg {
            for (y, row) in raw_grid.iter().enumerate().take(raw_height) {
                for (x, (c, _)) in row.iter().enumerate().take(raw_width) {
                    if *c != ' ' {
                        put_char(
                            x as i16 + s.offset_x,
                            y as i16 + s.offset_y,
                            *c,
                            s.style,
                            &mut grid,
                        );
                    }
                }
            }
        }

        for (y, row) in raw_grid.iter().enumerate().take(raw_height) {
            for (x, (c, style)) in row.iter().enumerate().take(raw_width) {
                if *c != ' ' {
                    put_char(x as i16, y as i16, *c, *style, &mut grid);
                }
            }
        }

        let mut min_y = 0;
        let mut max_y = final_h.saturating_sub(1);

        for (y, row) in grid.iter().enumerate().take(final_h) {
            let row_is_empty = row.iter().all(|(c, _)| *c == ' ');
            if !row_is_empty {
                min_y = y;
                break;
            }
        }

        for (y, row) in grid.iter().enumerate().take(final_h).rev() {
            let row_is_empty = row.iter().all(|(c, _)| *c == ' ');
            if !row_is_empty {
                max_y = y;
                break;
            }
        }

        if min_y > max_y {
            let output = Arc::new(BigTextRenderOutput {
                lines: Vec::new(),
                width: 0,
                height: 0,
            });
            if let Ok(mut cache) = cache_mutex.lock() {
                cache.insert(cache_key, output.clone());
            }
            return output;
        }

        let mut lines = Vec::new();
        for row in grid.iter().take(max_y + 1).skip(min_y) {
            let mut spans = Vec::new();
            let mut current_span_str = String::new();
            let mut current_style = if row.is_empty() {
                Style::default()
            } else {
                row[0].1
            };

            for (c, style) in row {
                if *style != current_style {
                    if !current_span_str.is_empty() {
                        spans.push(Span::new(current_span_str.clone()).style(current_style));
                        current_span_str.clear();
                    }
                    current_style = *style;
                }
                current_span_str.push(*c);
            }
            if !current_span_str.is_empty() {
                spans.push(Span::new(current_span_str).style(current_style));
            }

            lines.push(spans);
        }

        let height = lines.len().min(u16::MAX as usize) as u16;
        let width = final_w.min(u16::MAX as usize) as u16;

        let output = Arc::new(BigTextRenderOutput {
            lines,
            width,
            height,
        });

        if let Ok(mut cache) = cache_mutex.lock() {
            cache.insert(cache_key, output.clone());
        }

        output
    }

    fn render_text(&self, text: &str) -> Vec<String> {
        match self.font {
            BigFont::Pixel => self.render_pixel(text, false),
            BigFont::PixelBold => self.render_pixel(text, true),
            BigFont::Quadrant => self.render_quadrant(text),
            _ => self.render_figlet(text),
        }
    }

    fn render_figlet(&self, text: &str) -> Vec<String> {
        if matches!(self.font, BigFont::CustomFiglet)
            && let Some(font) = self.custom_figlet_font()
        {
            return if let Some(figure) = font.convert(text) {
                figure.to_string().lines().map(|s| s.to_string()).collect()
            } else {
                vec![text.to_string()]
            };
        }

        fn load_font<'a>(
            slot: &'a OnceLock<Option<FIGlet>>,
            font_data: &str,
        ) -> Option<&'a FIGlet> {
            slot.get_or_init(|| {
                FIGlet::from_content(font_data)
                    .or_else(|_| FIGlet::standard())
                    .ok()
            })
            .as_ref()
        }

        // Get or initialize the cached font for this font type.
        // If even the standard fallback fails, return plain text.
        let font = match self.font {
            BigFont::Standard => load_font(&FIGFONT_STANDARD, STANDARD_FONT),
            BigFont::Slant => load_font(&FIGFONT_SLANT, SLANT_FONT),
            BigFont::Bloody => load_font(&FIGFONT_BLOODY, BLOODY_FONT),
            BigFont::Colossal => load_font(&FIGFONT_COLOSSAL, COLOSSAL_FONT),
            BigFont::Roman => load_font(&FIGFONT_ROMAN, ROMAN_FONT),
            BigFont::SubZero => load_font(&FIGFONT_SUB_ZERO, SUB_ZERO_FONT),
            BigFont::Poison => load_font(&FIGFONT_POISON, POISON_FONT),
            BigFont::Nancyj => load_font(&FIGFONT_NANCYJ, NANCYJ_FONT),
            BigFont::SmallPoison => load_font(&FIGFONT_SMALL_POISON, SMALL_POISON_FONT),
            BigFont::DosRebel => load_font(&FIGFONT_DOS_REBEL, DOS_REBEL_FONT),
            BigFont::AnsiShadow => load_font(&FIGFONT_ANSI_SHADOW, ANSI_SHADOW_FONT),
            BigFont::Small => load_font(&FIGFONT_SMALL, SMALL_FONT),
            BigFont::CustomFiglet | BigFont::Pixel | BigFont::PixelBold | BigFont::Quadrant => {
                load_font(&FIGFONT_STANDARD, STANDARD_FONT)
            }
        };

        let Some(font) = font else {
            return vec![text.to_string()];
        };

        if let Some(f) = font.convert(text) {
            f.to_string()
                .lines()
                .map(|s: &str| s.to_string())
                .collect::<Vec<_>>()
        } else {
            vec![text.to_string()]
        }
    }

    fn custom_figlet_font(&self) -> Option<Arc<FIGlet>> {
        let content = self.custom_figlet.as_ref()?;
        let cache_mutex =
            CUSTOM_FIGFONT_CACHE.get_or_init(|| std::sync::Mutex::new(CustomFigletCache::new()));
        if let Ok(cache) = cache_mutex.lock()
            && let Some(cached) = cache.get(content)
        {
            return Some(cached);
        }

        let parsed = FIGlet::from_content(content.as_ref()).ok()?;
        let font = Arc::new(parsed);

        if let Ok(mut cache) = cache_mutex.lock() {
            cache.insert(content.clone(), font.clone());
        }

        Some(font)
    }

    fn render_pixel(&self, text: &str, bold: bool) -> Vec<String> {
        let mut bitmap: Vec<Vec<bool>> = Vec::new();
        let char_height = 8;

        for _ in 0..char_height {
            bitmap.push(Vec::new());
        }

        for c in text.chars() {
            if let Some(glyph) = BASIC_FONTS.get(c) {
                for (row_idx, byte) in glyph.iter().enumerate() {
                    if row_idx >= char_height {
                        break;
                    }
                    for bit in 0..8 {
                        let is_set = (byte & (1 << bit)) != 0;
                        bitmap[row_idx].push(is_set);
                    }
                }
            } else {
                for row in bitmap.iter_mut() {
                    for _ in 0..8 {
                        row.push(false);
                    }
                }
            }
        }

        if bold {
            for row in bitmap.iter_mut() {
                let original = row.clone();
                let mut new_row = Vec::with_capacity(original.len());
                for (i, &pixel) in original.iter().enumerate() {
                    let prev = if i > 0 { original[i - 1] } else { false };
                    new_row.push(pixel | prev);
                }
                *row = new_row;
            }
        }

        let mut lines = Vec::new();
        for y in (0..char_height).step_by(2) {
            let mut line = String::new();
            if y + 1 >= char_height {
                break;
            }

            let row_top = &bitmap[y];
            let row_bottom = &bitmap[y + 1];

            for x in 0..row_top.len() {
                let top = row_top[x];
                let bottom = row_bottom[x];

                let char = match (top, bottom) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                };
                line.push(char);
            }
            lines.push(line);
        }

        lines
    }

    fn render_quadrant(&self, text: &str) -> Vec<String> {
        let mut bitmap: Vec<Vec<bool>> = Vec::new();
        let char_height = 8;

        for _ in 0..char_height {
            bitmap.push(Vec::new());
        }

        for c in text.chars() {
            if let Some(glyph) = BASIC_FONTS.get(c) {
                for (row_idx, byte) in glyph.iter().enumerate() {
                    if row_idx >= char_height {
                        break;
                    }
                    for bit in 0..8 {
                        let is_set = (byte & (1 << bit)) != 0;
                        bitmap[row_idx].push(is_set);
                    }
                }
            } else {
                for row in bitmap.iter_mut() {
                    for _ in 0..8 {
                        row.push(false);
                    }
                }
            }
        }

        let mut lines = Vec::new();
        for y in (0..char_height).step_by(2) {
            let mut line = String::new();
            if y + 1 >= char_height {
                break;
            }

            let row_top = &bitmap[y];
            let row_bottom = &bitmap[y + 1];

            for x in (0..row_top.len()).step_by(2) {
                if x + 1 >= row_top.len() {
                    break;
                }

                let tl = row_top[x];
                let tr = row_top[x + 1];
                let bl = row_bottom[x];
                let br = row_bottom[x + 1];

                let char = match (tl, tr, bl, br) {
                    (false, false, false, false) => ' ',
                    (false, false, false, true) => '▗',
                    (false, false, true, false) => '▖',
                    (false, false, true, true) => '▄',
                    (false, true, false, false) => '▝',
                    (false, true, false, true) => '▐',
                    (false, true, true, false) => '▞',
                    (false, true, true, true) => '▟',
                    (true, false, false, false) => '▘',
                    (true, false, false, true) => '▚',
                    (true, false, true, false) => '▌',
                    (true, false, true, true) => '▙',
                    (true, true, false, false) => '▀',
                    (true, true, false, true) => '▜',
                    (true, true, true, false) => '▛',
                    (true, true, true, true) => '█',
                };
                line.push(char);
            }
            lines.push(line);
        }

        lines
    }
}

impl From<BigText> for Element {
    fn from(val: BigText) -> Self {
        Element::new(ElementKind::BigText(val))
    }
}

impl crate::layout::hash::LayoutHash for BigText {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.font.hash(hasher);
        crate::layout::hash::hash_spans_content(&self.text.spans, hasher);
        self.shadow.hash(hasher);
        self.custom_figlet.hash(hasher);
        Some(())
    }
}
