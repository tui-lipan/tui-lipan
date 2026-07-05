use std::fs;
use std::io::Cursor;

use font8x8::{BASIC_FONTS, BLOCK_FONTS, BOX_FONTS, UnicodeFonts};
use fontdb::{Database, Family, Query};
use fontdue::{Font, FontSettings};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{CapturedCell, CapturedFrame, PngOptions, PngTextRenderer};
use crate::style::Color;

type Rgb8 = (u8, u8, u8);

#[derive(Clone, Copy)]
struct CellPixels {
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EffectiveCellStyle {
    fg: Rgb8,
    bg: Rgb8,
    underline: Rgb8,
}

struct FontRenderer {
    fonts: Vec<Font>,
}

enum ActiveTextRenderer {
    Font(FontRenderer),
    Bitmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BitmapGlyphFallback {
    Ascii(char),
    IconPlaceholder,
    MissingBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedBitmapGlyph {
    Glyph {
        glyph: [u8; 8],
        fill_full_cell: bool,
    },
    Fallback(BitmapGlyphFallback),
}

pub(super) fn encode_frame(frame: &CapturedFrame, options: &PngOptions) -> Vec<u8> {
    try_encode_frame(frame, options).unwrap_or_default()
}

pub(super) fn try_encode_frame(
    frame: &CapturedFrame,
    options: &PngOptions,
) -> image::ImageResult<Vec<u8>> {
    let cell_width = u32::from(options.cell_width.max(1));
    let cell_height = u32::from(options.cell_height.max(1));
    let scale = u32::from(options.scale.max(1));
    let final_cell_width = cell_width.saturating_mul(scale);
    let final_cell_height = cell_height.saturating_mul(scale);
    let width = u32::from(frame.width).saturating_mul(final_cell_width);
    let height = u32::from(frame.height).saturating_mul(final_cell_height);

    let mut image = RgbImage::new(width, height);
    let columns = usize::from(frame.width);
    let text_renderer = ActiveTextRenderer::from_options(options);

    for y in 0..frame.height {
        let mut x = 0;
        while x < frame.width {
            let idx = usize::from(y)
                .saturating_mul(columns)
                .saturating_add(usize::from(x));
            if let Some(cell) = frame.cells.get(idx) {
                let cell_span = cell_span(cell, x, frame.width);
                let cell_rect = CellPixels {
                    x0: u32::from(x).saturating_mul(final_cell_width),
                    y0: u32::from(y).saturating_mul(final_cell_height),
                    width: final_cell_width.saturating_mul(u32::from(cell_span)),
                    height: final_cell_height,
                };
                draw_cell(&mut image, cell_rect, cell, options, &text_renderer);
                x = x.saturating_add(cell_span);
            } else {
                x = x.saturating_add(1);
            }
        }
    }

    if options.render_cursor
        && let Some(cursor) = frame.cursor.as_ref().filter(|cursor| cursor.visible)
        && cursor.x < frame.width
        && cursor.y < frame.height
    {
        let color = cursor_color(frame, cursor.x, cursor.y, columns, options);
        draw_cursor(
            &mut image,
            cursor.x,
            cursor.y,
            color,
            final_cell_width,
            final_cell_height,
        );
    }

    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn draw_cell(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    cell: &CapturedCell,
    options: &PngOptions,
    text_renderer: &ActiveTextRenderer,
) {
    let style = effective_colors(cell, options);

    fill_background(image, cell_rect, style.bg);
    draw_glyph(image, cell_rect, cell, style.fg, text_renderer);
    draw_decorations(image, cell_rect, cell, style);
}

fn cell_span(cell: &CapturedCell, x: u16, frame_width: u16) -> u16 {
    if UnicodeWidthStr::width(cell.symbol.as_str()) >= 2 && x + 1 < frame_width {
        2
    } else {
        1
    }
}

fn glyph_for(ch: char) -> Option<[u8; 8]> {
    BASIC_FONTS
        .get(ch)
        .or_else(|| BOX_FONTS.get(ch))
        .or_else(|| BLOCK_FONTS.get(ch))
}

fn effective_colors(cell: &CapturedCell, options: &PngOptions) -> EffectiveCellStyle {
    let mut fg = resolve_fg(cell.fg, options);
    let mut bg = resolve_bg(cell.bg, options);

    if cell.modifiers.reverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.modifiers.dim {
        fg = (fg.0 / 2, fg.1 / 2, fg.2 / 2);
    }

    EffectiveCellStyle {
        fg,
        bg,
        underline: resolve_fg(cell.underline_color, options),
    }
}

fn fill_background(image: &mut RgbImage, cell_rect: CellPixels, color: Rgb8) {
    fill_rect(
        image,
        cell_rect.x0,
        cell_rect.y0,
        cell_rect.width,
        cell_rect.height,
        color,
    );
}

fn primary_grapheme(symbol: &str) -> Option<&str> {
    symbol.graphemes(true).find(|grapheme| {
        grapheme
            .chars()
            .any(|ch| !ch.is_control() && !ch.is_whitespace())
    })
}

fn primary_char_for_bitmap(symbol: &str) -> Option<char> {
    let grapheme = primary_grapheme(symbol)?;
    let mut chars = grapheme.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() || ch.is_whitespace() {
        None
    } else {
        Some(ch)
    }
}

fn resolve_glyph(symbol: &str) -> Option<ResolvedBitmapGlyph> {
    let grapheme = primary_grapheme(symbol)?;
    let Some(ch) = primary_char_for_bitmap(grapheme) else {
        return Some(ResolvedBitmapGlyph::Fallback(
            BitmapGlyphFallback::MissingBox,
        ));
    };

    if let Some(glyph) = glyph_for(ch) {
        return Some(ResolvedBitmapGlyph::Glyph {
            glyph,
            fill_full_cell: is_box_or_block(ch),
        });
    }

    Some(ResolvedBitmapGlyph::Fallback(classify_fallback(ch)))
}

fn is_box_or_block(ch: char) -> bool {
    BOX_FONTS.get(ch).is_some() || BLOCK_FONTS.get(ch).is_some()
}

fn classify_fallback(ch: char) -> BitmapGlyphFallback {
    match ch {
        '' | '' | '' | '' => BitmapGlyphFallback::Ascii('>'),
        '' | '' | '' | '' => BitmapGlyphFallback::Ascii('<'),
        '▶' | '▸' | '❯' | '➤' | '➜' | '→' | '›' => BitmapGlyphFallback::Ascii('>'),
        '◀' | '◂' | '❮' | '←' | '‹' => BitmapGlyphFallback::Ascii('<'),
        '✓' | '✔' => BitmapGlyphFallback::Ascii('v'),
        '✗' | '×' | '✘' => BitmapGlyphFallback::Ascii('x'),
        '●' | '•' | '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏' => {
            BitmapGlyphFallback::Ascii('*')
        }
        '⠂' | '⠒' | '⠐' | '⠠' => BitmapGlyphFallback::Ascii('.'),
        '│' | '┃' | '║' => BitmapGlyphFallback::Ascii('|'),
        '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}' => {
            BitmapGlyphFallback::IconPlaceholder
        }
        _ => BitmapGlyphFallback::MissingBox,
    }
}

fn draw_glyph(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    cell: &CapturedCell,
    color: Rgb8,
    text_renderer: &ActiveTextRenderer,
) {
    let Some(resolved) = resolve_glyph(cell.symbol.as_str()) else {
        return;
    };
    if let ActiveTextRenderer::Font(renderer) = text_renderer
        && let Some(ch) = primary_char_for_bitmap(cell.symbol.as_str())
        && !is_box_or_block(ch)
        && renderer.draw_char(image, cell_rect, ch, color, cell.modifiers.bold)
    {
        return;
    }
    match resolved {
        ResolvedBitmapGlyph::Glyph {
            glyph,
            fill_full_cell,
        } => {
            stamp_glyph(image, cell_rect, glyph, color, 0, fill_full_cell);
            if cell.modifiers.bold {
                stamp_glyph(
                    image,
                    cell_rect,
                    glyph,
                    color,
                    bold_offset(cell_rect),
                    fill_full_cell,
                );
            }
        }
        ResolvedBitmapGlyph::Fallback(BitmapGlyphFallback::Ascii(ch)) => {
            if let Some(glyph) = glyph_for(ch) {
                stamp_glyph(image, cell_rect, glyph, color, 0, false);
                if cell.modifiers.bold {
                    stamp_glyph(
                        image,
                        cell_rect,
                        glyph,
                        color,
                        bold_offset(cell_rect),
                        false,
                    );
                }
            }
        }
        ResolvedBitmapGlyph::Fallback(BitmapGlyphFallback::IconPlaceholder) => {
            draw_icon_placeholder(image, cell_rect, color);
        }
        ResolvedBitmapGlyph::Fallback(BitmapGlyphFallback::MissingBox) => {
            draw_missing_box(image, cell_rect, color);
        }
    }
}

impl ActiveTextRenderer {
    fn from_options(options: &PngOptions) -> Self {
        match options.text_renderer {
            PngTextRenderer::Bitmap => Self::Bitmap,
            PngTextRenderer::Auto | PngTextRenderer::Font => FontRenderer::from_options(options)
                .map(Self::Font)
                .unwrap_or(Self::Bitmap),
        }
    }
}

impl FontRenderer {
    fn from_options(options: &PngOptions) -> Option<Self> {
        let mut fonts = Vec::new();
        if let Some(font) = options.font_path.as_ref().and_then(|path| {
            fs::read(path)
                .ok()
                .and_then(|bytes| Font::from_bytes(bytes, FontSettings::default()).ok())
        }) {
            fonts.push(font);
        }

        let mut db = Database::new();
        db.load_system_fonts();

        if let Some(family) = options.font_family.as_deref()
            && let Some(font) = load_font_family(&db, family)
        {
            fonts.push(font);
        }

        for family in [
            "Symbols Nerd Font Mono",
            "JetBrainsMono Nerd Font",
            "JetBrains Mono",
            "FiraCode Nerd Font Mono",
            "Fira Code",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Noto Sans Mono",
            "monospace",
        ] {
            if let Some(font) = load_font_family(&db, family) {
                fonts.push(font);
            }
        }

        (!fonts.is_empty()).then_some(Self { fonts })
    }

    fn draw_char(
        &self,
        image: &mut RgbImage,
        cell_rect: CellPixels,
        ch: char,
        color: Rgb8,
        bold: bool,
    ) -> bool {
        for font in &self.fonts {
            if font.lookup_glyph_index(ch) == 0 {
                continue;
            }
            if rasterize_font_char(image, cell_rect, font, ch, color, bold) {
                return true;
            }
        }
        false
    }
}

fn load_font_family(db: &Database, family: &str) -> Option<Font> {
    let families = if family.eq_ignore_ascii_case("monospace") {
        [Family::Monospace]
    } else {
        [Family::Name(family)]
    };
    let query = Query {
        families: &families,
        ..Query::default()
    };
    let id = db.query(&query)?;
    db.with_face_data(id, |data, face_index| {
        let settings = FontSettings {
            collection_index: face_index,
            ..FontSettings::default()
        };
        Font::from_bytes(data, settings).ok()
    })?
}

fn rasterize_font_char(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    font: &Font,
    ch: char,
    color: Rgb8,
    bold: bool,
) -> bool {
    if cell_rect.width == 0 || cell_rect.height == 0 {
        return false;
    }

    let font_size = (cell_rect.height as f32 * 0.82).max(1.0);
    let (metrics, bitmap) = font.rasterize(ch, font_size);
    if metrics.width == 0 || metrics.height == 0 || bitmap.is_empty() {
        return false;
    }

    let advance = metrics.advance_width.max(metrics.width as f32);
    let x_base = cell_rect.x0 as i32
        + ((cell_rect.width as f32 - advance).max(0.0) / 2.0).round() as i32
        + metrics.xmin;
    let baseline = cell_rect.y0 as i32 + (cell_rect.height as f32 * 0.78).round() as i32;
    let y_base = baseline - metrics.height as i32 - metrics.ymin;
    let bold_offset = bold_offset(cell_rect) as i32;
    let passes = if bold { 2 } else { 1 };
    let mut drew = false;

    for pass in 0..passes {
        let x_pass_offset = if pass == 0 { 0 } else { bold_offset.max(1) };
        for glyph_y in 0..metrics.height {
            for glyph_x in 0..metrics.width {
                let coverage = bitmap[glyph_y * metrics.width + glyph_x];
                if coverage == 0 {
                    continue;
                }
                let px = x_base + glyph_x as i32 + x_pass_offset;
                let py = y_base + glyph_y as i32;
                if px < cell_rect.x0 as i32
                    || py < cell_rect.y0 as i32
                    || px >= cell_rect.x0.saturating_add(cell_rect.width) as i32
                    || py >= cell_rect.y0.saturating_add(cell_rect.height) as i32
                {
                    continue;
                }
                blend_rgb(image, px as u32, py as u32, color, coverage);
                drew = true;
            }
        }
    }

    drew
}

fn draw_decorations(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    cell: &CapturedCell,
    style: EffectiveCellStyle,
) {
    if cell.modifiers.underline && cell_rect.height > 0 {
        let thickness = decoration_thickness(cell_rect);
        let y = cell_rect.y0 + cell_rect.height.saturating_sub(thickness);
        fill_rect(
            image,
            cell_rect.x0,
            y,
            cell_rect.width,
            thickness,
            style.underline,
        );
    }
    if cell.modifiers.strikethrough {
        let thickness = decoration_thickness(cell_rect);
        let y = cell_rect
            .y0
            .saturating_add(cell_rect.height / 2)
            .saturating_sub(thickness / 2);
        fill_rect(image, cell_rect.x0, y, cell_rect.width, thickness, style.fg);
    }
}

fn stamp_glyph(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    glyph: [u8; 8],
    color: Rgb8,
    x_offset: u32,
    fill_full_cell: bool,
) {
    if cell_rect.width == 0 || cell_rect.height == 0 {
        return;
    }

    let glyph_scale_x = if fill_full_cell {
        (cell_rect.width.saturating_add(7) / 8).max(1)
    } else {
        (cell_rect.width / 8).max(1)
    };
    let glyph_scale_y = if fill_full_cell {
        (cell_rect.height.saturating_add(7) / 8).max(1)
    } else {
        let target_height = cell_rect.height.saturating_mul(3) / 4;
        (target_height / 8).max(1).min(glyph_scale_x.max(1))
    };
    let glyph_width = 8 * glyph_scale_x;
    let glyph_height = 8 * glyph_scale_y;
    let pad_x = cell_rect.width.saturating_sub(glyph_width) / 2;
    let pad_y = cell_rect.height.saturating_sub(glyph_height) / 2;

    for (glyph_y, row) in glyph.iter().copied().enumerate() {
        for glyph_x in 0..8_u32 {
            if row & (1 << glyph_x) == 0 {
                continue;
            }
            for sy in 0..glyph_scale_y {
                for sx in 0..glyph_scale_x {
                    let px = cell_rect.x0 + pad_x + glyph_x * glyph_scale_x + sx + x_offset;
                    let py = cell_rect.y0
                        + pad_y
                        + u32::try_from(glyph_y).unwrap_or(0) * glyph_scale_y
                        + sy;
                    if px < cell_rect.x0 + cell_rect.width && py < cell_rect.y0 + cell_rect.height {
                        put_rgb(image, px, py, color);
                    }
                }
            }
        }
    }
}

fn bold_offset(cell_rect: CellPixels) -> u32 {
    (cell_rect.width / 16).max(1)
}

fn decoration_thickness(cell_rect: CellPixels) -> u32 {
    (cell_rect.height / 16).max(1)
}

fn draw_icon_placeholder(image: &mut RgbImage, cell_rect: CellPixels, color: Rgb8) {
    draw_missing_box(image, cell_rect, color);
    let inset_x = (cell_rect.width / 3).max(1);
    let inset_y = (cell_rect.height / 3).max(1);
    let x = cell_rect.x0.saturating_add(inset_x);
    let y = cell_rect.y0.saturating_add(inset_y);
    fill_rect(
        image,
        x,
        y,
        cell_rect
            .width
            .saturating_sub(inset_x.saturating_mul(2))
            .max(1),
        cell_rect
            .height
            .saturating_sub(inset_y.saturating_mul(2))
            .max(1),
        color,
    );
}

fn draw_missing_box(image: &mut RgbImage, cell_rect: CellPixels, color: Rgb8) {
    if cell_rect.width == 0 || cell_rect.height == 0 {
        return;
    }
    let box_width = cell_rect.width.saturating_mul(3) / 4;
    let box_height = cell_rect.height.saturating_mul(3) / 4;
    let x0 = cell_rect.x0 + cell_rect.width.saturating_sub(box_width) / 2;
    let y0 = cell_rect.y0 + cell_rect.height.saturating_sub(box_height) / 2;
    let thickness = decoration_thickness(cell_rect);
    fill_rect(image, x0, y0, box_width.max(1), thickness, color);
    fill_rect(
        image,
        x0,
        y0 + box_height.saturating_sub(thickness),
        box_width.max(1),
        thickness,
        color,
    );
    fill_rect(image, x0, y0, thickness, box_height.max(1), color);
    fill_rect(
        image,
        x0 + box_width.saturating_sub(thickness),
        y0,
        thickness,
        box_height.max(1),
        color,
    );
}

fn draw_cursor(
    image: &mut RgbImage,
    cell_x: u16,
    cell_y: u16,
    color: Rgb8,
    cell_width: u32,
    cell_height: u32,
) {
    let x0 = u32::from(cell_x).saturating_mul(cell_width);
    let y0 = u32::from(cell_y).saturating_mul(cell_height);
    let x1 = x0 + cell_width.saturating_sub(1);
    let y1 = y0 + cell_height.saturating_sub(1);
    let thickness = (cell_height / 16).max(1).min(cell_width).min(cell_height);

    fill_rect(image, x0, y0, cell_width, thickness, color);
    fill_rect(
        image,
        x0,
        y1.saturating_add(1).saturating_sub(thickness),
        cell_width,
        thickness,
        color,
    );
    fill_rect(image, x0, y0, thickness, cell_height, color);
    fill_rect(
        image,
        x1.saturating_add(1).saturating_sub(thickness),
        y0,
        thickness,
        cell_height,
        color,
    );
}

fn cursor_color(
    frame: &CapturedFrame,
    x: u16,
    y: u16,
    columns: usize,
    options: &PngOptions,
) -> Rgb8 {
    let idx = usize::from(y)
        .saturating_mul(columns)
        .saturating_add(usize::from(x));
    let Some(cell) = frame.cells.get(idx) else {
        return resolve_fg(options.default_fg, options);
    };

    let mut fg = resolve_fg(cell.fg, options);
    let mut bg = resolve_bg(cell.bg, options);
    if cell.modifiers.reverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.modifiers.dim {
        fg = (fg.0 / 2, fg.1 / 2, fg.2 / 2);
    }
    fg
}

fn fill_rect(image: &mut RgbImage, x0: u32, y0: u32, width: u32, height: u32, color: Rgb8) {
    for y in y0..y0.saturating_add(height) {
        for x in x0..x0.saturating_add(width) {
            put_rgb(image, x, y, color);
        }
    }
}

fn put_rgb(image: &mut RgbImage, x: u32, y: u32, color: Rgb8) {
    if x < image.width() && y < image.height() {
        image.put_pixel(x, y, Rgb([color.0, color.1, color.2]));
    }
}

fn blend_rgb(image: &mut RgbImage, x: u32, y: u32, color: Rgb8, alpha: u8) {
    if x >= image.width() || y >= image.height() {
        return;
    }
    if alpha == u8::MAX {
        put_rgb(image, x, y, color);
        return;
    }
    let existing = image.get_pixel(x, y).0;
    let alpha = u16::from(alpha);
    let inv_alpha = u16::from(u8::MAX) - alpha;
    let blend = |src: u8, dst: u8| -> u8 {
        ((u16::from(src) * alpha + u16::from(dst) * inv_alpha) / u16::from(u8::MAX)) as u8
    };
    image.put_pixel(
        x,
        y,
        Rgb([
            blend(color.0, existing[0]),
            blend(color.1, existing[1]),
            blend(color.2, existing[2]),
        ]),
    );
}

fn resolve_fg(color: Color, options: &PngOptions) -> Rgb8 {
    resolve_color(
        color,
        options.default_fg.to_rgb().unwrap_or((255, 255, 255)),
    )
}

fn resolve_bg(color: Color, options: &PngOptions) -> Rgb8 {
    resolve_color(color, options.default_bg.to_rgb().unwrap_or((0, 0, 0)))
}

fn resolve_color(color: Color, fallback: Rgb8) -> Rgb8 {
    color.to_rgb().unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_use_codepoints_use_icon_placeholder_not_question_mark() {
        assert_eq!(
            classify_fallback('\u{e000}'),
            BitmapGlyphFallback::IconPlaceholder
        );
        assert_eq!(
            classify_fallback('\u{f0001}'),
            BitmapGlyphFallback::IconPlaceholder
        );
        assert_ne!(
            resolve_glyph("\u{e000}"),
            Some(ResolvedBitmapGlyph::Fallback(BitmapGlyphFallback::Ascii(
                '?'
            )))
        );
        assert_eq!(
            resolve_glyph("\u{e000}"),
            Some(ResolvedBitmapGlyph::Fallback(
                BitmapGlyphFallback::IconPlaceholder
            ))
        );
    }

    #[test]
    fn common_tui_symbols_fallback_to_ascii_alternatives() {
        assert_eq!(classify_fallback('▶'), BitmapGlyphFallback::Ascii('>'));
        assert_eq!(classify_fallback('◂'), BitmapGlyphFallback::Ascii('<'));
        assert_eq!(classify_fallback('✓'), BitmapGlyphFallback::Ascii('v'));
        assert_eq!(classify_fallback('×'), BitmapGlyphFallback::Ascii('x'));
        assert_eq!(classify_fallback('●'), BitmapGlyphFallback::Ascii('*'));
        assert_eq!(classify_fallback('⠋'), BitmapGlyphFallback::Ascii('*'));
    }

    #[test]
    fn grapheme_selection_skips_empty_whitespace_and_controls() {
        assert_eq!(primary_char_for_bitmap(""), None);
        assert_eq!(primary_char_for_bitmap(" "), None);
        assert_eq!(primary_char_for_bitmap("\nA"), Some('A'));
        assert_eq!(primary_char_for_bitmap("\t A"), Some('A'));
        assert_eq!(
            resolve_glyph("👨‍👩‍👧‍👦"),
            Some(ResolvedBitmapGlyph::Fallback(
                BitmapGlyphFallback::MissingBox
            ))
        );
    }
}
