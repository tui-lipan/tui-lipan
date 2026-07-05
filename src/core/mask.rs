use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Bitmap mask over terminal cells, aligned to a scope-local origin.
#[derive(Clone, Debug)]
pub struct CellMask {
    /// Top-left of the mask bitmap in scope-local cell coordinates.
    pub origin: (u16, u16),
    /// Width of the mask in cells.
    pub w: u16,
    /// Height of the mask in cells.
    pub h: u16,
    /// Row-major packed bits: index `y * w + x`, packed into `u64` words.
    pub bits: Arc<[u64]>,
}

impl PartialEq for CellMask {
    fn eq(&self, other: &Self) -> bool {
        self.origin == other.origin
            && self.w == other.w
            && self.h == other.h
            && self.bits.as_ref() == other.bits.as_ref()
    }
}

impl Eq for CellMask {}

impl Hash for CellMask {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.origin.hash(state);
        self.w.hash(state);
        self.h.hash(state);
        self.bits.as_ref().hash(state);
    }
}

impl CellMask {
    /// Returns whether `(lx, ly)` is inside the mask and the corresponding bit is set.
    ///
    /// `lx` / `ly` are in the same coordinate system as [`Self::origin`] (scope-local).
    pub fn test_scope_local(&self, lx: i16, ly: i16) -> bool {
        let ox = self.origin.0 as i32;
        let oy = self.origin.1 as i32;
        let rel_x = lx as i32 - ox;
        let rel_y = ly as i32 - oy;
        if rel_x < 0 || rel_y < 0 {
            return false;
        }
        let ux = rel_x as u32;
        let uy = rel_y as u32;
        if ux >= self.w as u32 || uy >= self.h as u32 {
            return false;
        }
        self.bit_at(ux as u16, uy as u16)
    }

    /// Hit test using coordinates relative to the mask's top-left (`origin` treated as `(0, 0)`).
    pub fn test_region_local(&self, x: u16, y: u16) -> bool {
        if x >= self.w || y >= self.h {
            return false;
        }
        self.bit_at(x, y)
    }

    fn bit_at(&self, x: u16, y: u16) -> bool {
        let idx = y as usize * self.w as usize + x as usize;
        let word = idx / 64;
        let bit = idx % 64;
        self.bits.get(word).is_some_and(|w| (w >> bit) & 1 != 0)
    }

    /// Build a mask from non-space cells in a line grid (one string per row).
    pub fn from_char_lines(lines: &[String]) -> Option<(crate::style::Rect, Self)> {
        if lines.is_empty() {
            return None;
        }
        let grid_h = lines.len() as u16;
        let grid_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        if grid_w == 0 || grid_h == 0 {
            return None;
        }

        let mut min_x = grid_w;
        let mut min_y = grid_h;
        let mut max_x = 0u16;
        let mut max_y = 0u16;
        let mut any = false;

        for (row, line) in lines.iter().enumerate() {
            for (col, c) in line.chars().enumerate() {
                if c != ' ' {
                    let x = col as u16;
                    let y = row as u16;
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        if !any {
            return None;
        }

        let w = max_x - min_x + 1;
        let h = max_y - min_y + 1;
        let total = w as usize * h as usize;
        let words = total.div_ceil(64);
        let mut v = vec![0u64; words];

        for (row, line) in lines.iter().enumerate() {
            for (col, c) in line.chars().enumerate() {
                if c == ' ' {
                    continue;
                }
                let x = col as u16;
                let y = row as u16;
                if x < min_x || y < min_y {
                    continue;
                }
                let rx = x - min_x;
                let ry = y - min_y;
                let idx = ry as usize * w as usize + rx as usize;
                v[idx / 64] |= 1u64 << (idx % 64);
            }
        }

        let rect = crate::style::Rect {
            x: min_x as i16,
            y: min_y as i16,
            w,
            h,
        };

        Some((
            rect,
            Self {
                origin: (0, 0),
                w,
                h,
                bits: v.into(),
            },
        ))
    }
}
