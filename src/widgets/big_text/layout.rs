use std::sync::Arc;

use crate::core::mask::CellMask;
use crate::style::Rect;

use super::{BigFont, BigText, BigTextRenderOutput};

/// One glyph’s ink bounds and per-cell mask, relative to the [`BigText`](super::BigText) top-left.
#[derive(Clone)]
pub struct GlyphLayout {
    /// Source character.
    pub ch: char,
    /// Tight bounding rectangle of non-blank cells for this glyph.
    pub rect: Rect,
    /// Lit cells for this glyph (scope-local; [`CellMask::origin`] matches [`Self::rect`] top-left).
    pub mask: Arc<CellMask>,
}

fn matrix_from_output(out: &BigTextRenderOutput) -> Vec<Vec<char>> {
    let w = out.width as usize;
    let h = out.height as usize;
    let mut m = vec![vec![' '; w]; h];
    for (y, spans) in out.lines.iter().enumerate() {
        if y >= h {
            break;
        }
        let mut x = 0usize;
        for sp in spans {
            for ch in sp.content.chars() {
                if x < w {
                    m[y][x] = ch;
                    x += 1;
                }
            }
        }
    }
    m
}

fn column_ink_flags(matrix: &[Vec<char>]) -> Vec<bool> {
    let w = matrix.first().map(|r| r.len()).unwrap_or(0);
    let mut v = vec![false; w];
    for (x, has_ink) in v.iter_mut().enumerate() {
        for row in matrix.iter() {
            if row.get(x).copied().unwrap_or(' ') != ' ' {
                *has_ink = true;
                break;
            }
        }
    }
    v
}

fn ink_column_segments(col_ink: &[bool]) -> Vec<(usize, usize)> {
    let w = col_ink.len();
    let mut i = 0;
    let mut segs = Vec::new();
    while i < w {
        while i < w && !col_ink[i] {
            i += 1;
        }
        if i >= w {
            break;
        }
        let a = i;
        while i < w && col_ink[i] {
            i += 1;
        }
        segs.push((a, i));
    }
    segs
}

fn ink_x_bounds(col_ink: &[bool]) -> Option<(usize, usize)> {
    let x_min = col_ink.iter().position(|&k| k)?;
    let x_max = col_ink.len() - 1 - col_ink.iter().rev().position(|&k| k)?;
    Some((x_min, x_max))
}

fn equal_column_ranges(a: usize, b: usize, n: usize) -> Vec<(usize, usize)> {
    if n == 0 {
        return Vec::new();
    }
    let len = b.saturating_sub(a);
    (0..n)
        .map(|i| {
            let s = a + (len * i) / n;
            let e = a + (len * (i + 1)) / n;
            (s, e)
        })
        .collect()
}

fn standalone_char_widths(bt: &BigText, chars: &[char]) -> Vec<usize> {
    chars
        .iter()
        .map(|ch| {
            let mut solo = BigText::new()
                .font(bt.font)
                .style(bt.style)
                .shadow(bt.shadow);
            if let Some(ref cf) = bt.custom_figlet {
                solo = solo.custom_figlet(cf.clone());
            }
            let out = solo.text(ch.to_string()).build_lines();
            out.width as usize
        })
        .collect()
}

fn partition_proportional_to_widths(
    widths: &[usize],
    x_min: usize,
    x_max: usize,
) -> Vec<(usize, usize)> {
    let n = widths.len();
    let full_w = x_max.saturating_sub(x_min).saturating_add(1);
    if n == 0 {
        return Vec::new();
    }
    let sum: usize = widths.iter().sum();
    if sum == 0 {
        return equal_column_ranges(x_min, x_max + 1, n);
    }
    let raw: Vec<f64> = widths
        .iter()
        .map(|w| (*w as f64 / sum as f64) * full_w as f64)
        .collect();
    let mut floors: Vec<usize> = raw.iter().map(|x| x.floor() as usize).collect();
    let rem = full_w.saturating_sub(floors.iter().sum());
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let ra = raw[a] - floors[a] as f64;
        let rb = raw[b] - floors[b] as f64;
        rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
    });
    for idx in order.iter().take(rem) {
        floors[*idx] += 1;
    }
    let mut x = x_min;
    let mut ranges = Vec::with_capacity(n);
    for w in floors {
        let x0 = x;
        let x1 = x.saturating_add(w);
        ranges.push((x0, x1));
        x = x1;
    }
    ranges
}

