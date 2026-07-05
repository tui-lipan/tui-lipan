//! Central widget variant manifest.
//!
//! [`for_all_widget_variants!`] is the **single source of truth** for the set
//! of widget variants that appear in `ElementKind`, `NodeKind`, `Tag`, and the
//! various dispatch match arms across the crate.  Adding a new widget to the
//! manifest automatically wires up:
//!
//! - `Tag` enum + `tag_of_element()` + `tag_of_node()`  (in `layout/tag.rs`)
//! - `ElementKind::dimensions()` standard arms  (in `core/element.rs`)
//! - `LayoutHash for ElementKind` delegation arms  (in `layout/hash.rs`)
//! - `node_kind_delegate_match!` variant arms  (in `core/node/kind.rs` via
//!   `scripts/generate-node-kind-delegate-arms.py`)
//!
//! **Not** currently generated (stable Rust cannot nest `macro_rules!`):
//! - `renderers/mod.rs` module declarations  (module names don't map 1:1)
//!
//! # Macro callback pattern
//!
//! `for_all_widget_variants!` invokes a caller-supplied `$callback` macro with
//! the full categorised variant list.  Each consumer site defines a small
//! callback macro that destructures only the categories it cares about.
//!
//! # Categories
//!
//! | Category | `dimensions()` | `layout_hash` | In `NodeKind` | Feature |
//! |---|---|---|---|---|
//! | `direct` | `w.width, w.height` | delegate | yes | - |
//! | `direct_gated` | `w.width, w.height` | delegate | yes | ✓ |
//! | `direct_no_hash` | `w.width, w.height` | `None` | yes | - |
//! | `direct_no_hash_gated` | `w.width, w.height` | `None` | yes | ✓ |
//! | `props_dims` | `w.props.{w,h}` | delegate | yes | - |
//! | `const_auto_hash` | `(Auto, Auto)` | delegate | yes | - |
//! | `const_auto_hash_gated` | `(Auto, Auto)` | delegate | yes | ✓ |
//! | `const_flex` | `(Flex(1), Flex(1))` | delegate | yes | - |
//! | `const_flex_no_hash` | `(Flex(1), Flex(1))` | `None` | yes | - |
//! | `no_dims` | `None` | delegate | yes | - |
//! | `element_only_const_auto` | `(Auto, Auto)` | `None` | **no** | - |

/// Invoke `$callback!` with the full categorised widget variant list.
///
/// The callback receives blocks of variants grouped by category.
/// Feature-gated variants carry `=> "feature-name"` annotations.
#[macro_export]
#[doc(hidden)]
macro_rules! for_all_widget_variants {
    ($callback:path) => {
        $callback! {
            // ── direct: w.width, w.height · layout_hash delegates · in NodeKind ──
            @direct [
                Text,
                AsciiCanvas,
                Button,
                Input,
                HexArea,
                List,
                TextArea,
                Table,
                Tabs,
                DraggableTabBar,
                Divider,
                Spacer,
                Checkbox,
                ProgressBar,
                Slider,
                Spinner,
                Splitter,
                Heatmap,
                DocumentView,
                PanView,
                Flow,
                Canvas,
            ]
            // ── direct_gated: same as direct but behind a feature flag ──
            @direct_gated [
                Image => "image",
            ]
            // ── direct_no_hash: w.width, w.height · layout_hash None ──
            @direct_no_hash [
                Sparkline,
                Chart,
                Graph,
                SequenceDiagram,
                Flowchart,
                ClassDiagram,
                StateDiagram,
                ErDiagram,
                GanttDiagram,
                StatusBarLayout,
            ]
            // ── direct_no_hash_gated: same but gated ──
            @direct_no_hash_gated [
                Terminal => "terminal",
            ]
            // ── props_dims: w.props.width, w.props.height · layout_hash delegates ──
            @props_dims [
                VStack,
                HStack,
                ScrollView,
                Grid,
            ]
            // ── const_auto_hash: (Auto, Auto) · layout_hash delegates ──
            @const_auto_hash [
                Portal,
            ]
            // ── const_auto_hash_gated: same but gated ──
            @const_auto_hash_gated [
                BigText => "big-text",
            ]
            // ── const_flex: (Flex(1), Flex(1)) · layout_hash delegates ──
            @const_flex [
                ZStack,
                Center,
            ]
            // ── const_flex_no_hash: (Flex(1), Flex(1)) · layout_hash None ──
            @const_flex_no_hash [
                CenterPin,
            ]
            // ── no_dims: None · layout_hash delegates ──
            @no_dims [
                Animated,
                DragSource,
                DropTarget,
                EffectScope,
                Frame,
                Group,
                MouseRegion,
                Popover,
            ]
            // ── element_only_const_auto: (Auto, Auto) · not in NodeKind ──
            @element_only_const_auto [
                Component,
                ThemeProvider,
                ContextProvider,
                Memo,
            ]
        }
    };
}
