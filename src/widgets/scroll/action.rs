use crate::core::event::{KeyCode, KeyEvent, MouseKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ScrollAction {
    LineUp(usize),
    LineDown(usize),
    LineLeft(usize),
    LineRight(usize),
    Home,
    End,
}

/// Scroll viewport metrics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollMetrics {
    /// Total number of rows (scrollable content height).
    pub len: usize,
    /// Number of visible rows.
    pub visible: usize,
    /// Maximum scroll offset (row index).
    pub max_offset: usize,
}

/// One-shot scroll request applied relative to the current viewport.
///
/// Use this for command-driven navigation (page up/down, line up/down, top,
/// bottom) without permanently controlling the widget's settled offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollRequest {
    /// Scroll by a signed line delta. Negative values move up; positive values move down.
    Delta(isize),
    /// Scroll by a signed fraction of the current visible viewport height.
    ///
    /// For example, `ViewportDelta { numerator: 1, denominator: 2 }` moves down
    /// by roughly half a page. Negative numerators move up.
    ViewportDelta {
        /// Signed fraction numerator. Positive values move down; negative values move up.
        numerator: isize,
        /// Fraction denominator. `2` means half a page, `4` means quarter-page, etc.
        denominator: usize,
    },
    /// Jump to the start of the content.
    Top,
    /// Jump to the end of the content.
    Bottom,
}

impl ScrollRequest {
    /// Scroll by a signed line delta.
    pub const fn lines(delta: isize) -> Self {
        Self::Delta(delta)
    }

    /// Scroll by a signed fraction of the visible viewport height.
    pub const fn viewport_fraction(numerator: isize, denominator: usize) -> Self {
        Self::ViewportDelta {
            numerator,
            denominator,
        }
    }

    /// Scroll up by one full visible page.
    pub const fn page_up() -> Self {
        Self::viewport_fraction(-1, 1)
    }

    /// Scroll down by one full visible page.
    pub const fn page_down() -> Self {
        Self::viewport_fraction(1, 1)
    }

    /// Scroll up by half a visible page.
    pub const fn half_page_up() -> Self {
        Self::viewport_fraction(-1, 2)
    }

    /// Scroll down by half a visible page.
    pub const fn half_page_down() -> Self {
        Self::viewport_fraction(1, 2)
    }

    /// Jump to the start of the content.
    pub const fn top() -> Self {
        Self::Top
    }

    /// Jump to the end of the content.
    pub const fn bottom() -> Self {
        Self::Bottom
    }

    /// Returns whether this request could move a viewport currently at `offset`
    /// with the given `max_offset`.
    pub const fn has_effect(self, offset: usize, max_offset: usize) -> bool {
        match self {
            Self::Delta(delta) => (delta < 0 && offset > 0) || (delta > 0 && offset < max_offset),
            Self::ViewportDelta { numerator, .. } => {
                (numerator < 0 && offset > 0) || (numerator > 0 && offset < max_offset)
            }
            Self::Top => offset > 0,
            Self::Bottom => offset < max_offset,
        }
    }
}

/// Key bindings for scrollable widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollKeymap(u8);

impl ScrollKeymap {
    /// Disable key handling.
    pub const NONE: Self = Self(0);
    /// Arrow keys.
    pub const ARROWS: Self = Self(1 << 0);
    /// Vim-style j/k (vertical navigation).
    pub const VIM_VERTICAL: Self = Self(1 << 1);
    /// Vim-style h/l (horizontal navigation).
    pub const VIM_HORIZONTAL: Self = Self(1 << 2);
    /// Home and End keys.
    pub const HOME_END: Self = Self(1 << 3);
    /// Vim-style h/j/k/l (both vertical and horizontal navigation).
    /// Note: This is kept for backward compatibility.
    pub const VIM: Self = Self(Self::VIM_VERTICAL.0 | Self::VIM_HORIZONTAL.0);
    /// Default key set (arrows, j/k, h/l, home/end).
    pub const DEFAULT: Self = Self(Self::ARROWS.0 | Self::VIM.0 | Self::HOME_END.0);

    /// Check if this keymap includes another set.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns a new keymap with vertical vim keys enabled.
    pub fn with_vim_vertical(self) -> Self {
        Self(self.0 | Self::VIM_VERTICAL.0)
    }

    /// Returns a new keymap with horizontal vim keys enabled.
    pub fn with_vim_horizontal(self) -> Self {
        Self(self.0 | Self::VIM_HORIZONTAL.0)
    }

    /// Returns a new keymap with vertical vim keys disabled.
    pub fn without_vim_vertical(self) -> Self {
        Self(self.0 & !Self::VIM_VERTICAL.0)
    }

    /// Returns a new keymap with horizontal vim keys disabled.
    pub fn without_vim_horizontal(self) -> Self {
        Self(self.0 & !Self::VIM_HORIZONTAL.0)
    }
}