fn partition_column_ranges(
    col_ink: &[bool],
    n: usize,
    bt: &BigText,
    chars: &[char],
) -> Vec<(usize, usize)> {
    if n == 0 {
        return Vec::new();
    }
    let w = col_ink.len();
    if w == 0 {
        return vec![(0, 0); n];
    }
    let Some((x_min, x_max)) = ink_x_bounds(col_ink) else {
        return vec![(0, 0); n];
    };

    let segs = ink_column_segments(col_ink);
    if segs.len() == n {
        return segs;
    }
    let widths = standalone_char_widths(bt, chars);
    partition_proportional_to_widths(&widths, x_min, x_max)
}

fn layout_glyphs_for_line(bt: &BigText, line: &str, y_base: i16) -> (Vec<GlyphLayout>, u16) {
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    if n == 0 {
        return (Vec::new(), 0);
    }

    let out = bt.build_lines();
    if out.width == 0 || out.height == 0 {
        return (Vec::new(), 0);
    }

    let h = out.height;
    let matrix = matrix_from_output(&out);
    let col_ink = column_ink_flags(&matrix);
    let ranges = partition_column_ranges(&col_ink, n, bt, &chars);

    let mut layouts = Vec::with_capacity(n);
    for (i, ch) in chars.iter().enumerate() {
        let (x0, x1) = ranges[i];
        let sub: Vec<String> = matrix
            .iter()
            .map(|row| row.get(x0..x1).unwrap_or_default().iter().collect())
            .collect();

        let (abs_rect, mask) =
            if let Some((local_ink, mask_inner)) = CellMask::from_char_lines(&sub) {
                let abs_rect = Rect {
                    x: x0 as i16 + local_ink.x,
                    y: local_ink.y.saturating_add(y_base),
                    w: local_ink.w,
                    h: local_ink.h,
                };
                let mask = Arc::new(CellMask {
                    origin: (abs_rect.x.max(0) as u16, abs_rect.y.max(0) as u16),
                    w: mask_inner.w,
                    h: mask_inner.h,
                    bits: mask_inner.bits,
                });
                (abs_rect, mask)
            } else {
                let cw = x1.saturating_sub(x0).max(1) as u16;
                let abs_rect = Rect {
                    x: x0 as i16,
                    y: y_base,
                    w: cw,
                    h,
                };
                let cells = cw as usize * h as usize;
                let words = cells.div_ceil(64);
                let bits: Arc<[u64]> = vec![0u64; words].into();
                let mask = Arc::new(CellMask {
                    origin: (abs_rect.x.max(0) as u16, abs_rect.y.max(0) as u16),
                    w: cw,
                    h,
                    bits,
                });
                (abs_rect, mask)
            };

        layouts.push(GlyphLayout {
            ch: *ch,
            rect: abs_rect,
            mask,
        });
    }

    (layouts, h)
}

impl BigText {
    /// Lay out glyphs using the same raster as the internal `BigText::build_lines` path
    /// for each line (split on `'\n'`).
    ///
    /// Column bands: when FIGlet leaves at least one fully blank column between letters, each contiguous
    /// ink run maps to one character in order - **exact** alignment. When letters touch (no blank
    /// column), the ink span is split using **each character’s standalone FIGlet width** (same font and
    /// style as the line) scaled to the full raster width - a better fit than equal slices, though it
    /// still may not match smushed layout exactly for every font. Standalone widths usually sum to more
    /// than the fused line width; rescaling is intentional. Masks from `from_char_lines` still crop to
    /// actual ink, so a band can be slightly wider than visible glyph ink without painting outside it.
    ///
    /// Blank lines in `text` advance the vertical origin by one row each before the next non-empty
    /// line (so `"A\n\nB"` inserts a single empty row between blocks).
    pub fn layout_glyphs(text: &str, font: BigFont) -> Vec<GlyphLayout> {
        let mut out = Vec::new();
        let mut y_base = 0i16;

        for line in text.split('\n') {
            if line.is_empty() {
                y_base = y_base.saturating_add(1);
                continue;
            }

            let line_owned = line.to_string();
            let bt = BigText::new().font(font).text(line_owned.clone());
            let (row, row_h) = layout_glyphs_for_line(&bt, &line_owned, y_base);
            let row_h = row_h as i16;
            y_base = y_base.saturating_add(row_h.saturating_add(1));
            out.extend(row);
        }

        out
    }
}

