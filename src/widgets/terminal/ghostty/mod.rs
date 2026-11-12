//! Safe, thin wrapper over libghostty-vt (`ghostty-vt-sys`), the engine behind the experimental
//! `terminal-ghostty` backend of [`TerminalScreen`](super::TerminalScreen).
//!
//! The wrapper exposes only what a screen backend needs:
//!
//! - [`Terminal`]: feed bytes, resize, read modes and state, scroll the viewport, limit and
//!   compress scrollback, and register effect callbacks as Rust closures.
//! - [`RenderState`]: a per-frame copy of the viewport with dirty rows, cells, graphemes,
//!   styles, wide/wrap flags, the cursor and the colors.
//! - [`TrackedRef`]: a cell anchor that follows scrolling, reflow and pruning.
//!
//! # Safety model
//!
//! libghostty-vt is single-threaded and not reentrant. Every wrapper type holds a raw pointer,
//! so none is `Send` or `Sync`. Mutating calls take `&mut Terminal`, and callbacks receive only
//! copies or borrowed data, never the terminal, so a callback cannot write to the terminal that
//! is calling it. A panic inside a callback is caught at the FFI boundary and resumed once the
//! library call returns. Data borrowed from the library is tied to the borrow of the object that
//! owns it, which is exactly as long as the library keeps it valid.

// The wrapper covers more of the C API than the engine uses: the OSC 133, OSC 7501, pwd,
// render-hold and XTVERSION callbacks and parts of `RenderState` are exercised only by this
// module's tests, because the engine reads those sequences with its shared semantic parser.
#![allow(dead_code, unused_imports)]
// The FFI boundary. Every unsafe block states the C contract it relies on.
#![allow(unsafe_code)]

mod callbacks;
pub(crate) mod cell;
mod render;
mod terminal;
#[cfg(test)]
mod tests;

use ghostty_vt_sys as sys;

pub(crate) use callbacks::{
    ClipboardContent, ClipboardLocation, ClipboardWrite, ClipboardWriteReply, ClipboardWriteResult,
    ProgramStatus, ProgramStatusKind, ProgramStatusState, PromptKind, SemanticPrompt,
    SemanticPromptKind, SizeReport,
};
pub(crate) use render::{
    CellRef, CellWide, Cells, Colors, Cursor, CursorShape, Dirty, RenderState, Row, RowId, Rows,
    SemanticContent,
};
pub(crate) use terminal::{
    CompressionMode, CompressionResult, GridRow, MemoryUsage, Mode, Point, Screen, ScrollViewport,
    ScrollbackLimits, Scrollbar, Terminal, TrackedRef,
};

/// A failed libghostty-vt call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum GhosttyError {
    /// The library could not allocate memory.
    #[error("libghostty-vt: out of memory")]
    OutOfMemory,
    /// An argument was rejected, such as a zero size or an out-of-range point.
    #[error("libghostty-vt: invalid value")]
    InvalidValue,
    /// The linked library does not support the request.
    #[error("libghostty-vt: unsupported")]
    Unsupported,
    /// Any other result code.
    #[error("libghostty-vt: error {0}")]
    Other(i32),
}

/// Maps a result code to `Ok(())` or the error. `GHOSTTY_NO_VALUE` is an error here; callers
/// that expect it check for it first with [`no_value`].
fn check(result: sys::GhosttyResult) -> Result<(), GhosttyError> {
    match result {
        sys::GHOSTTY_SUCCESS => Ok(()),
        sys::GHOSTTY_OUT_OF_MEMORY => Err(GhosttyError::OutOfMemory),
        sys::GHOSTTY_INVALID_VALUE => Err(GhosttyError::InvalidValue),
        sys::GHOSTTY_UNSUPPORTED => Err(GhosttyError::Unsupported),
        other => Err(GhosttyError::Other(other)),
    }
}

/// Maps `GHOSTTY_NO_VALUE` to `Ok(None)`, success to `Ok(Some(value))`.
fn no_value<T>(result: sys::GhosttyResult, value: T) -> Result<Option<T>, GhosttyError> {
    if result == sys::GHOSTTY_NO_VALUE {
        return Ok(None);
    }
    check(result).map(|()| Some(value))
}

