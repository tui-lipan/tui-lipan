//! Image underlays needed by the PNG renderer, including glyph room and the final cursor layer.

use super::{
    CapturedCell, CapturedFrame, CapturedImage, CursorShape, cell_layout, glyph_room,
    restore_image_cells,
};

pub(crate) fn visible_image_underlays(frame: &CapturedFrame) -> Vec<Vec<Option<CapturedCell>>> {
    let (cells, _) = restore_image_cells(frame);
    let occluded = opaque_cells(frame);
    let mut needed = glyph_contributors(frame, &cells, &occluded);
    let cursor = frame
        .cursor
        .as_ref()
        .filter(|cursor| cursor.visible && cursor.x < frame.width && cursor.y < frame.height);
    let cursor_index = cursor
        .map(|cursor| usize::from(cursor.y) * usize::from(frame.width) + usize::from(cursor.x));
    if cursor.is_some_and(|cursor| cursor.shape == CursorShape::Block)
        && let Some(index) = cursor_index
    {
        needed[index] = true;
    }
    frame
        .images
        .iter()
        .map(|image| {
            image
                .underlying_cells
                .iter()
                .enumerate()
                .map(|(offset, original)| {
                    image_underlay_cell(
                        image,
                        offset,
                        original,
                        &cells,
                        &needed,
                        cursor_index,
                        frame.width,
                    )
                })
                .collect()
        })
        .collect()
}

fn image_underlay_cell(
    image: &CapturedImage,
    offset: usize,
    original: &Option<CapturedCell>,
    cells: &[CapturedCell],
    needed: &[bool],
    cursor_index: Option<usize>,
    width: u16,
) -> Option<CapturedCell> {
    original.as_ref()?;
    if !image.visible.get(offset).copied().unwrap_or(false) {
        return None;
    }
    let index = image.frame_cell_offset(offset, width, cells.len())?;
    let mut cell = cells[index].clone();
    if needed[index] {
        return Some(cell);
    }
    // Other cursor shapes still need the original colors and glyph span, but not its text.
    if cursor_index == Some(index) {
        let x = (index % usize::from(width)) as u16;
        cell.symbol = if cell.span_at(x, width) == 2 {
            "\u{3000}"
        } else {
            " "
        }
        .to_string();
        return Some(cell);
    }
    None
}

fn glyph_contributors(
    frame: &CapturedFrame,
    cells: &[CapturedCell],
    occluded: &[bool],
) -> Vec<bool> {
    let mut needed = vec![false; cells.len()];
    // Unit cell dimensions let the actual renderer express glyph spans and icon room in columns.
    for (index, x, rect) in cell_layout(cells, frame.width, frame.height, 1, 1) {
        let room = glyph_room(cells, frame.width, index, x, rect);
        let columns = (room.width as usize).min(usize::from(frame.width - x));
        if (index..index + columns).all(|at| occluded[at]) {
            continue;
        }
        needed[index] = true;
        // Empty continuations and blank icon room must keep their state/style for layout replay.
        for at in index + 1..index + columns {
            needed[at] = cells[at].symbol.is_empty() || cells[at].symbol == " ";
        }
    }
    needed
}

fn opaque_cells(frame: &CapturedFrame) -> Vec<bool> {
    let mut occluded = vec![false; frame.cells.len()];
    // Aspect-fitted images can uncover glyphs with another capture font. Only cell-filling planes
    // can establish opacity independently of font/cell dimensions.
    for image in frame
        .images
        .iter()
        .filter(|image| image.z_index >= 0 && image.fill_cell_box)
    {
        for offset in 0..image.visible.len() {
            if !image.visible[offset] {
                continue;
            }
            let Some(index) = image.frame_cell_offset(offset, frame.width, occluded.len()) else {
                continue;
            };
            occluded[index] = occluded[index] || cell_opaque(image, offset);
        }
    }
    occluded
}

