//! Utility functions.

pub(crate) mod arena;
/// Braille glyph and sub-cell drawing helpers.
pub mod braille;
/// Color readability and contrast helpers.
pub mod color_contrast;
pub(crate) mod diff;
pub(crate) mod file_icons;
pub(crate) mod gen_cache;
pub mod gradient;
/// Generic terminal hint discovery helpers.
pub mod hints;
pub(crate) mod math;
pub mod nucleo;
pub mod open_url;
pub(crate) mod prepared_text;
/// Sanitization helpers for untrusted display text.
pub mod sanitize;
pub(crate) mod scrollbar;
pub(crate) mod selection;
/// Display-column operations for styled spans.
pub mod spans;
pub(crate) mod text;

pub use file_icons::{
    FileIconOverride, directory_icon, directory_icon_span, file_icon, file_icon_span,
};
pub use open_url::{OpenUrlError, open_url};
pub use sanitize::{sanitize_display_span, sanitize_display_spans, sanitize_display_text};
pub use selection::{GridPos, GridSelection, GridSelectionEvent, SelectionEnd};
