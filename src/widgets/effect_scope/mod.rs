mod layout;
mod node;
mod reconcile;

use std::sync::Arc;

pub(crate) use self::layout::measure_effect_scope;
pub use self::node::EffectScopeNode;
pub(crate) use self::reconcile::reconcile_effect_scope;

use crate::app::ContrastPolicy;
use crate::core::element::{Element, ElementKind};
use crate::style::{CellEffect, Color, ColorTransform, LayoutConstraints, Length, VisualEffect};

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
    pub fn dim_by(self, amount: f32) -> Self {
        self.effect(VisualEffect::ColorTransform {
            fg: Some(ColorTransform::Dim(amount)),
            bg: Some(ColorTransform::Dim(amount)),
        })
    }

    /// Lighten the rendered subtree by an explicit amount.
    pub fn lighten_by(self, amount: f32) -> Self {
        self.effect(VisualEffect::ColorTransform {
            fg: Some(ColorTransform::Lighten(amount)),
            bg: Some(ColorTransform::Lighten(amount)),
        })
    }

    /// Tint the rendered subtree toward a color.
    pub fn tint_by(self, color: Color, alpha: f32) -> Self {
        self.effect(VisualEffect::ColorTransform {
            fg: Some(ColorTransform::Tint(color, alpha)),
            bg: Some(ColorTransform::Tint(color, alpha)),
        })
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