fn cell_opaque(image: &CapturedImage, offset: usize) -> bool {
    let columns = u64::from(image.area.w.max(1));
    let rows = u64::from(image.area.h.max(1));
    let col = offset as u64 % columns;
    let row = offset as u64 / columns;
    let width = u64::from(image.width);
    let height = u64::from(image.height);
    let left = col * width / columns;
    let right = ((col + 1) * width).div_ceil(columns);
    let top = row * height / rows;
    let bottom = ((row + 1) * height).div_ceil(rows);
    if left == right || top == bottom {
        return false;
    }
    (top..bottom).all(|y| {
        (left..right).all(|x| image.rgba.get(((y * width + x) * 4 + 3) as usize) == Some(&255))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, CursorState, PngOptions, PngTextRenderer, Rect};
    use std::sync::Arc;

    fn covered_leader(symbol: &str, next: &str, columns: u16) -> CapturedFrame {
        let cell = |symbol: &str| CapturedCell {
            symbol: symbol.to_string(),
            fg: Color::Red,
            bg: Color::Blue,
            underline_color: Color::Reset,
            modifiers: Default::default(),
        };
        let original = vec![cell(symbol), cell(next)];
        let mut frame = CapturedFrame {
            viewport: Rect {
                x: 0,
                y: 0,
                w: 2,
                h: 1,
            },
            width: 2,
            height: 1,
            cells: original.clone(),
            cursor: None,
            images: Vec::new(),
        };
        let mut image = CapturedImage::new(
            Rect {
                x: 0,
                y: 0,
                w: columns,
                h: 1,
            },
            u32::from(columns) * 8,
            16,
            Arc::from([20, 100, 220, 255].repeat(usize::from(columns) * 8 * 16)),
        );
        image.fill_cell_box = true;
        image.z_index = 1;
        image.paint_half_blocks(&mut frame.cells, frame.width, 8, 16, &original);
        frame.images.push(image);
        frame
    }

    fn assert_png_unchanged(frame: &CapturedFrame) {
        let mut redacted = frame.clone();
        for (image, cells) in redacted
            .images
            .iter_mut()
            .zip(frame.visible_image_underlays())
        {
            image.underlying_cells = cells;
        }
        for text_renderer in [PngTextRenderer::Bitmap, PngTextRenderer::Font] {
            let options = PngOptions {
                text_renderer,
                font_family: Some("JetBrainsMono Nerd Font".into()),
                ..Default::default()
            };
            assert!(
                frame.to_png(&options).unwrap() == redacted.to_png(&options).unwrap(),
                "underlay removal changed {text_renderer:?} PNG"
            );
        }
    }

    #[test]
    fn a_wide_glyph_can_contribute_through_an_image_free_continuation() {
        let frame = covered_leader("界", "", 1);
        assert_eq!(
            frame.visible_image_underlays()[0][0]
                .as_ref()
                .unwrap()
                .symbol,
            "界"
        );
        assert_png_unchanged(&frame);
    }

    #[test]
    fn a_private_use_icon_can_contribute_through_image_free_blank_room() {
        let frame = covered_leader("\u{f05b2}", " ", 1);
        assert_eq!(
            frame.visible_image_underlays()[0][0]
                .as_ref()
                .unwrap()
                .symbol,
            "\u{f05b2}"
        );
        assert_png_unchanged(&frame);
    }

    #[test]
    fn fully_occluded_text_is_removed_without_a_cursor() {
        let frame = covered_leader("S", " ", 2);
        assert!(
            frame.visible_image_underlays()[0]
                .iter()
                .all(Option::is_none)
        );
        assert_png_unchanged(&frame);
    }

    #[test]
    fn final_cursor_layers_keep_glyphs_or_anonymous_styles_as_needed() {
        for symbol in ["S", "界"] {
            for shape in [
                CursorShape::Block,
                CursorShape::HollowBlock,
                CursorShape::Underline,
                CursorShape::Bar,
            ] {
                let mut frame = covered_leader(symbol, "", 2);
                frame.cursor = Some(CursorState::new(0, 0).shape(shape));
                let cells = frame.visible_image_underlays();
                let retained = &cells[0][0].as_ref().unwrap().symbol;
                assert_eq!(retained == symbol, shape == CursorShape::Block);
                assert_png_unchanged(&frame);
            }
        }
    }
}
