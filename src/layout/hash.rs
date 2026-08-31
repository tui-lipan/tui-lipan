use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;

use crate::core::component::FocusContext;
use crate::core::element::{Element, ElementKind};
use crate::layout::axis::Axis;
use crate::layout::tag::tag_of_element;
use crate::style::{Length, Rect, Span};
use crate::widgets::containers::{FocusSizing, StackProps};

pub(crate) type LayoutHasher = FxHasher;

pub(crate) fn layout_hasher() -> LayoutHasher {
    LayoutHasher::default()
}

/// Trait for widgets that can produce a stable layout hash.
///
/// Implement this on widget structs to declare which fields affect layout
/// sizing and positioning. The `element_layout_hash` dispatcher calls this
/// after hashing the element tag and layout constraints.
///
/// # Recursive hashing
///
/// Widgets that contain child `Element`s (containers, wrappers) should call
/// `recurse(child)?` for each child and hash the returned `u64`. Return
/// `None` to signal an unhashable subtree (forces a layout cache miss).
pub(crate) trait LayoutHash {
    /// Hash layout-affecting fields into `hasher`.
    ///
    /// `recurse` hashes a child element and returns its hash, or `None` if
    /// the child is unhashable.
    ///
    /// Returns `Some(())` on success, `None` if this widget cannot produce a
    /// stable hash (e.g., an unhashable child).
    fn layout_hash(
        &self,
        hasher: &mut impl Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()>;
}

pub(crate) fn stack_layout_hash(
    props: &StackProps,
    children: &[Element],
    axis: Axis,
    bounds: Rect,
    focus: Option<&FocusContext>,
    pinned_key: Option<&str>,
) -> Option<u64> {
    let mut hasher = layout_hasher();
    axis.hash(&mut hasher);
    bounds.hash(&mut hasher);
    hash_stack_props(props, &mut hasher);
    pinned_key.hash(&mut hasher);
    focus
        .and_then(|ctx| ctx.focused_node_id())
        .hash(&mut hasher);

    for child in children {
        let child_hash = element_layout_hash(child)?;
        child_hash.hash(&mut hasher);
    }

    Some(hasher.finish())
}

pub(crate) fn grid_layout_hash(
    grid: &crate::widgets::Grid,
    bounds: Rect,
    focus: Option<&FocusContext>,
) -> Option<u64> {
    let mut hasher = layout_hasher();
    bounds.hash(&mut hasher);
    hash_grid_props(&grid.props, &mut hasher);
    focus
        .and_then(|ctx| ctx.focused_node_id())
        .hash(&mut hasher);

    for item in &grid.items {
        let child_hash = element_layout_hash(&item.element)?;
        child_hash.hash(&mut hasher);
        item.placement.hash(&mut hasher);
        item.span.hash(&mut hasher);
    }

    Some(hasher.finish())
}

pub(crate) fn hash_grid_props(props: &crate::widgets::GridProps, hasher: &mut impl Hasher) {
    props.columns.len().hash(hasher);
    for c in props.columns.iter().copied() {
        let c: Length = c;
        c.hash(hasher);
    }
    props.rows.len().hash(hasher);
    for r in props.rows.iter().copied() {
        let r: Length = r;
        r.hash(hasher);
    }
    props.gap_x.hash(hasher);
    props.gap_y.hash(hasher);
    props.padding.hash(hasher);
    props.align.hash(hasher);
    props.justify.hash(hasher);
    props.width.hash(hasher);
    props.height.hash(hasher);
    props.border.hash(hasher);
    props.border_style.hash(hasher);
}

pub(crate) fn element_layout_hash(el: &Element) -> Option<u64> {
    // SourceSnapshot drag sources include the collapse hint in their hash,
    // so the per-element cache must be skipped when a collapse hint is active
    // (the cached hash is from the previous drag state).
    let skip_cache = is_drag_source_with_active_collapse(el) || has_split_wrap_dynamic_state(el);

    if !skip_cache && let Some(hash) = el.layout_hash_cache.get() {
        return Some(hash);
    }

    let mut hasher = layout_hasher();
    tag_of_element(el).hash(&mut hasher);
    el.layout_constraints().hash(&mut hasher);

    el.kind.layout_hash(&mut hasher, &element_layout_hash)?;

    let hash = hasher.finish();
    if !skip_cache {
        el.layout_hash_cache.set(Some(hash));
    }
    Some(hash)
}

fn is_drag_source_with_active_collapse(el: &Element) -> bool {
    if let ElementKind::DragSource(source) = &el.kind {
        matches!(source.preview, crate::widgets::DragPreview::SourceSnapshot)
            && super::drag_source_layout_hint::drag_source_snapshot_collapse_key().is_some()
    } else {
        false
    }
}

#[cfg(feature = "diff-view")]
fn has_split_wrap_dynamic_state(el: &Element) -> bool {
    crate::widgets::element_subtree_has_split_wrap_sync(el)
}

#[cfg(not(feature = "diff-view"))]
fn has_split_wrap_dynamic_state(_el: &Element) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::element_layout_hash;
    use crate::core::element::{Element, IntoElement};
    use crate::style::{Align, Color, Length, Style};
    use crate::widgets::{DocumentView, Flow, Text};

