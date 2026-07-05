//! Ratatui backend drawing utilities (split modules).

mod cells;
mod colors;
mod convert;
mod placeholder;
mod scrollbars;
mod style_resolve;
mod text;
mod visual_effects;

pub(crate) use cells::*;
pub(crate) use colors::*;
pub(crate) use convert::*;
pub(crate) use placeholder::*;
pub(crate) use scrollbars::*;
pub(crate) use style_resolve::*;
pub(crate) use text::*;
pub(crate) use visual_effects::*;

#[cfg(test)]
mod tests;
