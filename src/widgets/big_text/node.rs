use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::node::WidgetNode;
use crate::utils::gradient::{ColorGradient, GradientDirection};

use super::{BigFont, Shadow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BigTextCacheKey {
    text_hash: u64,
    font: BigFont,
    style: crate::style::Style,
    shadow: Option<Shadow>,
    custom_figlet_hash: Option<u64>,
}

impl BigTextCacheKey {
    pub(crate) fn new(
        text: &crate::style::RichText,
        font: BigFont,
        style: crate::style::Style,
        shadow: Option<Shadow>,
        custom_figlet: Option<&Arc<str>>,
    ) -> Self {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let text_hash = hasher.finish();

        let custom_figlet_hash = custom_figlet.map(|content| {
            let mut hasher = DefaultHasher::new();
            content.hash(&mut hasher);
            hasher.finish()
        });

        Self {
            text_hash,
            font,
            style,
            shadow,
            custom_figlet_hash,
        }
    }
}

#[derive(Clone)]
pub struct BigTextNode {
    pub text: crate::style::RichText,
    pub font: BigFont,
    pub style: crate::style::Style,
    pub shadow: Option<Shadow>,
    pub custom_figlet: Option<Arc<str>>,
    pub output: Arc<super::BigTextRenderOutput>,
    pub cache_key: BigTextCacheKey,
    /// Render-time gradient - not part of the glyph cache key.
    pub gradient: Option<(ColorGradient, GradientDirection)>,
}

impl WidgetNode for BigTextNode {}