    #[test]
    fn layout_hash_cache_is_reused_and_invalidated_by_layout_changes() {
        let base = Text::new("hello").min_width(Length::Px(10));

        let hash_a = element_layout_hash(&base).expect("text should be hashable");
        let hash_b = element_layout_hash(&base).expect("text should stay hashable");
        assert_eq!(hash_a, hash_b);

        let changed = base.clone().max_width(Length::Px(20));
        let hash_c = element_layout_hash(&changed).expect("changed text should be hashable");
        assert_ne!(hash_a, hash_c);
    }

    #[test]
    fn flow_layout_hashes_hashable_children_and_ignores_style() {
        let make = |style: Style, gap: u16| -> Element {
            Flow::new()
                .gap(gap)
                .align(Align::Center)
                .padding((1, 2))
                .border(true)
                .style(style)
                .child(Text::new("alpha"))
                .child(Text::new("beta").height(Length::Px(1)))
                .into()
        };

        let base = make(Style::default(), 1);
        let restyled = make(Style::new().fg(Color::rgb(12, 34, 56)), 1);
        assert_eq!(element_layout_hash(&base), element_layout_hash(&restyled));

        let changed_gap = make(Style::default(), 2);
        assert_ne!(
            element_layout_hash(&base),
            element_layout_hash(&changed_gap)
        );
    }

    #[test]
    fn document_view_layout_hash_matches_for_shared_arc() {
        let body: Arc<str> = Arc::from("x".repeat(20_000));
        let el_a: Element = DocumentView::new(body.clone())
            .border(false)
            .scrollbar(false)
            .into();
        let el_b: Element = DocumentView::new(body)
            .border(false)
            .scrollbar(false)
            .into();
        assert_eq!(element_layout_hash(&el_a), element_layout_hash(&el_b));
    }

    #[test]
    fn document_view_layout_hash_matches_for_distinct_arc_same_bytes() {
        let body_a: Arc<str> = Arc::from("same");
        let body_b: Arc<str> = Arc::from("same");
        let el_a: Element = DocumentView::new(body_a)
            .border(false)
            .scrollbar(false)
            .into();
        let el_b: Element = DocumentView::new(body_b)
            .border(false)
            .scrollbar(false)
            .into();
        assert_eq!(
            element_layout_hash(&el_a),
            element_layout_hash(&el_b),
            "same body text must share layout hash so measure caches survive Arc reallocation"
        );
    }

    #[test]
    fn document_view_layout_hash_ignores_themed_document_styles() {
        let base = DocumentView::new("hello world")
            .border(false)
            .scrollbar(false);
        let mut themed = base.clone();
        themed.doc_styles.heading_styles[0] = Style::new().fg(Color::rgb(12, 34, 56)).bold();
        themed.doc_styles.list_item_style = Style::new().fg(Color::rgb(200, 100, 50));

        let a: Element = base.into();
        let b: Element = themed.into();
        assert_eq!(element_layout_hash(&a), element_layout_hash(&b));
    }

