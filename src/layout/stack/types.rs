//! Stack layout types.

use crate::layout::axis::Axis;
use crate::style::{Rect, ShrinkPriority};
use crate::widgets::containers::FocusAccordion;

#[derive(Clone, Debug)]
pub(crate) struct StackChildLayout {
    pub flex: u16,
    pub collapse_main: Option<u16>,
    pub protected: bool,
    pub size: u16,
    pub compact: bool,
    pub min_size: u16,
    /// Readable floor: the main size below which a reflowing child must start
    /// truncating its content. For non-reflowing children this equals `min_size`.
    pub min_content: u16,
    pub shrinkable: bool,
    pub shrink_priority: ShrinkPriority,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StackMeasuredSize {
    pub w: u16,
    pub h: u16,
}

impl StackMeasuredSize {
    pub fn main_axis(self, axis: Axis) -> u16 {
        match axis {
            Axis::Vertical => self.h,
            Axis::Horizontal => self.w,
        }
    }

    pub fn cross_axis(self, axis: Axis) -> u16 {
        match axis {
            Axis::Vertical => self.w,
            Axis::Horizontal => self.h,
        }
    }
}

pub(crate) struct StackMainLayout {
    pub sizes: Vec<u16>,
    pub gaps: Vec<u16>,
    pub measured_sizes: Vec<Option<StackMeasuredSize>>,
    /// Per-gap flag: `true` when adjacent children share a border (join overlap).
    /// At these positions the positioning loop must subtract 1 so the rects overlap.
    pub join_overlaps: Vec<bool>,
    pub join_count: u16,
}

pub(crate) struct ScrollContentLayout {
    pub rects: Vec<Rect>,
    pub content_height: u16,
    pub content_width: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FocusMode {
    Accordion,
    Squashed,
    Tiny,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FocusPolicyContext {
    pub mode: FocusMode,
    pub policy: FocusAccordion,
}
