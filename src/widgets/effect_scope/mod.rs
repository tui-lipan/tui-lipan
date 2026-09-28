mod layout;
mod node;
mod reconcile;

use std::sync::Arc;

pub(crate) use self::layout::measure_effect_scope;
pub use self::node::EffectScopeNode;
pub(crate) use self::reconcile::reconcile_effect_scope;

use crate::app::ContrastPolicy;
use crate::core::element::{Element, ElementKind};
use crate::style::{
    CellEffect, Color, ColorTransform, EffectAmount, LayoutConstraints, Length, VisualEffect,
};

/// Apply render-time color effects to an entire child subtree.
///
/// `EffectScope` post-processes the rendered cells inside its child bounds, so
/// explicit colors inside the subtree are still affected. This is useful for
/// dimming inactive panes, tinting overlays, or applying contrast adjustments
/// to a whole section at once.
#[derive(Clone, Default)]
pub struct EffectScope {
    pub(crate) child: Option<Box<Element>>,
    pub(crate) effects: Vec<VisualEffect>,
    pub(crate) cells_only: bool,
}

impl EffectScope {
    /// Create an empty effect scope.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set wrapped child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Dim the rendered subtree by an explicit amount.
    ///
    /// `amount` is an `f32` or a late-bound [`EffectAmount`]: an
    /// [`animated_amount`](crate::Context::animated_amount) transition or a
    /// [`pulsing_amount`](crate::Context::pulsing_amount), both of which animate with repaints
    /// alone.
    pub fn dim_by(self, amount: impl Into<EffectAmount>) -> Self {
        self.effect(VisualEffect::dim(amount))
    }

    /// Lighten the rendered subtree by an explicit amount.
    ///
    /// `amount` may be late-bound, as for [`Self::dim_by`].
    pub fn lighten_by(self, amount: impl Into<EffectAmount>) -> Self {
        self.effect(VisualEffect::lighten(amount))
    }

    /// Tint the rendered subtree toward a color.
    ///
    /// `alpha` may be late-bound, as for [`Self::dim_by`]. A breathing alert tint needs no app-side
    /// timer:
    ///
    /// ```no_run
    /// # use std::time::Duration;
    /// # use tui_lipan::prelude::*;
    /// # fn example(ctx: &Context<impl Component>) -> Element {
    /// let alpha = ctx.pulsing_amount(
    ///     "build-alert",
    ///     EffectPulse::new(0.08, 0.20).period(Duration::from_millis(1400)),
    /// );
    /// EffectScope::new()
    ///     .tint_by(Color::Rgb(220, 60, 60), alpha)
    ///     .child(Text::new("build failed"))
    ///     .into()
    /// # }
    /// ```
    pub fn tint_by(self, color: Color, alpha: impl Into<EffectAmount>) -> Self {
        self.effect(VisualEffect::tint(color, alpha))
    }

    /// Apply a relative transform to the resolved foreground color of the subtree.
    pub fn transform_fg(self, transform: ColorTransform) -> Self {
        self.effect(VisualEffect::transform_fg(transform))
    }

    /// Apply a relative transform to the resolved background color of the subtree.
    pub fn transform_bg(self, transform: ColorTransform) -> Self {
        self.effect(VisualEffect::transform_bg(transform))
    }

    /// Override contrast adjustment for the rendered subtree.
    pub fn contrast_policy(self, policy: ContrastPolicy) -> Self {
        self.effect(VisualEffect::ContrastPolicy(policy))
    }

    /// Append a visual effect to this scope.
    pub fn effect(mut self, effect: VisualEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Append a user-defined per-cell visual effect to this scope.
    pub fn custom_effect(self, effect: impl CellEffect) -> Self {
        self.effect(VisualEffect::Custom(Arc::new(effect)))
    }

    /// Append visual effects from an iterator.
    pub fn effects<I>(mut self, effects: I) -> Self
    where
        I: IntoIterator<Item = VisualEffect>,
    {
        self.effects.extend(effects);
        self
    }

    /// Keep this scope's effects off image pixels.
    ///
    /// By default an effect that recolors cells also recolors the terminal images drawn under the
    /// scope, so a dimmed pane dims its pictures too. With `cells_only`, the effects apply to text
    /// cells alone and images keep their own pixels, whatever the effect and whether its amount is
    /// fixed, fading, or pulsing. Use it for a signal that belongs to the text around a picture
    /// rather than to the picture, such as a pane's alert tint.
    ///
    /// It also saves the image re-encode a fixed effect costs when it appears and again when it
    /// goes.
    ///
    /// ```no_run
    /// # use tui_lipan::prelude::*;
    /// # fn example(pane: Element) -> Element {
    /// EffectScope::new()
    ///     .cells_only()
    ///     .tint_by(Color::Rgb(220, 60, 60), 0.08)
    ///     .child(pane)
    ///     .into()
    /// # }
    /// ```
    pub fn cells_only(mut self) -> Self {
        self.cells_only = true;
        self
    }

    /// Remove all visual effects from this scope.
    pub fn clear_effects(mut self) -> Self {
        self.effects.clear();
        self
    }
}

impl From<EffectScope> for Element {
    fn from(value: EffectScope) -> Self {
        let (min_w, min_h) = measure_effect_scope(&value, None, None);
        Element::new(ElementKind::EffectScope(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for EffectScope {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.effects.hash(hasher);
        if let Some(child) = self.child.as_ref() {
            recurse(child.as_ref())?.hash(hasher);
        } else {
            0u8.hash(hasher);
        }
        Some(())
    }
}