    #[cfg(feature = "diff-view")]
    fn first_hstack_children(element: &Element) -> Option<&[Element]> {
        if let crate::core::element::ElementKind::HStack(stack) = &element.kind {
            return Some(stack.children.as_slice());
        }

        element
            .kind
            .children()
            .into_iter()
            .find_map(first_hstack_children)
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn split_wrap_layout_hash_tracks_dynamic_pass_state() {
        use crate::widgets::{DiffView, DiffViewBackend, DiffViewMode, SplitWrapDualPass};

        let element: Element = DiffView::with_content(
            "alpha alpha alpha alpha alpha",
            "beta beta beta beta beta beta",
        )
        .backend(DiffViewBackend::DocumentView)
        .mode(DiffViewMode::Split)
        .wrap(true)
        .scrollbar(false)
        .height(Length::Auto)
        .into();

        let initial = element_layout_hash(&element).expect("split diff should be hashable");
        let children =
            first_hstack_children(&element).expect("split diff should contain an hstack");
        let widths = [40u16, 40u16];
        let pass =
            SplitWrapDualPass::begin_measure(children.iter().zip(widths.iter().copied()), Some(20))
                .expect("split panes should share wrap sync");

        let during_pass = element_layout_hash(&element).expect("pass hash should be hashable");
        assert_ne!(initial, during_pass);

        drop(pass);
        let reset = element_layout_hash(&element).expect("reset hash should be hashable");
        let reset_again = element_layout_hash(&element).expect("reset hash should stay stable");
        assert_ne!(during_pass, reset);
        assert_eq!(reset, reset_again);
    }
}

/// Generate the `LayoutHash for ElementKind` dispatch from the widget manifest.
macro_rules! impl_element_layout_hash {
    (
        @direct [ $($v:ident,)* ]
        @direct_gated [ $($gv:ident => $gf:literal,)* ]
        @direct_no_hash [ $($dnh:ident,)* ]
        @direct_no_hash_gated [ $($dnhg:ident => $dnhgf:literal,)* ]
        @props_dims [ $($pd:ident,)* ]
        @const_auto_hash [ $($cah:ident,)* ]
        @const_auto_hash_gated [ $($cahg:ident => $cahgf:literal,)* ]
        @const_flex [ $($cf:ident,)* ]
        @const_flex_no_hash [ $($cfnh:ident,)* ]
        @no_dims [ $($nd:ident,)* ]
        @element_only_const_auto [ $($eo:ident,)* ]
    ) => {
        impl LayoutHash for ElementKind {
            fn layout_hash(
                &self,
                hasher: &mut impl Hasher,
                recurse: &dyn Fn(&Element) -> Option<u64>,
            ) -> Option<()> {
                match self {
                    // Categories that delegate layout_hash
                    $( Self::$v(w) => w.layout_hash(hasher, recurse), )*
                    $( #[cfg(feature = $gf)] Self::$gv(w) => w.layout_hash(hasher, recurse), )*
                    $( Self::$pd(w) => w.layout_hash(hasher, recurse), )*
                    $( Self::$cah(w) => w.layout_hash(hasher, recurse), )*
                    $( #[cfg(feature = $cahgf)] Self::$cahg(w) => w.layout_hash(hasher, recurse), )*
                    $( Self::$cf(w) => w.layout_hash(hasher, recurse), )*
                    $( Self::$nd(w) => w.layout_hash(hasher, recurse), )*
                    // Categories without layout hashing (forces cache miss)
                    _ => None,
                }
            }
        }
    };
}

for_all_widget_variants!(impl_element_layout_hash);

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn hash_stack_props(props: &StackProps, hasher: &mut impl Hasher) {
    props.gap.hash(hasher);
    props.padding.hash(hasher);
    props.align.hash(hasher);
    props.justify.hash(hasher);
    props.width.hash(hasher);
    props.height.hash(hasher);
    props.focus_scope.hash(hasher);
    props.border.hash(hasher);
    props.border_style.hash(hasher);
    hash_focus_sizing(props.focus_sizing, hasher);
}

fn hash_focus_sizing(sizing: FocusSizing, hasher: &mut impl Hasher) {
    match sizing {
        FocusSizing::None => {
            0u8.hash(hasher);
        }
        FocusSizing::Accordion(policy) => {
            1u8.hash(hasher);
            policy.focused_min.hash(hasher);
            policy.collapsed.hash(hasher);
            policy.tiny_collapsed.hash(hasher);
            policy.expanded_weight.hash(hasher);
            policy.squash_threshold.hash(hasher);
            policy.tiny_threshold.hash(hasher);
            policy.sticky.hash(hasher);
        }
    }
}

pub(crate) fn hash_spans_content(spans: &[Span], hasher: &mut impl Hasher) {
    spans.len().hash(hasher);
    for span in spans {
        span.content.hash(hasher);
    }
}

/// Hash children of a container, returning `None` if any child is unhashable.
pub(crate) fn hash_children(
    children: &[Element],
    hasher: &mut impl Hasher,
    recurse: &dyn Fn(&Element) -> Option<u64>,
) -> Option<()> {
    for child in children {
        recurse(child)?.hash(hasher);
    }
    Some(())
}
