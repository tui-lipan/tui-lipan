use std::time::Duration;

use crate::animation::{Easing, TransitionConfig};
use crate::core::element::Key;
use crate::style::Rect;

use super::action::ScrollMetrics;

/// Distance-based timing for smooth programmatic scroll targets.
///
/// The resolved target distance is measured in terminal rows. The transition
/// duration is computed as:
///
/// `min_duration + duration_per_row * distance_rows`, clamped to
/// `min_duration..=max_duration`.
#[derive(Clone, Copy, Debug)]
pub struct ScrollDistanceConfig {
    /// Minimum duration for non-zero target jumps.
    pub min_duration: Duration,
    /// Maximum duration cap for long target jumps.
    pub max_duration: Duration,
    /// Additional duration added per row of target distance.
    pub duration_per_row: Duration,
    /// Easing curve used by the generated transition.
    pub easing: Easing,
}

impl ScrollDistanceConfig {
    /// Create distance-based smooth-scroll timing.
    pub const fn new(
        min_duration: Duration,
        max_duration: Duration,
        duration_per_row: Duration,
        easing: Easing,
    ) -> Self {
        Self {
            min_duration,
            max_duration,
            duration_per_row,
            easing,
        }
    }

    /// Compute the transition duration for a row distance.
    pub fn duration_for_distance(self, distance_rows: usize) -> Duration {
        if distance_rows == 0 {
            return Duration::ZERO;
        }

        let min_duration = self.min_duration.min(self.max_duration);
        let max_duration = self.min_duration.max(self.max_duration);
        let rows = distance_rows.min(u32::MAX as usize) as u32;
        min_duration
            .saturating_add(self.duration_per_row.saturating_mul(rows))
            .min(max_duration)
    }

    /// Build the concrete transition config for a row distance.
    pub fn transition_config_for_distance(self, distance_rows: usize) -> TransitionConfig {
        TransitionConfig {
            duration: self.duration_for_distance(distance_rows),
            easing: self.easing,
        }
    }

    /// Minimum transition config used when no distance is available.
    pub const fn min_transition_config(self) -> TransitionConfig {
        TransitionConfig {
            duration: self.min_duration,
            easing: self.easing,
        }
    }
}

impl Default for ScrollDistanceConfig {
    fn default() -> Self {
        Self {
            min_duration: Duration::from_millis(120),
            max_duration: Duration::from_millis(700),
            duration_per_row: Duration::from_millis(8),
            easing: Easing::EaseOutQuad,
        }
    }
}

/// How programmatic scroll targets are applied.
///
/// `Instant` preserves the historical behavior: target APIs snap directly to
/// their resolved row. Smooth variants animate framework-owned target navigation
/// while leaving controlled offsets and user input immediate.
#[derive(Clone, Copy, Debug, Default)]
pub enum ScrollBehavior {
    /// Snap directly to the target row.
    #[default]
    Instant,
    /// Animate to the target row with the provided transition timing.
    Smooth(TransitionConfig),
    /// Animate to the target row with duration derived from row distance.
    SmoothDistance(ScrollDistanceConfig),
}

/// Physics parameters for opt-in smooth wheel scrolling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollWheelConfig {
    /// Velocity impulse added per wheel line, in content rows per second.
    pub acceleration: f32,
    /// Exponential velocity decay per second. Higher values stop sooner.
    pub deceleration: f32,
    /// Absolute velocity clamp, in content rows per second.
    pub max_velocity: f32,
    /// Velocity below this threshold stops the inertial animation.
    pub stop_velocity: f32,
}

impl ScrollWheelConfig {
    /// Create smooth wheel-scroll physics parameters.
    pub const fn new(
        acceleration: f32,
        deceleration: f32,
        max_velocity: f32,
        stop_velocity: f32,
    ) -> Self {
        Self {
            acceleration,
            deceleration,
            max_velocity,
            stop_velocity,
        }
    }
}

impl Default for ScrollWheelConfig {
    fn default() -> Self {
        Self {
            acceleration: 40.0,
            deceleration: 12.0,
            max_velocity: 320.0,
            stop_velocity: 0.05,
        }
    }
}

/// How user wheel input is applied.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ScrollWheelBehavior {
    /// Apply wheel deltas immediately as discrete line jumps.
    #[default]
    Immediate,
    /// Add wheel deltas to an inertial velocity and decay smoothly over time.
    Smooth(ScrollWheelConfig),
}

impl ScrollWheelBehavior {
    /// Apply wheel deltas immediately as discrete line jumps.
    pub const fn immediate() -> Self {
        Self::Immediate
    }

    /// Add wheel deltas to an inertial velocity using `config`.
    pub const fn smooth(config: ScrollWheelConfig) -> Self {
        Self::Smooth(config)
    }

    /// Add wheel deltas to an inertial velocity using default physics.
    pub fn smooth_default() -> Self {
        Self::Smooth(ScrollWheelConfig::default())
    }
}

/// Semantic target for framework-owned `ScrollView` navigation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScrollTarget {
    /// Scroll to the start of the content.
    Top,
    /// Scroll to the end of the content.
    Bottom,
    /// Scroll so the first child subtree containing this key is brought into view.
    Key(Key),
    /// Scroll so the first child subtree containing this key is brought into view,
    /// then add `offset` rows from that child's top.
    KeyOffset {
        /// Key to search for in the first matching child subtree.
        key: Key,
        /// Row offset added to the matched child's top position.
        offset: usize,
    },
}

