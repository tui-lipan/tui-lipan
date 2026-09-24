use std::io::Cursor;

use font8x8::{BASIC_FONTS, BLOCK_FONTS, BOX_FONTS, GREEK_FONTS, LATIN_FONTS, UnicodeFonts};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    CapturedCell, CapturedFrame, CapturedImage, CursorShape, PngOptions, PngTextRenderer,
    UnderlineStyle,
};
use crate::style::Color;

mod boxdraw;
mod font;

use font::FontRenderer;

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

pub(super) fn encode_frame(
    frame: &CapturedFrame,
    options: &PngOptions,
) -> image::ImageResult<Vec<u8>> {
    match options.text_renderer {
        PngTextRenderer::Bitmap => encode_with(frame, options, None),
        PngTextRenderer::Auto | PngTextRenderer::Font => {
            font::with_renderer(options, |fonts| encode_with(frame, options, fonts))
        }
    }
}

fn encode_with(
    frame: &CapturedFrame,
    options: &PngOptions,
    mut fonts: Option<&mut FontRenderer>,
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
    let image_backgrounds = image_cell_backgrounds(frame);

    // Every cell to draw, left to right: its index, and its pixels across its full span.
    let mut layout = Vec::with_capacity(frame.cells.len());
    for y in 0..frame.height {
        let mut x = 0;
        while x < frame.width {
            let idx = usize::from(y)
                .saturating_mul(columns)
                .saturating_add(usize::from(x));
            let Some(cell) = frame.cells.get(idx) else {
                x = x.saturating_add(1);
                continue;
            };
            let cell_span = cell.span_at(x, frame.width);
            layout.push((
                idx,
                x,
                CellPixels {
                    x0: u32::from(x).saturating_mul(final_cell_width),
                    y0: u32::from(y).saturating_mul(final_cell_height),
                    width: final_cell_width.saturating_mul(u32::from(cell_span)),
                    height: final_cell_height,
                },
            ));
            x = x.saturating_add(cell_span);
        }
    }

    // Backgrounds first, then everything drawn on them, so a glyph allowed past its own cell - an
    // icon spreading into the blank beside it - is not painted over by that cell's background.
    let stand_in = |idx: usize| {
        image_backgrounds
            .get(idx)
            .copied()
            .flatten()
            .filter(|_| frame.cells[idx].symbol == super::image_layer::UPPER_HALF)
    };
    for &(idx, _, cell_rect) in &layout {
        let background = match stand_in(idx) {
            Some(background) => resolve_bg(background, options),
            None => effective_colors(&frame.cells[idx], options).bg,
        };
        fill_background(&mut image, cell_rect, background);
    }
    for &(idx, x, cell_rect) in &layout {
        if stand_in(idx).is_some() {
            continue;
        }
        let cell = &frame.cells[idx];
        let room = glyph_room(frame, idx, x, cell_rect);
        let style = effective_colors(cell, options);
        draw_glyph(
            &mut image,
            cell_rect,
            room,
            cell,
            style.fg,
            fonts.as_deref_mut(),
        );
        draw_decorations(&mut image, cell_rect, cell, style);
    }

    for captured in &frame.images {
        draw_image(&mut image, captured, final_cell_width, final_cell_height);
    }

    if options.render_cursor
        && let Some(cursor) = frame.cursor.as_ref().filter(|cursor| cursor.visible)
        && cursor.x < frame.width
        && cursor.y < frame.height
    {
        let idx = usize::from(cursor.y)
            .saturating_mul(columns)
            .saturating_add(usize::from(cursor.x));
        // The cursor covers the whole glyph it sits on, both columns of a wide one.
        let cell_rect = layout
            .iter()
            .find(|&&(cell_idx, ..)| cell_idx == idx)
            .map_or(
                CellPixels {
                    x0: u32::from(cursor.x).saturating_mul(final_cell_width),
                    y0: u32::from(cursor.y).saturating_mul(final_cell_height),
                    width: final_cell_width,
                    height: final_cell_height,
                },
                |&(_, _, rect)| rect,
            );
        let cell = frame.cells.get(idx);
        let cell_style = cell.map(|cell| effective_colors(cell, options));
        let color = cursor
            .color
            .and_then(|color| resolve_color(color, options))
            .or(cell_style.map(|style| style.fg))
            .unwrap_or_else(|| resolve_fg(options.default_fg, options));
        draw_cursor(&mut image, cell_rect, cursor.shape, color);
        if cursor.shape == CursorShape::Block
            && let Some(cell) = cell
            && let Some(style) = cell_style
        {
            let text = if style.bg == color {
                style.fg
            } else {
                style.bg
            };
            draw_glyph(&mut image, cell_rect, cell_rect, cell, text, fonts);
        }
    }

    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut out, ImageFormat::Png)?;
    Ok(out.into_inner())
}

