mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_animated;
pub use self::node::AnimatedNode;
pub(crate) use self::reconcile::reconcile_animated;

use std::hash::Hash;

use crate::animation::{ExitAnimation, TransitionConfig};
use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::layout::hash::LayoutHash;
use crate::style::{Color, LayoutConstraints, Length};
use crate::widgets::Spacer;

/// Animate child opacity, revealed height, colors, and optional visual position changes.
#[derive(Clone)]
pub struct Animated {
    pub(crate) child: Box<Element>,
    pub(crate) opacity: f32,
    pub(crate) opacity_fg_only: bool,
    pub(crate) opacity_target: Option<Color>,
    pub(crate) fg: Option<Color>,
    pub(crate) bg: Option<Color>,
    pub(crate) transition: TransitionConfig,
    pub(crate) height: Option<Length>,
    pub(crate) layout_height: Option<Length>,
    pub(crate) position_transition: bool,
    pub(crate) auto_exit: Option<ExitAnimation>,
    pub(crate) on_opacity_transition_end: Option<Callback<()>>,
    pub(crate) on_height_transition_end: Option<Callback<()>>,
    pub(crate) on_position_transition_end: Option<Callback<()>>,
}

impl Default for Animated {
    fn default() -> Self {
        Self {
            child: Box::new(Spacer::new().into()),
            opacity: 1.0,
            opacity_fg_only: false,
            opacity_target: None,
            auto_exit: None,
            fg: None,
            bg: None,
            transition: TransitionConfig::default(),
            height: None,
            layout_height: None,
            position_transition: false,
            on_opacity_transition_end: None,
            on_height_transition_end: None,
            on_position_transition_end: None,
        }
    }
}

impl Animated {
    /// Create an animated wrapper around `child`.
    pub fn new(child: impl Into<Element>) -> Self {
        Self {
            child: Box::new(child.into()),
            ..Self::default()
        }
    }

