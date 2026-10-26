//! Relaying a replacement as edits of the image readers already hold.
//!
//! A browser pane sends the whole viewport for every change, however small. When readers already
//! hold the previous frame, the changed rectangles say the same in a fraction of the bytes: Kitty's
//! root-frame edit (`a=f,r=1`) overwrites each one, then the image's placements are retired and its
//! placement made again, which is what a retransmission does. The result depends on the previous
//! frame, so the caller records it as ordered output rather than as a replaceable one.

use super::graphics::{transmission_keys, write_chunked};

/// Rows compared as one unit. Each changed band becomes one rectangle before merging.
const BAND_ROWS: usize = 16;
/// Past this share of the frame, a whole frame costs about the same and can still be coalesced.
const MAX_CHANGED_PERCENT: usize = 70;
/// Rectangles beyond this are merged with a neighbour, so commands stay few.
const MAX_RECTS: usize = 16;
/// Bytes compared at once while looking for the first or last difference in a row.
const BLOCK: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    fn area(self) -> usize {
        self.width * self.height
    }

    fn union(self, other: Self) -> Self {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }

    /// Whether replacing the two with their union draws little that did not change.
    fn merges_cheaply(self, other: Self) -> bool {
        self.union(other).area() * 4 <= (self.area() + other.area()) * 5
    }
}

/// The edits that turn `old` into `new`, both `width` pixels wide in `format` (24 or 32), for the
/// transmission `wire` of image `id`. `places` is whether the transmission also placed the image.
/// `None` when so much changed that the whole frame is the better thing to send.
pub(super) fn edit_transmission(
    wire: &[u8],
    places: bool,
    id: u32,
    format: u32,
    width: u32,
    old: &[u8],
    new: &[u8],
) -> Option<Vec<u8>> {
    let channels = if format == 24 { 3 } else { 4 };
    let width = width as usize;
    let stride = width.checked_mul(channels).filter(|stride| *stride > 0)?;
    if old.len() != new.len() || !new.len().is_multiple_of(stride) {
        return None;
    }
    let rects = changed_rects(old, new, width, new.len() / stride, channels)?;
    let (_, keys) = transmission_keys(wire);
    let mut out = Vec::new();
    for rect in rects {
        let compressed = super::zlib_bands::compress(&gather(new, stride, channels, rect))?;
        let header = format!(
            "a=f,r=1,i={id},x={},y={},s={},v={},f={format},X=1,o=z,q=2",
            rect.x, rect.y, rect.width, rect.height
        );
        write_chunked(
            &mut out,
            &[header.as_bytes()],
            Some(b"a=f"),
            Some(b"2"),
            &compressed,
        );
    }
    out.extend_from_slice(format!("\x1b_Ga=d,d=i,i={id},q=2\x1b\\").as_bytes());
    if places {
        // Everything that says where and how to place it; the rest described the pixels.
        let display: Vec<&[u8]> = keys
            .into_iter()
            .filter(|pair| !matches!(pair, [b'a' | b'f' | b's' | b'v' | b'o' | b'i', b'=', ..]))
            .collect();
        out.extend_from_slice(format!("\x1b_Ga=p,i={id}").as_bytes());
        for pair in display {
            out.push(b',');
            out.extend_from_slice(pair);
        }
        out.extend_from_slice(b"\x1b\\");
    }
    Some(out)
}

/// Rectangles covering every pixel that differs, or `None` when they would cover too much.
fn changed_rects(
    old: &[u8],
    new: &[u8],
    width: usize,
    height: usize,
    channels: usize,
) -> Option<Vec<Rect>> {
    let stride = width * channels;
    let mut rects: Vec<Rect> = Vec::new();
    for band in (0..height).step_by(BAND_ROWS) {
        let mut extent: Option<Rect> = None;
        for row in band..(band + BAND_ROWS).min(height) {
            let range = row * stride..(row + 1) * stride;
            let (a, b) = (&old[range.clone()], &new[range]);
            let Some(first) = first_difference(a, b) else {
                continue;
            };
            let last = last_difference(a, b).unwrap_or(first);
            let changed = Rect {
                x: first / channels,
                y: row,
                width: last / channels + 1 - first / channels,
                height: 1,
            };
            extent = Some(extent.map_or(changed, |extent| extent.union(changed)));
        }
        let Some(rect) = extent else {
            continue;
        };
        match rects.last_mut() {
            Some(previous) if previous.merges_cheaply(rect) => *previous = previous.union(rect),
            _ => rects.push(rect),
        }
    }
    while rects.len() > MAX_RECTS {
        let index = (1..rects.len())
            .min_by_key(|&index| rects[index - 1].union(rects[index]).area())
            .expect("more than one rectangle");
        rects[index - 1] = rects[index - 1].union(rects[index]);
        rects.remove(index);
    }
    let changed: usize = rects.iter().map(|rect| rect.area()).sum();
    (changed * 100 <= width * height * MAX_CHANGED_PERCENT).then_some(rects)
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    let block = a
        .chunks(BLOCK)
        .zip(b.chunks(BLOCK))
        .position(|(x, y)| x != y)?;
    let start = block * BLOCK;
    a[start..]
        .iter()
        .zip(&b[start..])
        .position(|(x, y)| x != y)
        .map(|offset| start + offset)
}