pub(super) fn encode_image(image: &CapturedImage) -> image::ImageResult<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut out,
        &image.rgba,
        image.width,
        image.height,
        image::ExtendedColorType::Rgba8,
        ImageFormat::Png,
    )?;
    Ok(out.into_inner())
}

/// For each cell showing an image, row-major, the background it had under the image. A cell holding
/// the half-block stand-in draws only that background for the image to cover; one the image left
/// clear kept its own text, which draws as usual under the image's transparent pixels.
fn image_cell_backgrounds(frame: &CapturedFrame) -> Vec<Option<Color>> {
    let mut backgrounds = vec![None; frame.cells.len()];
    // The first image to show a cell is the one drawn under the others, so its record of the
    // background is the one from before any image.
    for image in frame.images.iter().rev() {
        for row in 0..image.area.h {
            for col in 0..image.area.w {
                let offset = usize::from(row) * usize::from(image.area.w) + usize::from(col);
                let (Ok(x), Ok(y)) = (
                    u16::try_from(i32::from(image.area.x) + i32::from(col)),
                    u16::try_from(i32::from(image.area.y) + i32::from(row)),
                ) else {
                    continue;
                };
                if !image.visible[offset] || x >= frame.width || y >= frame.height {
                    continue;
                }
                backgrounds[usize::from(y) * usize::from(frame.width) + usize::from(x)] =
                    Some(image.backgrounds[offset]);
            }
        }
    }
    backgrounds
}

/// Scale `captured` into its area, from the top-left corner and keeping its aspect ratio, over the
/// cells it still shows in.
fn draw_image(canvas: &mut RgbImage, captured: &CapturedImage, cell_w: u32, cell_h: u32) {
    let fitted = captured.fitted_size(cell_w, cell_h);
    let (Ok(area_x), Ok(area_y)) = (
        u32::try_from(captured.area.x),
        u32::try_from(captured.area.y),
    ) else {
        return;
    };
    let (origin_x, origin_y) = (area_x * cell_w, area_y * cell_h);
    let area_w = usize::from(captured.area.w);
    for dy in 0..fitted.1 {
        let row = (dy / cell_h) as usize;
        for dx in 0..fitted.0 {
            let col = (dx / cell_w) as usize;
            if !captured
                .visible
                .get(row * area_w + col)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            if let Some([r, g, b, alpha]) = captured.sample(fitted, (dx, dy), (dx + 1, dy + 1))
                && alpha > 0
            {
                blend_rgb(canvas, origin_x + dx, origin_y + dy, (r, g, b), alpha);
            }
        }
    }
}

/// The pixels the glyph at `idx` may draw into. A private-use icon followed by a plain blank gets
/// both cells, as terminals give it: Nerd Font icons are often wider than one cell, and a program
/// leaves the blank after one for exactly that. Everything else keeps its own cell.
fn glyph_room(frame: &CapturedFrame, idx: usize, x: u16, cell_rect: CellPixels) -> CellPixels {
    let cell = &frame.cells[idx];
    let is_icon = primary_grapheme(&cell.symbol)
        .and_then(base_char)
        .is_some_and(is_private_use);
    let next_is_blank = x + 1 < frame.width
        && frame
            .cells
            .get(idx + 1)
            .is_some_and(|next| next.symbol == " " && next.bg == cell.bg);
    if is_icon && next_is_blank && cell_rect.width > 0 {
        CellPixels {
            width: cell_rect.width * 2,
            ..cell_rect
        }
    } else {
        cell_rect
    }
}