    /// Set wrapped child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Box::new(child.into());
        self
    }

    /// Set target opacity (`0.0` transparent, `1.0` fully visible).
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// When true, [`Animated::opacity`] only scales foreground alpha; cell backgrounds are unchanged.
    pub fn opacity_fg_only(mut self, fg_only: bool) -> Self {
        self.opacity_fg_only = fg_only;
        self
    }

    /// When set, the opacity post-pass blends toward this color instead of the terminal backdrop.
    ///
    /// Only [`Animated::opacity`] is animated; changing this target mid-transition snaps immediately.
    /// Composes with [`Animated::fg`] / [`Animated::bg`] (they set the base colors that the wash runs on)
    /// and with [`Animated::opacity_fg_only`] (restricts the wash to foreground cells).
    pub fn opacity_target(mut self, color: Color) -> Self {
        self.opacity_target = Some(color);
        self
    }

    /// Set target animated foreground color.
    pub fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Set target animated background color.
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Configure transition timing for this wrapper.
    pub fn transition(mut self, transition: TransitionConfig) -> Self {
        self.transition = transition;
        self
    }

    /// Configure transition duration in milliseconds.
    pub fn duration(mut self, ms: u64) -> Self {
        self.transition.duration = std::time::Duration::from_millis(ms);
        self
    }

    /// Configure transition easing.
    pub fn easing(mut self, easing: crate::animation::Easing) -> Self {
        self.transition.easing = easing;
        self
    }

    /// Set optional animated height target.
    ///
    /// - `None`: wrapper height follows parent allocation.
    /// - `Some(Length::Auto)`: uses measured child natural height.
    /// - `Some(Length::Px(_))`: uses explicit pixel target.
    pub fn height(mut self, height: Length) -> Self {
        self.height = Some(height);
        self
    }

    /// Override the height used for stack measurement and gap math while [`Animated::height`] still
    /// drives the animated target.
    ///
    /// Use while collapsing so parents keep reserving natural height until
    /// [`Animated::on_height_transition_end`] fires, then clear (`None`) so layout matches the final
    /// target.
    pub fn layout_height(mut self, height: Option<Length>) -> Self {
        self.layout_height = height;
        self
    }

    /// Enable or disable visual position transitions for this wrapper.
    ///
    /// When enabled on an existing keyed `Animated` node, layout rect changes animate visually from
    /// the previous origin to the new final origin while hit-testing and layout use the final rect
    /// immediately. Initial mount does not animate.
    pub fn position_transition(mut self, enabled: bool) -> Self {
        self.position_transition = enabled;
        self
    }

    /// Called once when a height transition reaches its target (including zero-duration jumps).
    pub fn on_height_transition_end(mut self, cb: Callback<()>) -> Self {
        self.on_height_transition_end = Some(cb);
        self
    }

    /// Called once when an opacity transition reaches its target (including zero-duration jumps).
    pub fn on_opacity_transition_end(mut self, cb: Callback<()>) -> Self {
        self.on_opacity_transition_end = Some(cb);
        self
    }

    /// Called once when a position transition reaches its final layout origin.
    ///
    /// This also fires for zero-duration position transitions that snap immediately.
    pub fn on_position_transition_end(mut self, cb: Callback<()>) -> Self {
        self.on_position_transition_end = Some(cb);
        self
    }

    /// Fade and collapse helper for mount/unmount transitions.
    ///
    /// Sets opacity, animated height, and duration in one call to drive the
    /// standard "appear / disappear" animation. Pair with
    /// [`Animated::on_exit_complete`] to be notified when the disappearance
    /// finishes so the parent can actually drop the element from state.
    ///
    /// - `visible == true`: opacity `1.0`, height `Length::Auto`.
    /// - `visible == false`: opacity `0.0`, height `Length::Px(0)`.
    ///
    /// Both directions use `duration_ms` and the wrapper's currently configured
    /// easing (defaults to `EaseOutQuad`; override with [`Animated::easing`]).
    ///
    /// ```ignore
    /// // state.visible: bool, state.removed: bool
    /// if !state.removed {
    ///     Animated::new(child)
    ///         .exit(state.visible, 200)
    ///         .on_exit_complete(ctx.link().callback(|_| Msg::Removed))
    /// }
    /// ```
    pub fn exit(mut self, visible: bool, duration_ms: u64) -> Self {
        self.opacity = if visible { 1.0 } else { 0.0 };
        self.height = Some(if visible { Length::Auto } else { Length::Px(0) });
        self.transition.duration = std::time::Duration::from_millis(duration_ms);
        self
    }

    /// Play an exit animation automatically when this element is removed.
    ///
    /// [`Animated::exit`] requires the parent to keep the element in its own state until
    /// [`Animated::on_exit_complete`] fires, because the reconciler frees any node that is not
    /// re-described during `view()`. `auto_exit` moves that bookkeeping into the framework: the
    /// element can simply stop being described, and its container retains the already-rendered
    /// subtree, animates it out, and drops it.
    ///
    /// Takes anything that converts into an [`ExitAnimation`]. A bare duration is the common case
    /// and means "fade out over this many milliseconds":
    ///
    /// ```ignore
    /// // No `removed` flag, no on_exit_complete plumbing: dropping it from the list is enough.
    /// VStack::new().children(state.rows.iter().map(|row| {
    ///     Animated::new(row_view(row)).auto_exit(200).key(row.id)
    /// }))
    ///
    /// // Or say what leaving should look like.
    /// Animated::new(toast)
    ///     .auto_exit(ExitAnimation::slide(180, 0, -1).with_collapse(true))
    ///     .key(id)
    /// ```
    ///
    /// # Requirements
    ///
    /// The element must carry a [`Key`](crate::Key) and sit directly in a `VStack`, `HStack`,
    /// `ZStack`, or `Canvas`. Keys are how the container recognizes that a specific child left
    /// rather than that the list merely reordered. Debug builds log when either is missing.
    ///
    /// # What the container decides
    ///
    /// Everything visual comes from the [`ExitAnimation`]. The one thing it does not control is
    /// whether height collapses, because that is a layout question the parent owns:
    ///
    /// - A **`VStack` or `HStack`** always collapses, whatever the exit says. The collapse is what
    ///   lets siblings reflow into the vacated space, so it is part of the container's contract.
    /// - A **`Canvas` or `ZStack`** collapses only if the exit asked for it with
    ///   [`ExitAnimation::with_collapse`]. Nothing reflows around a positioned child, so there is
    ///   no space to reclaim and the collapse is a pure effect.
    ///
    /// A `Canvas` additionally draws exiting children *beneath* every live one, so a departing
    /// element can never cover something the application is still describing.
    ///
    /// # Lifecycle and disposal
    ///
    /// A retained subtree is a **snapshot**, not a living element. The container keeps the node it
    /// already reconciled; the element itself stopped being described, so on that same frame its
    /// component state, hooks, command registrations, and scroll state were all disposed by the
    /// ordinary sweep. Only the resolved node data survives, which is exactly enough to keep
    /// painting it.
    ///
    /// The framework enforces what follows from that, so an exit cannot reach into a dropped
    /// scope:
    ///
    /// - The subtree is **inert**: skipped for hit-testing, focus, and key routing. It cannot be
    ///   clicked, cannot take focus, and receives no keys.
    /// - Transition-end callbacks (`on_opacity_transition_end` and friends) do **not** fire during
    ///   an automatic exit.
    /// - Nothing re-runs `view()`, so no effect, command, or state read happens on its behalf.
    ///
    /// Retention also ends on a deadline derived from the exit duration, so a container that stops
    /// being rendered mid-exit cannot hold the subtree indefinitely. Re-adding the same key before
    /// the exit finishes cancels it and hands the live element back.
    ///
    /// Use [`Animated::exit`] with [`ExitQueue`](crate::animation::ExitQueue) instead when the app
    /// needs to own the lifecycle, or when the exit has to change where the element's *children*
    /// sit: a retained subtree is never re-laid out, so scaling and reflowing are out of reach.
    /// See [`ExitAnimation`] for that boundary in full.
    pub fn auto_exit(mut self, exit: impl Into<ExitAnimation>) -> Self {
        // Deliberately touches neither `height` nor `transition`. Opting into an exit must not
        // change how the element looks or lays out while it is alive; the exit carries its own
        // duration and easing, and the collapse reads the node's real rectangle rather than a
        // resolved `Length`.
        self.auto_exit = Some(exit.into());
        self
    }

    /// Callback fired once when an [`Animated::exit`]-style collapse finishes,
    /// i.e. when the height transition reaches its final target.
    ///
    /// This is an alias for [`Animated::on_height_transition_end`] —
    /// `exit(false, ..)` settles height last, so this fires when the element
    /// has fully collapsed and is safe to remove from state.
    pub fn on_exit_complete(self, cb: Callback<()>) -> Self {
        self.on_height_transition_end(cb)
    }
}

