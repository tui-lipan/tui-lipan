use std::time::Duration;

use crate::core::node::{NodeKind, WidgetNode};
use crate::style::VisualEffect;

use super::EffectScope;

/// Runtime node for subtree effect post-processing.
#[derive(Clone, Debug, Default)]
pub struct EffectScopeNode {
    pub effects: Vec<VisualEffect>,
}

impl EffectScopeNode {
    pub fn has_animated_effects(&self) -> bool {
        self.effects.iter().any(VisualEffect::is_animated)
    }

    pub fn animation_interval(&self) -> Option<Duration> {
        self.effects
            .iter()
            .filter_map(VisualEffect::animation_interval)
            .min()
    }
}

impl WidgetNode for EffectScopeNode {}

impl From<EffectScope> for EffectScopeNode {
    fn from(value: EffectScope) -> Self {
        Self {
            effects: value.effects,
        }
    }
}

impl From<EffectScopeNode> for NodeKind {
    fn from(node: EffectScopeNode) -> Self {
        NodeKind::EffectScope(node)
    }
}