/// Private-use code points: where icon fonts such as Nerd Font keep their symbols.
pub(super) fn is_private_use(ch: char) -> bool {
    matches!(
        ch,
        '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}'
    )
}

fn glyph_for(ch: char) -> Option<[u8; 8]> {
    BASIC_FONTS
        .get(ch)
        .or_else(|| BOX_FONTS.get(ch))
        .or_else(|| BLOCK_FONTS.get(ch))
        .or_else(|| LATIN_FONTS.get(ch))
        .or_else(|| GREEK_FONTS.get(ch))
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
        // An underline with no color of its own is drawn in the text's color, as terminals do.
        underline: resolve_color(cell.underline_color, options).unwrap_or(fg),
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

/// The character a grapheme is built on: its first, ahead of any combining marks, variation
/// selectors, or joined sequence members.
fn base_char(grapheme: &str) -> Option<char> {
    grapheme
        .chars()
        .next()
        .filter(|ch| !ch.is_control() && !ch.is_whitespace())
}

/// The built-in font has no combining marks, so a bitmap grapheme draws its base character alone.
fn resolve_bitmap_glyph(ch: char) -> ResolvedBitmapGlyph {
    match glyph_for(ch) {
        Some(glyph) => ResolvedBitmapGlyph::Glyph {
            glyph,
            fill_full_cell: is_box_or_block(ch),
        },
        None => ResolvedBitmapGlyph::Fallback(classify_fallback(ch)),
    }
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
    room: CellPixels,
    cell: &CapturedCell,
    color: Rgb8,
    fonts: Option<&mut FontRenderer>,
) {
    let Some(grapheme) = primary_grapheme(cell.symbol.as_str()) else {
        return;
    };
    let Some(base) = base_char(grapheme) else {
        return;
    };
    // Box-drawing and block characters are drawn from geometry, as terminals draw them, so they
    // meet the cell edges exactly and curves stay smooth at any size.
    if boxdraw::draw(image, cell_rect, base, color) {
        return;
    }
    if let Some(fonts) = fonts
        && fonts.draw(image, cell_rect, room, grapheme, color, cell.modifiers.bold)
    {
        return;
    }
    match resolve_bitmap_glyph(base) {
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

fn draw_decorations(
    image: &mut RgbImage,
    cell_rect: CellPixels,
    cell: &CapturedCell,
    style: EffectiveCellStyle,
) {
    if let Some(shape) = cell.modifiers.underline
        && cell_rect.height > 0
    {
        draw_underline(image, cell_rect, shape, style.underline);
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

/// Draw an underline along the bottom of `cell_rect`.
///
/// Patterned shapes take their phase from the absolute pixel column, so an underline running
/// across several cells stays continuous.
fn draw_underline(image: &mut RgbImage, cell_rect: CellPixels, shape: UnderlineStyle, color: Rgb8) {
    let thickness = decoration_thickness(cell_rect);
    let bottom = cell_rect.y0 + cell_rect.height.saturating_sub(thickness);
    let columns = cell_rect.x0..cell_rect.x0.saturating_add(cell_rect.width);
    match shape {
        UnderlineStyle::Single => {
            fill_rect(
                image,
                cell_rect.x0,
                bottom,
                cell_rect.width,
                thickness,
                color,
            );
        }
        UnderlineStyle::Double => {
            let upper = bottom.saturating_sub(thickness * 2).max(cell_rect.y0);
            fill_rect(
                image,
                cell_rect.x0,
                bottom,
                cell_rect.width,
                thickness,
                color,
            );
            fill_rect(
                image,
                cell_rect.x0,
                upper,
                cell_rect.width,
                thickness,
                color,
            );
        }
        UnderlineStyle::Dotted => {
            for x in columns.filter(|x| (x / thickness).is_multiple_of(2)) {
                fill_rect(image, x, bottom, 1, thickness, color);
            }
        }
        UnderlineStyle::Dashed => {
            let unit = (cell_rect.height / 6).max(2);
            for x in columns.filter(|x| (x / unit) % 3 != 2) {
                fill_rect(image, x, bottom, 1, thickness, color);
            }
        }
        UnderlineStyle::Curly => {
            let amplitude = (cell_rect.height / 12).max(1);
            let wavelength = (cell_rect.height / 2).max(4) as f32;
            let centre = bottom.saturating_sub(amplitude).max(cell_rect.y0);
            let mut previous: Option<u32> = None;
            for x in columns {
                let phase = x as f32 / wavelength * std::f32::consts::TAU;
                let y = (centre as f32 + amplitude as f32 * phase.sin()).round() as u32;
                let y = y.clamp(cell_rect.y0, bottom);
                // Join steep steps so a thin wave has no gaps.
                let (top, span) = match previous {
                    Some(prev) => (y.min(prev), y.abs_diff(prev) + thickness),
                    None => (y, thickness),
                };
                fill_rect(image, x, top, 1, span, color);
                previous = Some(y);
            }
        }
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

/// Draw a cursor of `shape` over `cell_rect`. A block fills the cell; the caller redraws the
/// glyph on top of it.
fn draw_cursor(image: &mut RgbImage, cell_rect: CellPixels, shape: CursorShape, color: Rgb8) {
    let CellPixels {
        x0,
        y0,
        width,
        height,
    } = cell_rect;
    let outline = (height / 16).max(1).min(width).min(height);
    // A bar or underline cursor is drawn heavier than the outline, as terminals draw it.
    let line = (height / 8).max(1);
    match shape {
        CursorShape::Block => fill_rect(image, x0, y0, width, height, color),
        CursorShape::HollowBlock => {
            fill_rect(image, x0, y0, width, outline, color);
            fill_rect(image, x0, y0 + height - outline, width, outline, color);
            fill_rect(image, x0, y0, outline, height, color);
            fill_rect(image, x0 + width - outline, y0, outline, height, color);
        }
        CursorShape::Underline => {
            let line = line.min(height);
            fill_rect(image, x0, y0 + height - line, width, line, color);
        }
        CursorShape::Bar => fill_rect(image, x0, y0, line.min(width), height, color),
    }
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
    resolve_color(color, options)
        .or_else(|| resolve_color(options.default_fg, options))
        .unwrap_or((255, 255, 255))
}

fn resolve_bg(color: Color, options: &PngOptions) -> Rgb8 {
    resolve_color(color, options)
        .or_else(|| resolve_color(options.default_bg, options))
        .unwrap_or((0, 0, 0))
}

/// The RGB value of `color`, taking the 16 ANSI slots from the palette. `None` for a sentinel
/// such as [`Color::Reset`], which the caller resolves to a default.
fn resolve_color(color: Color, options: &PngOptions) -> Option<Rgb8> {
    ansi_slot(color)
        .map_or(color, |slot| options.ansi_palette[slot])
        .to_rgb()
}

/// The ANSI palette slot `color` names, as a named color or as `Indexed(0..16)`.
fn ansi_slot(color: Color) -> Option<usize> {
    Some(match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(index) if index < 16 => usize::from(index),
        Color::Reset
        | Color::Backdrop
        | Color::Transparent
        | Color::Rgb(..)
        | Color::Indexed(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn primary_char_for_bitmap(symbol: &str) -> Option<char> {
        base_char(primary_grapheme(symbol)?)
    }

    fn row_of(symbols: &[&str], bg: Color) -> CapturedFrame {
        let cells: Vec<CapturedCell> = symbols
            .iter()
            .map(|symbol| CapturedCell {
                symbol: symbol.to_string(),
                fg: Color::Reset,
                bg,
                underline_color: Color::Reset,
                modifiers: super::super::CellModifiers::default(),
            })
            .collect();
        CapturedFrame {
            viewport: crate::style::Rect {
                x: 0,
                y: 0,
                w: cells.len() as u16,
                h: 1,
            },
            width: cells.len() as u16,
            height: 1,
            cells,
            cursor: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn an_icon_followed_by_a_blank_may_spread_into_it() {
        let rect = |x: u32| CellPixels {
            x0: x * 8,
            y0: 0,
            width: 8,
            height: 16,
        };
        let frame = row_of(
            &["\u{F06E4}", " ", "\u{F05B2}", "x", "a", " ", "\u{F05B2}"],
            Color::Reset,
        );

        assert_eq!(
            glyph_room(&frame, 0, 0, rect(0)).width,
            16,
            "icon, then a blank"
        );
        assert_eq!(
            glyph_room(&frame, 2, 2, rect(2)).width,
            8,
            "icon, then text"
        );
        assert_eq!(glyph_room(&frame, 4, 4, rect(4)).width, 8, "not an icon");
        assert_eq!(
            glyph_room(&frame, 6, 6, rect(6)).width,
            8,
            "icon at the row's end"
        );

        // A blank of another color is a different surface, not room to spread into.
        let mut split = row_of(&["\u{F06E4}", " "], Color::Reset);
        split.cells[1].bg = Color::Red;
        assert_eq!(glyph_room(&split, 0, 0, rect(0)).width, 8);
    }

    fn resolve_glyph(symbol: &str) -> Option<ResolvedBitmapGlyph> {
        primary_char_for_bitmap(symbol).map(resolve_bitmap_glyph)
    }

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

    /// Encode a one-cell frame holding `symbol` in white on black, with `cursor` over it, and
    /// return its 8x16 pixels.
    fn cursor_pixels(symbol: &str, cursor: super::super::CursorState) -> image::RgbImage {
        let mut frame = row_of(&[symbol], Color::Black);
        frame.cells[0].fg = Color::White;
        frame.cursor = Some(cursor);
        let options = PngOptions {
            scale: 1,
            text_renderer: PngTextRenderer::Bitmap,
            ..PngOptions::default()
        };
        let png = encode_frame(&frame, &options).expect("encode");
        image::load_from_memory(&png).expect("decode").to_rgb8()
    }

    fn lit(image: &image::RgbImage, x: u32, y: u32, color: Rgb8) -> bool {
        image.get_pixel(x, y).0 == [color.0, color.1, color.2]
    }

    #[test]
    fn each_cursor_shape_draws_its_own_outline() {
        use super::super::CursorState;
        let white = (255, 255, 255);
        let at = || CursorState::new(0, 0);

        let block = cursor_pixels(" ", at());
        assert!(lit(&block, 4, 8, white), "a block fills the cell");

        let hollow = cursor_pixels(" ", at().shape(CursorShape::HollowBlock));
        assert!(lit(&hollow, 0, 8, white) && lit(&hollow, 4, 0, white));
        assert!(
            !lit(&hollow, 4, 8, white),
            "a hollow block leaves its middle"
        );

        let underline = cursor_pixels(" ", at().shape(CursorShape::Underline));
        assert!(lit(&underline, 4, 15, white));
        assert!(!lit(&underline, 4, 8, white) && !lit(&underline, 0, 0, white));

        let bar = cursor_pixels(" ", at().shape(CursorShape::Bar));
        assert!(lit(&bar, 0, 8, white) && lit(&bar, 1, 8, white));
        assert!(!lit(&bar, 4, 8, white) && !lit(&bar, 7, 15, white));
    }

    #[test]
    fn a_cursor_takes_its_own_color_and_a_block_inverts_the_glyph() {
        use super::super::CursorState;
        let red = (255, 0, 0);
        let bar = cursor_pixels(
            " ",
            CursorState::new(0, 0)
                .shape(CursorShape::Bar)
                .color(Color::Rgb(255, 0, 0)),
        );
        assert!(lit(&bar, 0, 8, red));

        // With no cursor color the block is the text color, and the text turns the background's.
        let block = cursor_pixels("#", CursorState::new(0, 0));
        let black = (0, 0, 0);
        let glyph_pixels = (0..8)
            .flat_map(|x| (0..16).map(move |y| (x, y)))
            .filter(|&(x, y)| lit(&block, x, y, black))
            .count();
        assert!(
            glyph_pixels > 0,
            "the glyph is drawn in the background color"
        );
        assert!(lit(&block, 0, 0, (255, 255, 255)));
    }
}
