use std::hash::{Hash, Hasher};

use crate::core::element::{Element, ElementKind};

use crate::layout::hash::{LayoutHash, layout_hasher};
use crate::style::Length;

// Use the normal widget hash machinery, but recurse through this debug-only hash rather than
// the layout cache: fixed-allocation live text may change without invalidating layout. Include
// binding identity and non-live text properties so paint cannot conceal a changed source/style.
fn debug_element_hash(element: &Element) -> Option<u64> {
    let mut hasher = layout_hasher();
    std::mem::discriminant(&element.kind).hash(&mut hasher);
    element.key.hash(&mut hasher);
    element.layout.hash(&mut hasher);
    if let ElementKind::Text(text) = &element.kind {
        text.style.hash(&mut hasher);
        text.overflow.hash(&mut hasher);
        text.width.hash(&mut hasher);
        text.height.hash(&mut hasher);
        text.source
            .as_ref()
            .map(|source| source.identity())
            .hash(&mut hasher);
        if !has_fixed_live_allocation(text) {
            text.spans.hash(&mut hasher);
        }
    } else {
        element.kind.layout_hash(&mut hasher, &debug_element_hash)?;
    }
    Some(hasher.finish())
}

fn has_fixed_live_allocation(text: &crate::widgets::Text) -> bool {
    text.source.is_some()
        && !matches!(text.width, Length::Auto)
        && !matches!(text.height, Length::Auto)
}

/// Compare two unexpanded view trees for the debug paint-vs-view guard.
///
/// Hashable subtrees use a debug hash that permits fixed-allocation live text content updates.
/// Unhashable list containers compare children; other unhashable kinds compare unequal.
pub(crate) fn debug_element_tree_eq(a: &Element, b: &Element) -> bool {
    if a.key != b.key {
        return false;
    }
    if a.layout != b.layout {
        return false;
    }
    match (&a.kind, &b.kind) {
        (ElementKind::Text(ta), ElementKind::Text(tb)) => {
            ta.source.as_ref().map(|source| source.identity())
                == tb.source.as_ref().map(|source| source.identity())
                && (has_fixed_live_allocation(ta) || ta.spans == tb.spans)
                && ta.style == tb.style
                && ta.overflow == tb.overflow
                && ta.width == tb.width
                && ta.height == tb.height
        }
        (ElementKind::Component(ca), ElementKind::Component(cb)) => {
            ca.type_id == cb.type_id && ca.state_key == cb.state_key && ca.props.debug_eq(&cb.props)
        }
        (ElementKind::Group(ga), ElementKind::Group(gb)) => {
            ga.scope == gb.scope && debug_element_tree_eq(ga.child.as_ref(), gb.child.as_ref())
        }
        (ElementKind::Memo(ma), ElementKind::Memo(mb)) => {
            ma.deps_hash == mb.deps_hash && ma.call_site == mb.call_site
        }
        (ElementKind::ThemeProvider(ta), ElementKind::ThemeProvider(tb)) => {
            ta.theme == tb.theme && debug_element_tree_eq(&ta.child, &tb.child)
        }
        (ElementKind::ContextProvider(ca), ElementKind::ContextProvider(cb)) => {
            ca.type_id == cb.type_id
                && ca.generation == cb.generation
                && (ca.equals)(ca.value.as_ref(), cb.value.as_ref())
                && debug_element_tree_eq(&ca.child, &cb.child)
        }
        (ElementKind::VStack(va), ElementKind::VStack(vb)) => {
            debug_container_children_or_hash(a, b, &va.children, &vb.children)
        }
        (ElementKind::HStack(ha), ElementKind::HStack(hb)) => {
            debug_container_children_or_hash(a, b, &ha.children, &hb.children)
        }
        (ElementKind::ZStack(za), ElementKind::ZStack(zb)) => {
            match (debug_element_hash(a), debug_element_hash(b)) {
                (Some(h1), Some(h2)) => h1 == h2,
                _ => {
                    za.style == zb.style
                        && za.passthrough == zb.passthrough
                        && za.children.len() == zb.children.len()
                        && za
                            .children
                            .iter()
                            .zip(zb.children.iter())
                            .all(|(c, d)| debug_element_tree_eq(c, d))
                }
            }
        }
        (ElementKind::Flow(fa), ElementKind::Flow(fb)) => {
            match (debug_element_hash(a), debug_element_hash(b)) {
                (Some(h1), Some(h2)) => h1 == h2,
                _ => {
                    fa.gap == fb.gap
                        && fa.align == fb.align
                        && fa.padding == fb.padding
                        && fa.border == fb.border
                        && fa.border_style == fb.border_style
                        && fa.style == fb.style
                        && fa.width == fb.width
                        && fa.height == fb.height
                        && fa.children.len() == fb.children.len()
                        && fa
                            .children
                            .iter()
                            .zip(fb.children.iter())
                            .all(|(c, d)| debug_element_tree_eq(c, d))
                }
            }
        }
        (ElementKind::ScrollView(sa), ElementKind::ScrollView(sb)) => {
            match (debug_element_hash(a), debug_element_hash(b)) {
                (Some(h1), Some(h2)) => h1 == h2,
                _ => {
                    sa.children.len() == sb.children.len()
                        && sa
                            .children
                            .iter()
                            .zip(sb.children.iter())
                            .all(|(c, d)| debug_element_tree_eq(c, d))
                }
            }
        }
        (a_kind, b_kind) if std::mem::discriminant(a_kind) != std::mem::discriminant(b_kind) => {
            false
        }
        _ => match (debug_element_hash(a), debug_element_hash(b)) {
            (Some(ha), Some(hb)) => ha == hb,
            _ => false,
        },
    }
}

