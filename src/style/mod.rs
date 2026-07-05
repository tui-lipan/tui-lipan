//! Styling and layout primitives for the TUI framework.
//!
//! This module contains types for working with colors, styles, rectangles,
//! padding, and rich text.

/// ANSI escape sequence parser (SGR only).
pub mod ansi;
/// Color definitions.
pub mod color;
mod document_baking;
/// Visual post-processing effect model types.
pub mod effects;
/// Geometric primitives (Rect, Padding, Edge).
pub mod geometry;
/// Layout primitives (Constraints, Length, Size, Align).
pub mod layout;
/// Color palette.
pub mod palette;
/// Built-in theme presets.
pub mod presets;
/// Theme loading and hot-reload support.
#[cfg(feature = "theme-reload")]
pub mod reload;
/// Runtime themed style-slot resolution.
pub mod resolve;
/// Host terminal color palette probing.
pub mod terminal_colors;
/// Rich text primitives (Span, RichText).
pub mod text;
/// Theme primitives (Style, Palette, Borders).
pub mod theme;

pub use ansi::{
    parse_ansi, parse_ansi_into, write_bg_sgr, write_cell_style_sgr, write_fg_sgr,
    write_text_modifiers_sgr, write_underline_color_sgr,
};
pub use color::*;
pub(crate) use document_baking::apply_document_theme_carve_out;
pub use effects::*;
pub use geometry::*;
pub use layout::*;
#[cfg(feature = "theme-reload")]
pub use reload::{ThemeWatcher, load_theme_from_toml};
pub use resolve::{resolve_selection_slot, resolve_slot};
pub use terminal_colors::*;
pub use text::*;
pub use theme::*;
