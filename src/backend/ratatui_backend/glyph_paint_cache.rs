//! Per-draw memoization: progress/spinner glyph work and WCAG/APCA readable foreground
//! (`readable_text_color*` family). Cleared at the start of each [`crate::backend::ratatui_backend::render::render`]
//! pass and once per `AppRunner::draw_current_tree` tick.
//!
//! # Active memo TLS
//! [`ActiveMemoGuard`] registers the current frame's cache for the duration of [`render`]. Code that runs
//! outside that scope (e.g. [`crate::backend::ratatui_backend::render::render_regions`] incremental scroll)
//! sees no active memo and falls back to uncached contrast math.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHasher};

use crate::style::{Color, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::{ProgressStyle, ProgressZone, SpinnerStyle};

/// Hash of progressive zones for cache keys (`0` when `zones.is_empty()`).
pub(crate) fn fingerprint_progress_zones(zones: &[ProgressZone]) -> u64 {
    let mut hasher = FxHasher::default();
    zones.len().hash(&mut hasher);
    for z in zones {
        z.upto.to_bits().hash(&mut hasher);
        z.style.hash(&mut hasher);
        z.symbol.hash(&mut hasher);
    }
    hasher.finish()
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub(crate) struct SpinnerSimpleGlyphKey {
    pub spinner_style: SpinnerStyle,
    pub frame_mod: u16,
    pub lipan_style: Style,
}

#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub(crate) struct ProgressTrackCacheKey {
    pub bar_width: u16,
    pub full_filled_cells: u16,
    pub partial_slot: Option<u8>,
    pub inverted: bool,
    pub progress_style: ProgressStyle,
    pub filled_style: Style,
    pub empty_style: Style,
    pub filled_gradient: Option<ColorGradient>,
    pub zones_fingerprint: u64,
    pub block_empty_bg_dim_bits: u32,
    pub is_block_mode: bool,
    /// Surface the empty track blends toward; part of the key so a theme
    /// switch cannot serve a track cached against the previous background.
    pub track_backdrop: Option<Color>,
}

/// Key for [`crate::utils::color_contrast::readable_text_color`] and friends: preferred fg + background.
pub(crate) type ReadableTextPairKey = (Option<Color>, Color);

#[derive(Default)]
pub(crate) struct PaintGlyphCaches {
    pub(crate) spinner_rat_style: FxHashMap<SpinnerSimpleGlyphKey, ratatui::style::Style>,
    pub(crate) progress_track: FxHashMap<ProgressTrackCacheKey, Arc<[(char, Style)]>>,
    pub(crate) readable_wcag_fg: FxHashMap<ReadableTextPairKey, Color>,
    pub(crate) readable_bw_fg: FxHashMap<ReadableTextPairKey, Color>,
    pub(crate) readable_apca_fg: FxHashMap<ReadableTextPairKey, Color>,
}

impl PaintGlyphCaches {
    pub(crate) fn clear(&mut self) {
        self.spinner_rat_style.clear();
        self.progress_track.clear();
        self.readable_wcag_fg.clear();
        self.readable_bw_fg.clear();
        self.readable_apca_fg.clear();
    }
}

thread_local! {
    static ACTIVE_PAINT_MEMO: RefCell<Option<Rc<RefCell<PaintGlyphCaches>>>> =
        const { RefCell::new(None) };
}

pub(crate) struct ActiveMemoGuard {
    previous: Option<Rc<RefCell<PaintGlyphCaches>>>,
}

impl ActiveMemoGuard {
    /// Register `active` for nested `finalize_style` memoization (`None` = disabled).
    pub(crate) fn install(active: Option<Rc<RefCell<PaintGlyphCaches>>>) -> Self {
        let previous = ACTIVE_PAINT_MEMO.with(|cell| {
            let mut slot = cell.borrow_mut();
            std::mem::replace(&mut *slot, active)
        });
        Self { previous }
    }
}

impl Drop for ActiveMemoGuard {
    fn drop(&mut self) {
        let prev = self.previous.take();
        ACTIVE_PAINT_MEMO.with(|cell| {
            *cell.borrow_mut() = prev;
        });
    }
}

pub(crate) fn active_paint_memo() -> Option<Rc<RefCell<PaintGlyphCaches>>> {
    ACTIVE_PAINT_MEMO.with(|cell| cell.borrow().clone())
}