pub fn measure_big_text(big_text: &BigText) -> (u16, u16) {
    let output = big_text.build_lines();
    (output.width, output.height)
}

#[cfg(test)]
mod tests {
    use super::{BigFont, BigText};

    #[test]
    fn layout_glyphs_partition_covers_ink_exactly_once() {
        let glyphs = BigText::layout_glyphs("ZAP", BigFont::AnsiShadow);
        let bt = BigText::new().font(BigFont::AnsiShadow).text("ZAP");
        let out = bt.build_lines();
        assert_eq!(glyphs.len(), 3);
        assert!(out.width > 0 && out.height > 0);

        let w = out.width as usize;
        let h = out.height as usize;
        let matrix = super::matrix_from_output(&out);

        let mut ink: Vec<(i16, i16)> = Vec::new();
        for (y, row) in matrix.iter().enumerate().take(h) {
            for (x, ch) in row.iter().enumerate().take(w) {
                if *ch != ' ' {
                    ink.push((x as i16, y as i16));
                }
            }
        }

        for (x, y) in &ink {
            let n = glyphs
                .iter()
                .filter(|g| g.mask.test_scope_local(*x, *y))
                .count();
            assert_eq!(n, 1, "cell ({x},{y}) covered {n} times");
        }

        for g in &glyphs {
            for (y, row) in matrix.iter().enumerate().take(h) {
                for (x, ch) in row.iter().enumerate().take(w) {
                    if g.mask.test_scope_local(x as i16, y as i16) {
                        assert_ne!(*ch, ' ');
                    }
                }
            }
        }

        let max_right = glyphs
            .iter()
            .map(|g| g.rect.x as i32 + i32::from(g.rect.w))
            .max()
            .unwrap();
        assert!(max_right <= out.width as i32);
        let min_left = glyphs.iter().map(|g| g.rect.x).min().unwrap();
        assert!(min_left >= 0);
    }

    #[test]
    fn layout_glyphs_tui_lipan_partition_covers_ink_exactly_once() {
        let glyphs = BigText::layout_glyphs("TUI-LIPAN", BigFont::AnsiShadow);
        let bt = BigText::new().font(BigFont::AnsiShadow).text("TUI-LIPAN");
        let out = bt.build_lines();
        let n = "TUI-LIPAN".chars().count();
        assert_eq!(glyphs.len(), n);
        assert!(out.width > 0 && out.height > 0);

        let w = out.width as usize;
        let h = out.height as usize;
        let matrix = super::matrix_from_output(&out);

        let mut ink: Vec<(i16, i16)> = Vec::new();
        for (y, row) in matrix.iter().enumerate().take(h) {
            for (x, ch) in row.iter().enumerate().take(w) {
                if *ch != ' ' {
                    ink.push((x as i16, y as i16));
                }
            }
        }

        for (x, y) in &ink {
            let c = glyphs
                .iter()
                .filter(|g| g.mask.test_scope_local(*x, *y))
                .count();
            assert_eq!(c, 1, "cell ({x},{y}) covered {c} times");
        }

        for g in &glyphs {
            for (y, row) in matrix.iter().enumerate().take(h) {
                for (x, ch) in row.iter().enumerate().take(w) {
                    if g.mask.test_scope_local(x as i16, y as i16) {
                        assert_ne!(*ch, ' ');
                    }
                }
            }
        }
    }

    #[test]
    fn layout_glyphs_wi_fused_bands_differ_from_even_column_split() {
        let glyphs = BigText::layout_glyphs("WI", BigFont::AnsiShadow);
        assert_eq!(glyphs.len(), 2);
        assert_ne!(
            glyphs[0].rect.w, glyphs[1].rect.w,
            "tight ink rects should differ for W vs I"
        );

        let bt = BigText::new().font(BigFont::AnsiShadow).text("WI");
        let chars: Vec<char> = "WI".chars().collect();
        let out = bt.build_lines();
        let matrix = super::matrix_from_output(&out);
        let col_ink = super::column_ink_flags(&matrix);
        let Some((x_min, x_max)) = super::ink_x_bounds(&col_ink) else {
            panic!("expected ink");
        };
        let prop = super::partition_column_ranges(&col_ink, 2, &bt, &chars);
        let even = super::equal_column_ranges(x_min, x_max + 1, 2);
        assert_ne!(
            prop, even,
            "WI in AnsiShadow: column bands should not match an even split of the ink span"
        );
    }
}
