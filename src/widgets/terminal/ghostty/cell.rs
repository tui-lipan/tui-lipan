//! Decoding packed `GhosttyCell` values.
//!
//! Reading a row cell by cell through the library costs several FFI calls per cell, so rows are
//! read in bulk (`ROW_DATA_CELLS_RAW`, or one grid reference per row) and decoded here with the
//! bit layout of the linked library. That layout is not covered by the ABI promise: a test pins
//! it against `ghostty_type_json()`, and [`decoder`] checks it once at run time against
//! `ghostty_cell_get`, falling back to one library call per field when they disagree.

use std::sync::OnceLock;

use super::sys;

pub(crate) const CONTENT_CODEPOINT: i32 = sys::GHOSTTY_CELL_CONTENT_CODEPOINT;
pub(crate) const CONTENT_GRAPHEME: i32 = sys::GHOSTTY_CELL_CONTENT_CODEPOINT_GRAPHEME;
pub(crate) const CONTENT_BG_PALETTE: i32 = sys::GHOSTTY_CELL_CONTENT_BG_COLOR_PALETTE;
pub(crate) const CONTENT_BG_RGB: i32 = sys::GHOSTTY_CELL_CONTENT_BG_COLOR_RGB;
pub(crate) const WIDE_WIDE: i32 = sys::GHOSTTY_CELL_WIDE_WIDE;
pub(crate) const WIDE_SPACER_TAIL: i32 = sys::GHOSTTY_CELL_WIDE_SPACER_TAIL;
pub(crate) const WIDE_SPACER_HEAD: i32 = sys::GHOSTTY_CELL_WIDE_SPACER_HEAD;

/// The fields of a packed cell the builder reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Decoded {
    pub tag: i32,
    pub codepoint: u32,
    pub palette_index: u8,
    pub rgb: super::Rgb,
    pub style_id: u16,
    pub wide: i32,
    pub hyperlink: bool,
}

/// Bit layout of `GhosttyCell` in libghostty-vt 1.0.1 (`ghostty_type_json()`, kind `packed`).
pub(crate) mod layout {
    pub const CONTENT_TAG: (u32, u32) = (0, 2);
    pub const CONTENT: (u32, u32) = (2, 24);
    pub const CODEPOINT_WIDTH: u32 = 21;
    pub const STYLE_ID: (u32, u32) = (26, 16);
    pub const WIDE: (u32, u32) = (42, 2);
    pub const HYPERLINK: (u32, u32) = (45, 1);
}

fn bits(value: u64, (lsb, width): (u32, u32)) -> u64 {
    (value >> lsb) & ((1u64 << width) - 1)
}

/// Decodes a cell with the pinned layout.
pub(crate) fn decode_packed(cell: sys::GhosttyCell) -> Decoded {
    let content = bits(cell, layout::CONTENT);
    Decoded {
        tag: bits(cell, layout::CONTENT_TAG) as i32,
        codepoint: (content & ((1 << layout::CODEPOINT_WIDTH) - 1)) as u32,
        palette_index: (content & 0xFF) as u8,
        rgb: super::Rgb {
            r: (content & 0xFF) as u8,
            g: ((content >> 8) & 0xFF) as u8,
            b: ((content >> 16) & 0xFF) as u8,
        },
        style_id: bits(cell, layout::STYLE_ID) as u16,
        wide: bits(cell, layout::WIDE) as i32,
        hyperlink: bits(cell, layout::HYPERLINK) != 0,
    }
}