fn debug_container_children_or_hash(
    a: &Element,
    b: &Element,
    a_children: &[Element],
    b_children: &[Element],
) -> bool {
    match (debug_element_hash(a), debug_element_hash(b)) {
        (Some(h1), Some(h2)) => h1 == h2,
        _ => {
            a_children.len() == b_children.len()
                && a_children
                    .iter()
                    .zip(b_children.iter())
                    .all(|(c, d)| debug_element_tree_eq(c, d))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Length, Span, Style};
    use crate::widgets::{Overflow, Text, TextSource, VStack};

    #[test]
    fn paint_guard_allows_live_content_updates_directly_and_in_containers() {
        let source = TextSource::new([Span::new("before")]);
        let before: Element = Text::from_source(source.clone()).into();
        let nested_before: Element = VStack::new().child(before.clone()).into();
        // Prime layout caches too: these must not dictate debug paint equivalence.
        crate::layout::hash::element_layout_hash(&nested_before);
        source.set([Span::new("updated").style(Style::default().fg(Color::Red))]);
        let after: Element = Text::from_source(source).into();
        let nested_after: Element = VStack::new().child(after.clone()).into();
        assert!(debug_element_tree_eq(&before, &after));
        assert!(debug_element_tree_eq(&nested_before, &nested_after));
    }

    #[test]
    fn paint_guard_still_rejects_non_live_changes() {
        let source = TextSource::new([Span::new("same")]);
        let original = Text::from_source(source.clone());
        let changes = [
            original.clone().style(Style::default().fg(Color::Red)),
            original.clone().width(Length::Px(5)),
            original.clone().height(Length::Px(2)),
            original.clone().overflow(Overflow::Ellipsis),
            Text::from_source(TextSource::new([Span::new("same")])),
            Text::new("same")
                .width(Length::Flex(1))
                .height(Length::Px(1)),
        ];
        for changed in changes {
            let before: Element = original.clone().into();
            let after: Element = changed.into();
            assert!(!debug_element_tree_eq(&before, &after));
            assert!(!debug_element_tree_eq(
                &VStack::new().child(before).into(),
                &VStack::new().child(after).into()
            ));
        }
        for (width, height) in [(Length::Auto, Length::Px(1)), (Length::Px(5), Length::Auto)] {
            let before: Element = Text::from_source(source.clone())
                .width(width)
                .height(height)
                .into();
            source.set([Span::new("changed")]);
            let after: Element = Text::from_source(source.clone())
                .width(width)
                .height(height)
                .into();
            assert!(!debug_element_tree_eq(&before, &after));
            source.set([Span::new("same")]);
        }
        assert!(!debug_element_tree_eq(
            &Text::new("before").into(),
            &Text::new("after").into()
        ));
    }
}