/// An RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<sys::GhosttyColorRgb> for Rgb {
    fn from(c: sys::GhosttyColorRgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

impl From<Rgb> for sys::GhosttyColorRgb {
    fn from(c: Rgb) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
        }
    }
}

/// A color in a cell style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum StyleColor {
    /// The terminal default.
    #[default]
    Default,
    /// An index into the 256-color palette.
    Palette(u8),
    /// A direct color.
    Rgb(Rgb),
}

impl StyleColor {
    fn from_raw(color: sys::GhosttyStyleColor) -> Self {
        // SAFETY: `tag` says which union arm the library wrote; both arms are plain bytes.
        unsafe {
            match i32::from(color.tag) {
                sys::GHOSTTY_STYLE_COLOR_PALETTE => Self::Palette(color.value.palette),
                sys::GHOSTTY_STYLE_COLOR_RGB => Self::Rgb(color.value.rgb.into()),
                _ => Self::Default,
            }
        }
    }
}

/// Underline style (SGR 4 and its sub-parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// The complete visual style of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Style {
    pub fg: StyleColor,
    pub bg: StyleColor,
    pub underline_color: StyleColor,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub underline: Underline,
}

impl Style {
    fn from_raw(style: &sys::GhosttyStyle) -> Self {
        let has = |flag: sys::GhosttyStyleFlags| style.flags & flag != 0;
        let underline = match sys::ghostty_style_underline(style) {
            sys::GHOSTTY_SGR_UNDERLINE_NONE => Underline::None,
            sys::GHOSTTY_SGR_UNDERLINE_DOUBLE => Underline::Double,
            sys::GHOSTTY_SGR_UNDERLINE_CURLY => Underline::Curly,
            sys::GHOSTTY_SGR_UNDERLINE_DOTTED => Underline::Dotted,
            sys::GHOSTTY_SGR_UNDERLINE_DASHED => Underline::Dashed,
            _ => Underline::Single,
        };
        Self {
            fg: StyleColor::from_raw(style.fg_color),
            bg: StyleColor::from_raw(style.bg_color),
            underline_color: StyleColor::from_raw(style.underline_color),
            bold: has(sys::GHOSTTY_STYLE_FLAG_BOLD),
            italic: has(sys::GHOSTTY_STYLE_FLAG_ITALIC),
            faint: has(sys::GHOSTTY_STYLE_FLAG_FAINT),
            blink: has(sys::GHOSTTY_STYLE_FLAG_BLINK),
            inverse: has(sys::GHOSTTY_STYLE_FLAG_INVERSE),
            invisible: has(sys::GHOSTTY_STYLE_FLAG_INVISIBLE),
            strikethrough: has(sys::GHOSTTY_STYLE_FLAG_STRIKETHROUGH),
            overline: has(sys::GHOSTTY_STYLE_FLAG_OVERLINE),
            underline,
        }
    }
}

/// Borrows the bytes of a library string. Empty or NULL strings give an empty slice.
///
/// # Safety
///
/// `s` must come from the library and point at `len` readable bytes for `'a`, which the caller
/// bounds by the lifetime the library documents for it.
unsafe fn string_bytes<'a>(s: sys::GhosttyString) -> &'a [u8] {
    if s.ptr.is_null() || s.len == 0 {
        return &[];
    }
    // SAFETY: guaranteed by the caller.
    unsafe { std::slice::from_raw_parts(s.ptr, s.len) }
}

/// A sized struct, zeroed with its leading `size` field set (`GHOSTTY_INIT_SIZED`).
///
/// # Safety
///
/// `T` must be a libghostty-vt sized struct: plain data whose first field is `size: usize`, for
/// which all-zero bytes are a valid value.
unsafe fn sized<T>() -> T {
    let mut value = std::mem::MaybeUninit::<T>::zeroed();
    // SAFETY: all-zero is valid for T (caller contract), and `ghostty_sized_init` writes only
    // the size field within `size_of::<T>()` bytes.
    unsafe {
        sys::ghostty_sized_init(value.as_mut_ptr().cast(), std::mem::size_of::<T>());
        value.assume_init()
    }
}

/// The version of the linked library, such as `1.0.1`.
pub(crate) fn library_version() -> &'static str {
    // SAFETY: ghostty_version returns a static NUL-terminated string.
    let version = unsafe { std::ffi::CStr::from_ptr(sys::ghostty_version()) };
    version.to_str().unwrap_or("")
}
