//! Tag system for element/node type matching during reconciliation.

use crate::core::element::{Element, ElementKind};
use crate::core::node::{Node, NodeKind};

// ---------------------------------------------------------------------------
// Helper macros that expand the categorised variant list into Tag code.
// ---------------------------------------------------------------------------

/// Generate the `Tag` enum, `tag_of_element()`, and `tag_of_node()`.
macro_rules! impl_tags {
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
        /// Tag for identifying element/node types during reconciliation.
        #[allow(missing_docs)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Tag {
            // All non-gated variants (every category)
            $( $v, )*
            $( $dnh, )*
            $( $pd, )*
            $( $cah, )*
            $( $cf, )*
            $( $cfnh, )*
            $( $nd, )*
            $( $eo, )*
            // Gated variants
            $( #[cfg(feature = $gf)] $gv, )*
            $( #[cfg(feature = $dnhgf)] $dnhg, )*
            $( #[cfg(feature = $cahgf)] $cahg, )*
        }

        pub(crate) fn tag_of_element(el: &Element) -> Tag {
            match &el.kind {
                $( ElementKind::$v(_) => Tag::$v, )*
                $( ElementKind::$dnh(_) => Tag::$dnh, )*
                $( ElementKind::$pd(_) => Tag::$pd, )*
                $( ElementKind::$cah(_) => Tag::$cah, )*
                $( ElementKind::$cf(_) => Tag::$cf, )*
                $( ElementKind::$cfnh(_) => Tag::$cfnh, )*
                $( ElementKind::$nd(_) => Tag::$nd, )*
                $( ElementKind::$eo(_) => Tag::$eo, )*
                $( #[cfg(feature = $gf)] ElementKind::$gv(_) => Tag::$gv, )*
                $( #[cfg(feature = $dnhgf)] ElementKind::$dnhg(_) => Tag::$dnhg, )*
                $( #[cfg(feature = $cahgf)] ElementKind::$cahg(_) => Tag::$cahg, )*
            }
        }

        pub(crate) fn tag_of_node(node: &Node) -> Tag {
            match &node.kind {
                // All NodeKind variants (excludes @element_only_const_auto)
                $( NodeKind::$v(_) => Tag::$v, )*
                $( NodeKind::$dnh(_) => Tag::$dnh, )*
                $( NodeKind::$pd(_) => Tag::$pd, )*
                $( NodeKind::$cah(_) => Tag::$cah, )*
                $( NodeKind::$cf(_) => Tag::$cf, )*
                $( NodeKind::$cfnh(_) => Tag::$cfnh, )*
                $( NodeKind::$nd(_) => Tag::$nd, )*
                // Note: $eo (element_only) variants are NOT in NodeKind
                $( #[cfg(feature = $gf)] NodeKind::$gv(_) => Tag::$gv, )*
                $( #[cfg(feature = $dnhgf)] NodeKind::$dnhg(_) => Tag::$dnhg, )*
                $( #[cfg(feature = $cahgf)] NodeKind::$cahg(_) => Tag::$cahg, )*
            }
        }
    };
}

for_all_widget_variants!(impl_tags);

pub(crate) fn unwrap_transparent_element(el: &Element) -> &Element {
    match &el.kind {
        ElementKind::ThemeProvider(tp) => unwrap_transparent_element(&tp.child),
        ElementKind::ContextProvider(cp) => unwrap_transparent_element(&cp.child),
        _ => el,
    }
}

pub(crate) fn reuse_key_of_element(el: &Element) -> Option<crate::core::element::Key> {
    unwrap_transparent_element(el).key.clone()
}

/// Check if a node can be reused for an element.
pub(crate) fn can_reuse(node: &Node, el: &Element) -> bool {
    let el = unwrap_transparent_element(el);
    node.key == el.key && tag_of_node(node) == tag_of_element(el)
}