/// Decodes a cell through the library, one call per field.
pub(crate) fn decode_ffi(cell: sys::GhosttyCell) -> Decoded {
    /// # Safety
    ///
    /// `T` must be the documented output type for `key`, valid when zeroed.
    unsafe fn get<T>(cell: sys::GhosttyCell, key: sys::GhosttyCellData) -> T {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: a cell is plain data; `out` has the documented type.
        unsafe { sys::ghostty_cell_get(cell, key, (&raw mut out).cast()) };
        out
    }
    // SAFETY: each key's documented output type.
    unsafe {
        let tag: sys::GhosttyCellContentTag = get(cell, sys::GHOSTTY_CELL_DATA_CONTENT_TAG);
        let rgb: sys::GhosttyColorRgb = if tag == CONTENT_BG_RGB {
            get(cell, sys::GHOSTTY_CELL_DATA_COLOR_RGB)
        } else {
            sys::GhosttyColorRgb { r: 0, g: 0, b: 0 }
        };
        Decoded {
            tag,
            codepoint: get(cell, sys::GHOSTTY_CELL_DATA_CODEPOINT),
            palette_index: if tag == CONTENT_BG_PALETTE {
                get(cell, sys::GHOSTTY_CELL_DATA_COLOR_PALETTE)
            } else {
                0
            },
            rgb: rgb.into(),
            style_id: get(cell, sys::GHOSTTY_CELL_DATA_STYLE_ID),
            wide: get(cell, sys::GHOSTTY_CELL_DATA_WIDE),
            hyperlink: get(cell, sys::GHOSTTY_CELL_DATA_HAS_HYPERLINK),
        }
    }
}

/// Synthetic cells covering every field, for checking a decoder against the library.
pub(crate) fn sample_cells() -> impl Iterator<Item = sys::GhosttyCell> {
    let mut cells = Vec::new();
    for tag in 0u64..4 {
        for content in [0u64, 0x41, 0x1F600, 0x10_FFFF, 0xC0FFEE, 0xFF] {
            for style in [0u64, 1, 0x1234, 0xFFFF] {
                for wide in 0u64..4 {
                    for flags in 0u64..8 {
                        cells.push(
                            tag | (content << 2) | (style << 26) | (wide << 42) | (flags << 44),
                        );
                    }
                }
            }
        }
    }
    cells.into_iter()
}

/// Whether the packed layout agrees with the linked library, checked once.
pub(crate) fn packed_layout_ok() -> bool {
    static PACKED_MATCHES: OnceLock<bool> = OnceLock::new();
    *PACKED_MATCHES.get_or_init(|| sample_cells().step_by(7).all(packed_agrees))
}

/// The decoder to use: the packed layout when it agrees with the linked library.
pub(crate) fn decoder() -> fn(sys::GhosttyCell) -> Decoded {
    if packed_layout_ok() {
        decode_packed
    } else {
        decode_ffi
    }
}

/// Whether the packed decoder reads `cell` as the library does. Content that depends on the tag
/// is compared only under that tag.
pub(crate) fn packed_agrees(cell: sys::GhosttyCell) -> bool {
    let (p, f) = (decode_packed(cell), decode_ffi(cell));
    let content = match p.tag {
        CONTENT_BG_PALETTE => p.palette_index == f.palette_index,
        CONTENT_BG_RGB => p.rgb == f.rgb,
        _ => p.codepoint == f.codepoint,
    };
    p.tag == f.tag
        && content
        && p.style_id == f.style_id
        && p.wide == f.wide
        && p.hyperlink == f.hyperlink
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_layout_matches_the_linked_library() {
        assert_eq!(
            super::super::library_version(),
            "1.0.1",
            "re-check the packed layout"
        );
        for cell in sample_cells() {
            assert!(packed_agrees(cell), "cell {cell:#x}");
        }
        // SAFETY: a static NUL-terminated JSON string.
        let json = unsafe { std::ffi::CStr::from_ptr(sys::ghostty_type_json()) };
        let types: serde_json::Value = serde_json::from_slice(json.to_bytes()).unwrap();
        let bits = &types["types"]["GhosttyCell"]["bits"];
        let field = |name: &str| {
            (
                bits[name]["lsb"].as_u64().unwrap() as u32,
                bits[name]["width"].as_u64().unwrap() as u32,
            )
        };
        assert_eq!(field("content_tag"), layout::CONTENT_TAG);
        assert_eq!(field("content"), layout::CONTENT);
        assert_eq!(field("style_id"), layout::STYLE_ID);
        assert_eq!(field("wide"), layout::WIDE);
        assert_eq!(field("hyperlink"), layout::HYPERLINK);
        let codepoint = &bits["content"]["arms"]["CODEPOINT"]["bits"]["codepoint"];
        assert_eq!(
            codepoint["width"].as_u64(),
            Some(u64::from(layout::CODEPOINT_WIDTH))
        );
    }
}