fn last_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    // Blocks counted from the end, so the comparison stays aligned with the row's last byte.
    let block = a
        .rchunks(BLOCK)
        .zip(b.rchunks(BLOCK))
        .position(|(x, y)| x != y)?;
    let end = a.len() - block * BLOCK;
    a[..end].iter().zip(&b[..end]).rposition(|(x, y)| x != y)
}

/// The pixels of `rect`, row after row.
fn gather(pixels: &[u8], stride: usize, channels: usize, rect: Rect) -> Vec<u8> {
    let row = rect.width * channels;
    let mut out = Vec::with_capacity(row * rect.height);
    for y in rect.y..rect.y + rect.height {
        let start = y * stride + rect.x * channels;
        out.extend_from_slice(&pixels[start..start + row]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, seed: u8) -> Vec<u8> {
        (0..width * height * 4)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn paint(pixels: &mut [u8], width: usize, rect: Rect, value: u8) {
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                pixels[(y * width + x) * 4..][..4].fill(value);
            }
        }
    }

    #[test]
    fn rectangles_cover_exactly_what_changed_and_nothing_far_from_it() {
        let (width, height) = (200, 120);
        let old = frame(width, height, 0);
        let mut new = old.clone();
        let small = Rect {
            x: 17,
            y: 5,
            width: 3,
            height: 2,
        };
        let far = Rect {
            x: 150,
            y: 90,
            width: 40,
            height: 20,
        };
        paint(&mut new, width, small, 1);
        paint(&mut new, width, far, 2);
        let rects = changed_rects(&old, &new, width, height, 4).unwrap();
        assert_eq!(rects, [small, far]);
    }

    #[test]
    fn an_unchanged_frame_needs_no_pixels_and_a_rewritten_one_is_sent_whole() {
        let old = frame(64, 64, 0);
        assert_eq!(changed_rects(&old, &old, 64, 64, 4), Some(Vec::new()));
        let new = frame(64, 64, 1);
        assert_eq!(changed_rects(&old, &new, 64, 64, 4), None);
    }

    #[test]
    fn differences_are_found_at_every_offset() {
        let a = vec![0u8; 3 * BLOCK + 5];
        for index in 0..a.len() {
            let mut b = a.clone();
            b[index] = 1;
            assert_eq!(first_difference(&a, &b), Some(index));
            assert_eq!(last_difference(&a, &b), Some(index));
        }
        assert_eq!(first_difference(&a, &a), None);
        assert_eq!(last_difference(&a, &a), None);
    }

    #[test]
    fn many_scattered_changes_are_merged_down_to_the_limit() {
        let (width, height) = (64, BAND_ROWS * (MAX_RECTS + 8));
        let old = vec![0u8; width * height * 4];
        let mut new = old.clone();
        for band in 0..MAX_RECTS + 8 {
            // Alternate far-apart columns so neighbours never merge cheaply.
            let x = if band % 2 == 0 { 0 } else { width - 1 };
            paint(
                &mut new,
                width,
                Rect {
                    x,
                    y: band * BAND_ROWS,
                    width: 1,
                    height: 1,
                },
                9,
            );
        }
        let rects = changed_rects(&old, &new, width, height, 4).unwrap();
        assert!(rects.len() <= MAX_RECTS);
        // Every changed pixel is still covered.
        for band in 0..MAX_RECTS + 8 {
            let x = if band % 2 == 0 { 0 } else { width - 1 };
            let y = band * BAND_ROWS;
            assert!(
                rects
                    .iter()
                    .any(|rect| (rect.x..rect.x + rect.width).contains(&x)
                        && (rect.y..rect.y + rect.height).contains(&y))
            );
        }
    }
}
