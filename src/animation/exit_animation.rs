//! Declarative description of what an [`Animated::auto_exit`](crate::widgets::Animated::auto_exit)
//! element does on its way out.

use std::time::Duration;

use crate::animation::Easing;
use crate::style::Color;

/// What an [`Animated::auto_exit`](crate::widgets::Animated::auto_exit) element animates to as it
/// leaves, and for how long.
///
/// Retention (keeping a removed keyed child alive long enough to animate) is the framework's job.
/// *What* the exit looks like is the application's, and this is how it says so. Every field is a
/// target the already-reconciled node interpolates toward; unset fields are left alone.
///
/// ```ignore
/// // The default: fade out over 200ms.
/// Animated::new(row).auto_exit(200).key(id)
///
/// // A toast leaving a stack: slide up a row, fade, and let the toasts below close the gap.
/// Animated::new(toast)
///     .auto_exit(ExitAnimation::slide(180, 0, -1).with_collapse(true))
///     .key(id)
///
/// // Flash before going.
/// Animated::new(row).auto_exit(ExitAnimation::new(220).bg(theme.status.error)).key(id)
/// ```
///
/// # What can and cannot be expressed
///
/// The retained subtree is **frozen**: the container keeps the node it already reconciled, and
/// nothing is cloned, re-derived, or re-laid out. That is what makes retention nearly free, and it
/// is a hard boundary rather than a missing feature.
///
/// So an exit can change how the already-rendered cells *look*: their alpha ([`opacity`]), their
/// colors ([`fg`], [`bg`]), where they are painted ([`offset`]), and how much of the element is
/// visible ([`with_collapse`]). It can never change where the element's **children** sit. There is
/// no scale, no reflow, and no re-wrap, because the border of a shrinking box is drawn by a child
/// at a rectangle nobody recomputes; it would be clipped away rather than move.
///
/// The rule for choosing:
///
/// - Does the exit only change how already-rendered cells look? Use `auto_exit`.
/// - Does it change where the element's children sit? Keep describing the element yourself, with
///   [`Animated::exit`](crate::widgets::Animated::exit) and
///   [`ExitQueue`](crate::animation::ExitQueue).
///
/// [`opacity`]: Self::opacity
/// [`fg`]: Self::fg
/// [`bg`]: Self::bg
/// [`offset`]: Self::offset
/// [`with_collapse`]: Self::with_collapse
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitAnimation {
    pub(crate) duration: Duration,
    pub(crate) easing: Option<Easing>,
    pub(crate) opacity: Option<f32>,
    pub(crate) fg: Option<Color>,
    pub(crate) bg: Option<Color>,
    pub(crate) offset: Option<(i16, i16)>,
    pub(crate) collapse: bool,
}

impl ExitAnimation {
    /// Fade to fully transparent over `duration_ms`.
    pub const fn new(duration_ms: u64) -> Self {
        Self {
            duration: Duration::from_millis(duration_ms),
            easing: None,
            opacity: Some(0.0),
            fg: None,
            bg: None,
            offset: None,
            collapse: false,
        }
    }

    /// Fade while translating `dx` columns and `dy` rows from where the element sat when it was
    /// removed. Negative values move left and up. See [`offset`](Self::offset).
    pub const fn slide(duration_ms: u64, dx: i16, dy: i16) -> Self {
        let mut exit = Self::new(duration_ms);
        exit.offset = Some((dx, dy));
        exit
    }

    /// Fade while collapsing to zero height.
    ///
    /// In a `VStack` or `HStack` the collapse always happens, because it is what lets siblings
    /// reflow into the vacated space, so this only differs from [`new`](Self::new) under a
    /// `Canvas` or `ZStack`. There it is purely an effect, and one with a caveat: the subtree is
    /// clipped rather than re-laid out, so the element keeps full size while its lower rows,
    /// bottom border included, are cut away. Clean for borderless content, wrong for a bordered
    /// box.
    pub const fn collapse(duration_ms: u64) -> Self {
        let mut exit = Self::new(duration_ms);
        exit.collapse = true;
        exit
    }

    /// Opacity to fade toward. Defaults to `0.0`.
    pub const fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Leave opacity untouched, for an exit carried entirely by movement or color.
    pub const fn keep_opacity(mut self) -> Self {
        self.opacity = None;
        self
    }

    /// Foreground color to animate toward.
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Background color to animate toward.
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Translate by `dx` columns and `dy` rows while leaving.
    ///
    /// The translation is **relative to the position the element occupied when it was removed**,
    /// not to its layout rectangle. If a movement transition was still in flight at that moment
    /// (a reorder, a FLIP move), the exit starts from wherever the element had actually reached
    /// and absorbs the remainder, so a row removed mid-move slides on from where it was rather
    /// than snapping back first.
    ///
    /// Painting only. The element is already inert, so nothing about hit-testing, focus, layout,
    /// or scrolling consults it. Movement is clipped by the parent, which is usually what you
    /// want: the element slides out under the container's edge.
    pub const fn offset(mut self, dx: i16, dy: i16) -> Self {
        self.offset = Some((dx, dy));
        self
    }

    /// Easing for every property in this exit. Defaults to the widget's own
    /// [`transition`](crate::widgets::Animated::transition) easing.
    pub const fn easing(mut self, easing: Easing) -> Self {
        self.easing = Some(easing);
        self
    }

    /// Whether to collapse height as well. See [`collapse`](Self::collapse).
    pub const fn with_collapse(mut self, collapse: bool) -> Self {
        self.collapse = collapse;
        self
    }

    /// How long the exit runs. Retention lasts at least this long, plus a small grace period.
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

impl From<u64> for ExitAnimation {
    /// `auto_exit(200)` is `auto_exit(ExitAnimation::new(200))`: a plain fade.
    fn from(duration_ms: u64) -> Self {
        Self::new(duration_ms)
    }
}

impl From<Duration> for ExitAnimation {
    fn from(duration: Duration) -> Self {
        Self {
            duration,
            ..Self::new(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_duration_shorthand_is_a_fade() {
        assert_eq!(ExitAnimation::from(200), ExitAnimation::new(200));
        assert_eq!(ExitAnimation::new(200).opacity, Some(0.0));
        assert_eq!(
            ExitAnimation::new(200).duration(),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn an_exit_can_drop_the_fade_and_move_instead() {
        let exit = ExitAnimation::slide(180, 0, -1).keep_opacity();
        assert_eq!(exit.opacity, None);
        assert_eq!(exit.offset, Some((0, -1)));
    }

    #[test]
    fn collapse_is_independent_of_every_other_property() {
        let exit = ExitAnimation::slide(120, 2, 0).with_collapse(true);
        assert!(exit.collapse);
        assert_eq!(exit.offset, Some((2, 0)));
        assert_eq!(exit.opacity, Some(0.0));
        assert!(!ExitAnimation::new(120).collapse);
    }
}
