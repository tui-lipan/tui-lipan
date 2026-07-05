use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;

use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Length, Span, Style};

use super::{Overflow, Text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextRenderKey {
    hash: u64,
}

impl TextRenderKey {
    fn new(text: &Text) -> Self {
        let mut hasher = FxHasher::default();
        text.spans.hash(&mut hasher);
        text.style.hash(&mut hasher);
        text.overflow.hash(&mut hasher);
        Self {
            hash: hasher.finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextWidgetKey {
    pub(crate) render_key: TextRenderKey,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl TextWidgetKey {
    fn new(text: &Text, render_key: TextRenderKey) -> Self {
        Self {
            render_key,
            width: text.width,
            height: text.height,
        }
    }
}

#[derive(Clone)]
pub struct TextNode {
    pub spans: Vec<Span>,
    pub style: Style,
    pub overflow: Overflow,
    pub(crate) render_key: TextRenderKey,
    pub(crate) widget_key: TextWidgetKey,
}

impl Default for TextNode {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            style: Style::default(),
            overflow: Overflow::Auto,
            render_key: TextRenderKey { hash: 0 },
            widget_key: TextWidgetKey {
                render_key: TextRenderKey { hash: 0 },
                width: Length::Auto,
                height: Length::Auto,
            },
        }
    }
}

impl WidgetNode for TextNode {}

impl From<Text> for TextNode {
    fn from(text: Text) -> Self {
        let render_key = TextRenderKey::new(&text);
        let widget_key = TextWidgetKey::new(&text, render_key);
        Self {
            spans: text.spans,
            style: text.style,
            overflow: text.overflow,
            render_key,
            widget_key,
        }
    }
}

impl TextNode {
    pub(crate) fn render_key_for(text: &Text) -> TextRenderKey {
        TextRenderKey::new(text)
    }

    pub(crate) fn widget_key_for(text: &Text, render_key: TextRenderKey) -> TextWidgetKey {
        TextWidgetKey::new(text, render_key)
    }
}

impl From<TextNode> for NodeKind {
    fn from(node: TextNode) -> Self {
        NodeKind::Text(node)
    }
}

#[cfg(test)]
mod tests {
    use super::TextNode;
    use crate::style::Length;
    use crate::widgets::text::Text;

    #[test]
    fn width_only_change_keeps_render_key() {
        let base = Text::new("hello");
        let base_render = TextNode::render_key_for(&base);

        let widened = base.clone().width(Length::Px(20));
        let widened_render = TextNode::render_key_for(&widened);

        assert_eq!(base_render, widened_render);
    }

    #[test]
    fn content_change_invalidates_render_key() {
        let a = Text::new("hello");
        let b = Text::new("world");

        let key_a = TextNode::render_key_for(&a);
        let key_b = TextNode::render_key_for(&b);

        assert_ne!(key_a, key_b);
    }

    #[test]
    fn widget_key_tracks_layout_fields() {
        let base = Text::new("hello");
        let render = TextNode::render_key_for(&base);
        let key_base = TextNode::widget_key_for(&base, render);

        let resized = base.clone().height(Length::Px(3));
        let key_resized = TextNode::widget_key_for(&resized, render);

        assert_ne!(key_base, key_resized);
    }
}
