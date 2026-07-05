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
pub(crate) mod math;
pub mod nucleo;
pub mod open_url;
pub(crate) mod prepared_text;
pub(crate) mod scrollbar;
pub(crate) mod selection;
pub(crate) mod text;

pub use file_icons::{FileIconOverride, file_icon, file_icon_span};
pub use open_url::{OpenUrlError, open_url};
pub use selection::{GridPos, GridSelection, GridSelectionEvent};