impl ScrollTarget {
    /// Scroll to the start of the content.
    pub const fn top() -> Self {
        Self::Top
    }

    /// Scroll to the end of the content.
    pub const fn bottom() -> Self {
        Self::Bottom
    }

    /// Scroll so the first child subtree containing `key` is brought into view.
    pub fn key(key: impl Into<Key>) -> Self {
        Self::Key(key.into())
    }

    /// Scroll to `offset` rows below the first child subtree containing `key`.
    pub fn key_offset(key: impl Into<Key>, offset: usize) -> Self {
        Self::KeyOffset {
            key: key.into(),
            offset,
        }
    }
}

impl ScrollBehavior {
    /// Snap directly to the target row.
    pub const fn instant() -> Self {
        Self::Instant
    }

    /// Animate target navigation with `config`.
    pub const fn smooth(config: TransitionConfig) -> Self {
        Self::Smooth(config)
    }

    /// Animate target navigation with [`TransitionConfig::default`].
    pub fn smooth_default() -> Self {
        Self::Smooth(TransitionConfig::default())
    }

    /// Animate target navigation with distance-based default timing.
    pub fn smooth_adaptive() -> Self {
        Self::SmoothDistance(ScrollDistanceConfig::default())
    }

    /// Animate target navigation with distance-based timing.
    pub const fn smooth_distance(config: ScrollDistanceConfig) -> Self {
        Self::SmoothDistance(config)
    }

    /// Return a representative transition config when this behavior is smooth.
    ///
    /// For distance-based behavior this returns the minimum-duration config.
    /// Use [`Self::transition_config_for_distance`] when the target distance is
    /// known.
    pub const fn transition_config(self) -> Option<TransitionConfig> {
        match self {
            Self::Instant => None,
            Self::Smooth(config) => Some(config),
            Self::SmoothDistance(config) => Some(config.min_transition_config()),
        }
    }

    /// Return the concrete transition config for a target distance.
    pub fn transition_config_for_distance(self, distance_rows: usize) -> Option<TransitionConfig> {
        match self {
            Self::Instant => None,
            Self::Smooth(config) => Some(config),
            Self::SmoothDistance(config) => {
                Some(config.transition_config_for_distance(distance_rows))
            }
        }
    }
}

/// A scroll event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScrollEvent {
    /// New scroll offset (row index).
    pub offset: usize,
    /// Scroll metrics from the current viewport.
    pub metrics: ScrollMetrics,
}

/// Visibility classification for a child in a [`ScrollView`](crate::widgets::ScrollView) viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollChildVisibility {
    /// The whole immediate child is visible in the effective viewport.
    FullyVisible,
    /// Only part of the immediate child is visible in the effective viewport.
    PartiallyVisible,
}

/// Direction in which a previously visible child exited the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollChildExitDirection {
    /// The child is now fully above the viewport.
    Above,
    /// The child is now fully below the viewport.
    Below,
    /// The child identity no longer exists or no longer has measurable geometry.
    Removed,
}

/// Snapshot of one immediate [`ScrollView`](crate::widgets::ScrollView) child visible in the effective viewport.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScrollVisibleChild {
    /// Immediate child index in the scroll view.
    pub index: usize,
    /// Immediate child key, when present.
    pub key: Option<Key>,
    /// Child rect relative to scroll content before offset.
    pub content_rect: Rect,
    /// Child rect relative to the effective child viewport after offset and indicators.
    pub viewport_rect: Rect,
    /// Clipped child portion in the effective child viewport.
    pub visible_rect: Rect,
    /// Number of visible rows for this child.
    pub visible_height: u16,
    /// Rows clipped above the effective viewport.
    pub clipped_above: u16,
    /// Rows clipped below the effective viewport.
    pub clipped_below: u16,
    /// Whether the child is fully or partially visible.
    pub visibility: ScrollChildVisibility,
}

/// Previously visible child that exited the viewport.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScrollExitedChild {
    /// Last visible snapshot for the child.
    pub child: ScrollVisibleChild,
    /// Exit direction relative to the current viewport.
    pub direction: ScrollChildExitDirection,
}

/// Viewport-change event for visible immediate [`ScrollView`](crate::widgets::ScrollView) children.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ScrollViewportEvent {
    /// Current row offset.
    pub offset: usize,
    /// Scroll metrics from the current viewport.
    pub metrics: ScrollMetrics,
    /// Effective viewport width available to children.
    pub viewport_width: u16,
    /// Total number of immediate children.
    pub children_len: usize,
    /// First visible immediate child index, if any.
    pub first_visible_index: Option<usize>,
    /// Last visible immediate child index, if any.
    pub last_visible_index: Option<usize>,
    /// Visible immediate children after clipping.
    pub visible: Vec<ScrollVisibleChild>,
    /// Children whose identity became visible since the last emitted snapshot.
    /// Children that remain visible but change partial/full clipping stay in [`Self::visible`].
    pub entered: Vec<ScrollVisibleChild>,
    /// Children whose identity exited since the last emitted snapshot.
    /// Children that remain visible but change partial/full clipping stay in [`Self::visible`].
    pub exited: Vec<ScrollExitedChild>,
    /// Whether a top scroll indicator consumes a row.
    pub top_indicator: bool,
    /// Whether a bottom scroll indicator consumes a row.
    pub bottom_indicator: bool,
    /// Count displayed by the bottom indicator.
    pub bottom_count: usize,
}