impl From<Animated> for Element {
    fn from(value: Animated) -> Self {
        let (min_w, min_h) = measure_animated(&value, None, None);
        let mut layout = LayoutConstraints::default().min_width(Length::Px(min_w));
        if value.height.is_none() {
            layout = layout.min_height(Length::Px(min_h));
        }
        Element::new(ElementKind::Animated(value)).with_layout(layout)
    }
}

impl LayoutHash for Animated {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        self.opacity.to_bits().hash(hasher);
        self.opacity_fg_only.hash(hasher);
        self.opacity_target.hash(hasher);
        self.transition.duration.hash(hasher);
        self.transition.easing.hash(hasher);
        self.height.hash(hasher);
        self.layout_height.hash(hasher);
        self.position_transition.hash(hasher);
        recurse(self.child.as_ref())?.hash(hasher);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Spacer;

    #[test]
    fn exit_visible_sets_full_opacity_and_auto_height() {
        let a = Animated::new(Spacer::new()).exit(true, 200);
        assert_eq!(a.opacity, 1.0);
        assert_eq!(a.height, Some(Length::Auto));
        assert_eq!(a.transition.duration.as_millis(), 200);
    }

    #[test]
    fn exit_hidden_sets_zero_opacity_and_zero_height() {
        let a = Animated::new(Spacer::new()).exit(false, 150);
        assert_eq!(a.opacity, 0.0);
        assert_eq!(a.height, Some(Length::Px(0)));
        assert_eq!(a.transition.duration.as_millis(), 150);
    }

    #[test]
    fn on_exit_complete_aliases_height_transition_end() {
        let cb = Callback::new(|_: ()| {});
        let a = Animated::new(Spacer::new()).on_exit_complete(cb);
        assert!(a.on_height_transition_end.is_some());
    }
}
