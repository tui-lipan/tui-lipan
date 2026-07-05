//! Braille glyph utilities for 2x4 sub-cell drawing.

pub(crate) const BRAILLE_LEFT_FILL_BOTTOM_UP: [u8; 5] = [0x00, 0x40, 0x44, 0x46, 0x47];
pub(crate) const BRAILLE_RIGHT_FILL_BOTTOM_UP: [u8; 5] = [0x00, 0x80, 0xA0, 0xB0, 0xB8];
pub(crate) const BRAILLE_LEFT_FILL_TOP_DOWN: [u8; 5] = [0x00, 0x01, 0x03, 0x07, 0x47];
pub(crate) const BRAILLE_RIGHT_FILL_TOP_DOWN: [u8; 5] = [0x00, 0x08, 0x18, 0x38, 0xB8];

/// Braille dot bit for each `(row, col)` in a 2x4 cell.
pub const BRAILLE_PIXEL_BITS: [[u8; 2]; 4] =
    [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

/// Return the braille character for a dot mask, using a space for the empty mask.
pub fn braille_char(mask: u8) -> char {
    if mask == 0 {
        ' '
    } else {
        char::from_u32(0x2800 + mask as u32).unwrap_or(' ')
    }
}

/// Return the mask for a vertical fill level in one half of a braille cell.
pub fn braille_fill_mask(level: usize, is_left: bool, mirror_y: bool) -> u8 {
    let level = level.min(4);
    if mirror_y {
        if is_left {
            BRAILLE_LEFT_FILL_TOP_DOWN[level]
        } else {
            BRAILLE_RIGHT_FILL_TOP_DOWN[level]
        }
    } else if is_left {
        BRAILLE_LEFT_FILL_BOTTOM_UP[level]
    } else {
        BRAILLE_RIGHT_FILL_BOTTOM_UP[level]
    }
}

/// Set a single sub-cell pixel in a braille mask.
pub fn set_pixel(mask: u8, sub_x: u8, sub_y: u8) -> u8 {
    BRAILLE_PIXEL_BITS
        .get(sub_y as usize)
        .and_then(|row| row.get(sub_x as usize))
        .map_or(mask, |bit| mask | bit)
}

/// Clear a single sub-cell pixel in a braille mask.
pub fn clear_pixel(mask: u8, sub_x: u8, sub_y: u8) -> u8 {
    BRAILLE_PIXEL_BITS
        .get(sub_y as usize)
        .and_then(|row| row.get(sub_x as usize))
        .map_or(mask, |bit| mask & !bit)
}

/// Visit all sub-cell pixels along a Bresenham line, including both endpoints.
pub fn line_pixels(from: (i32, i32), to: (i32, i32), mut visit: impl FnMut(i32, i32)) {
    let (mut x0, mut y0) = from;
    let (x1, y1) = to;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        visit(x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = err.saturating_mul(2);
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_clear_pixel_use_standard_dot_layout() {
        let mask = set_pixel(0, 0, 0);
        assert_eq!(mask, 0x01);

        let mask = set_pixel(mask, 1, 3);
        assert_eq!(mask, 0x81);

        assert_eq!(clear_pixel(mask, 0, 0), 0x80);
        assert_eq!(set_pixel(mask, 2, 0), mask);
        assert_eq!(clear_pixel(mask, 0, 4), mask);
    }

    #[test]
    fn line_pixels_visits_bresenham_points() {
        let mut points = Vec::new();
        line_pixels((0, 0), (3, 5), |x, y| points.push((x, y)));
        assert_eq!(points, vec![(0, 0), (1, 1), (1, 2), (2, 3), (2, 4), (3, 5)]);
    }

    #[test]
    fn line_pixels_visits_reverse_lines() {
        let mut points = Vec::new();
        line_pixels((3, 5), (0, 0), |x, y| points.push((x, y)));
        assert_eq!(points, vec![(3, 5), (2, 4), (2, 3), (1, 2), (1, 1), (0, 0)]);
    }
}
