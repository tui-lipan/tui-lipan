use unicode_width::UnicodeWidthStr;

use super::AsciiCanvas;

pub(crate) fn measure_ascii_canvas(
    canvas: &AsciiCanvas,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    // Sequence mode: size comes from the sequence dimensions.
    if let Some(ref seq) = canvas.sequence {
        let w = seq.width();
        let h = seq.height();
        let w = max_w.map(|mw| w.min(mw)).unwrap_or(w);
        let h = max_h.map(|mh| h.min(mh)).unwrap_or(h);
        return (w, h);
    }

    if canvas.lines.is_empty() {
        if let Some((w, h)) = canvas.grid_size {
            let w = max_w.map(|mw| w.min(mw)).unwrap_or(w);
            let h = max_h.map(|mh| h.min(mh)).unwrap_or(h);
            return (w, h);
        }
        return (0, 0);
    }

    let mut width = 0usize;
    let mut height = 0usize;

    for line in &canvas.lines {
        width = width.max(UnicodeWidthStr::width(line.as_ref()));
        height = height.saturating_add(1);
    }

    if let Some((grid_w, grid_h)) = canvas.grid_size {
        width = width.max(grid_w as usize);
        height = height.max(grid_h as usize);
    }

    if let Some(max_w) = max_w {
        width = width.min(max_w as usize);
    }
    if let Some(max_h) = max_h {
        height = height.min(max_h as usize);
    }

    let w = width.min(u16::MAX as usize) as u16;
    let h = height.min(u16::MAX as usize) as u16;
    (w, h)
}
