use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{resolve_rect_with_auto, reuse_or_replace_kind};
use crate::style::{LayoutConstraints, Length, Rect};

use super::BigText;
use super::node::{BigTextCacheKey, BigTextNode};

pub fn reconcile_big_text(
    tree: &mut NodeTree,
    id: NodeId,
    big_text: &BigText,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let cache_key = BigTextCacheKey::new(
        &big_text.text,
        big_text.font,
        big_text.style,
        big_text.shadow,
        big_text.custom_figlet.as_ref(),
    );

    let mut cached_output = None;
    {
        let node = tree.node_mut(id);
        if let NodeKind::BigText(existing) = &mut node.kind
            && existing.cache_key == cache_key
        {
            cached_output = Some(existing.output.clone());
        }
    }

    let output = cached_output.unwrap_or_else(|| big_text.build_lines());
    let (natural_w, natural_h) = (output.width, output.height);

    let rect = resolve_rect_with_auto(
        rect,
        constraints,
        Length::Auto,
        Length::Auto,
        natural_w,
        natural_h,
    );

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    let replace_output = output.clone();

    reuse_or_replace_kind(
        &mut node.kind,
        |kind| {
            if let NodeKind::BigText(existing) = kind {
                existing.output = output.clone();
                existing.text = big_text.text.clone();
                existing.font = big_text.font;
                existing.style = big_text.style;
                existing.shadow = big_text.shadow;
                existing.custom_figlet = big_text.custom_figlet.clone();
                existing.cache_key = cache_key;
                existing.gradient = big_text.gradient;
                true
            } else {
                false
            }
        },
        || {
            NodeKind::BigText(BigTextNode {
                text: big_text.text.clone(),
                font: big_text.font,
                style: big_text.style,
                shadow: big_text.shadow,
                custom_figlet: big_text.custom_figlet.clone(),
                output: replace_output,
                cache_key,
                gradient: big_text.gradient,
            })
        },
    );

    id
}