impl std::ops::BitOr for ScrollKeymap {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for ScrollKeymap {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for ScrollKeymap {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitAndAssign for ScrollKeymap {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for ScrollKeymap {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl Default for ScrollKeymap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// ScrollView child clipping behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ScrollClip {
    /// Allow partially visible children.
    #[default]
    Partial,
}

pub(crate) fn apply_scroll_request(
    offset: usize,
    metrics: ScrollMetrics,
    request: ScrollRequest,
) -> usize {
    if metrics.visible == 0 || metrics.len == 0 {
        return 0;
    }

    match request {
        ScrollRequest::Delta(delta) => {
            if delta < 0 {
                apply_scroll_action(offset, metrics, ScrollAction::LineUp(delta.unsigned_abs()))
            } else if delta > 0 {
                apply_scroll_action(offset, metrics, ScrollAction::LineDown(delta as usize))
            } else {
                offset.min(metrics.max_offset)
            }
        }
        ScrollRequest::ViewportDelta {
            numerator,
            denominator,
        } => {
            let denominator = denominator.max(1);
            let magnitude = ((metrics.visible.saturating_mul(numerator.unsigned_abs()))
                + denominator.saturating_sub(1))
                / denominator;
            let lines = magnitude.max(1);
            if numerator < 0 {
                apply_scroll_action(offset, metrics, ScrollAction::LineUp(lines))
            } else if numerator > 0 {
                apply_scroll_action(offset, metrics, ScrollAction::LineDown(lines))
            } else {
                offset.min(metrics.max_offset)
            }
        }
        ScrollRequest::Top => 0,
        ScrollRequest::Bottom => metrics.max_offset,
    }
}

pub(crate) fn scroll_metrics(len: usize, visible: usize, _offset: usize) -> ScrollMetrics {
    let visible = visible.min(len);
    let max_offset = if visible == 0 {
        0
    } else {
        len.saturating_sub(visible)
    };
    ScrollMetrics {
        len,
        visible,
        max_offset,
    }
}

pub(crate) fn scroll_action_from_key(key: &KeyEvent, keymap: ScrollKeymap) -> Option<ScrollAction> {
    match key.code {
        KeyCode::Up if keymap.contains(ScrollKeymap::ARROWS) => Some(ScrollAction::LineUp(1)),
        KeyCode::Down if keymap.contains(ScrollKeymap::ARROWS) => Some(ScrollAction::LineDown(1)),
        KeyCode::Left if keymap.contains(ScrollKeymap::ARROWS) => Some(ScrollAction::LineLeft(1)),
        KeyCode::Right if keymap.contains(ScrollKeymap::ARROWS) => Some(ScrollAction::LineRight(1)),
        KeyCode::Char('k') if keymap.contains(ScrollKeymap::VIM_VERTICAL) => {
            Some(ScrollAction::LineUp(1))
        }
        KeyCode::Char('j') if keymap.contains(ScrollKeymap::VIM_VERTICAL) => {
            Some(ScrollAction::LineDown(1))
        }
        KeyCode::Char('h') if keymap.contains(ScrollKeymap::VIM_HORIZONTAL) => {
            Some(ScrollAction::LineLeft(1))
        }
        KeyCode::Char('l') if keymap.contains(ScrollKeymap::VIM_HORIZONTAL) => {
            Some(ScrollAction::LineRight(1))
        }
        KeyCode::Home if keymap.contains(ScrollKeymap::HOME_END) => Some(ScrollAction::Home),
        KeyCode::End if keymap.contains(ScrollKeymap::HOME_END) => Some(ScrollAction::End),
        _ => None,
    }
}

/// Convert a mouse event into a scroll action with a configurable line count.
///
/// Used for coalescing multiple scroll-wheel events into a single action.
pub(crate) fn scroll_action_from_mouse_n(
    event: crate::core::event::MouseEvent,
    lines: usize,
) -> Option<ScrollAction> {
    if event.mods.shift {
        match event.kind {
            MouseKind::ScrollUp => Some(ScrollAction::LineLeft(lines)),
            MouseKind::ScrollDown => Some(ScrollAction::LineRight(lines)),
            _ => None,
        }
    } else {
        match event.kind {
            MouseKind::ScrollUp => Some(ScrollAction::LineUp(lines)),
            MouseKind::ScrollDown => Some(ScrollAction::LineDown(lines)),
            _ => None,
        }
    }
}

pub(crate) fn apply_scroll_action(
    offset: usize,
    metrics: ScrollMetrics,
    action: ScrollAction,
) -> usize {
    if metrics.visible == 0 || metrics.len == 0 {
        return 0;
    }

    let max_offset = metrics.max_offset;

    match action {
        ScrollAction::LineUp(lines) | ScrollAction::LineLeft(lines) => offset.saturating_sub(lines),
        ScrollAction::LineDown(lines) | ScrollAction::LineRight(lines) => {
            offset.saturating_add(lines).min(max_offset)
        }
        ScrollAction::Home => 0,
        ScrollAction::End => max_offset,
    }
}
