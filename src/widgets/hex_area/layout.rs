use super::HexArea;

pub fn measure_hex_area(hex_area: &HexArea) -> (u16, u16) {
    let bytes_per_row = hex_area.bytes_per_row.max(1) as usize;
    let total_rows = hex_area.bytes.len().div_ceil(bytes_per_row).max(1);

    let offsets_width = if hex_area.show_offsets {
        10usize
    } else {
        0usize
    };
    let hex_width = bytes_per_row
        .saturating_mul(2)
        .saturating_add(bytes_per_row.saturating_sub(1));
    let ascii_width = if hex_area.show_ascii {
        2usize.saturating_add(bytes_per_row)
    } else {
        0usize
    };

    let content_width = offsets_width
        .saturating_add(hex_width)
        .saturating_add(ascii_width);
    let content_height = total_rows;

    let border_pad = if hex_area.border { 2usize } else { 0usize };
    let width = content_width
        .saturating_add(hex_area.padding.horizontal() as usize)
        .saturating_add(border_pad)
        .min(u16::MAX as usize) as u16;
    let height = content_height
        .saturating_add(hex_area.padding.vertical() as usize)
        .saturating_add(border_pad)
        .min(u16::MAX as usize) as u16;

    (width, height)
}
