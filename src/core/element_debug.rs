use crate::core::element::{Element, ElementKind};
use crate::layout::hash::element_layout_hash;

/// Compare two unexpanded view trees for the debug paint-vs-view guard.
///
/// When [`element_layout_hash`](crate::layout::hash::element_layout_hash) returns `None`
/// (unhashable subtree), common list containers fall back to hashing the full element or
/// comparing children; other kinds compare unequal unless both subtree hashes agree.
pub(crate) fn debug_element_tree_eq(a: &Element, b: &Element) -> bool {
    if a.key != b.key {
        return false;
    }
    if a.layout != b.layout {
        return false;
    }
    match (&a.kind, &b.kind) {
        (ElementKind::Text(ta), ElementKind::Text(tb)) => {
            ta.spans == tb.spans
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
            match (element_layout_hash(a), element_layout_hash(b)) {
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
            match (element_layout_hash(a), element_layout_hash(b)) {
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
            match (element_layout_hash(a), element_layout_hash(b)) {
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
        _ => match (element_layout_hash(a), element_layout_hash(b)) {
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
    match (element_layout_hash(a), element_layout_hash(b)) {
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
