//! Container widgets.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::layout::axis::Axis;
use crate::style::{
    Align, BorderStyle, Justify, LayoutConstraints, Length, Padding, RichText, Style, StyleSlot,
};
use crate::widgets::TabsEvent;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) mod layout;
pub(crate) mod node;
pub(crate) mod reconcile;

pub(crate) use self::layout::measure_stack;

/// Visual style variant for border tabs in a [`VStack`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum TabVariant {
    /// Classic style: `[ Active ]|Inactive`
    #[default]
    Classic,
    /// Minimal style: `Active - Inactive` (no brackets, differentiated by color only)
    Minimal,
    /// Custom brackets and separator.
    Custom {
        /// Characters surrounding the active tab (prefix, suffix).
        active_brackets: (char, char),
        /// String used to separate tabs (e.g., `"|"`, `" | "`, `" :: "`).
        separator: &'static str,
    },
}

/// Focus-aware sizing policy for stack containers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FocusSizing {
    /// No focus-aware sizing.
    #[default]
    None,
    /// Accordion-style focus sizing (lazygit-style).
    Accordion(FocusAccordion),
}

/// Focus traversal behavior for a container subtree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FocusScope {
    /// Use the surrounding focus traversal behavior.
    #[default]
    None,
    /// Remove this subtree from traversal, automatic fallback, and click focus.
    /// Explicit keyed focus requests may still enter the subtree.
    Exclude,
    /// Keep next/previous focus traversal within this subtree while it contains focus.
    Contain,
}

impl FocusSizing {
    /// Use the default accordion policy.
    pub fn accordion() -> Self {
        Self::Accordion(FocusAccordion::default())
    }
}

/// Accordion sizing policy for focused stacks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FocusAccordion {
    /// Minimum height for the focused child.
    pub focused_min: u16,
    /// Height assigned to non-focused children when squashed.
    pub collapsed: u16,
    /// Height assigned to non-focused children in tiny mode.
    pub tiny_collapsed: u16,
    /// Flex weight multiplier for the focused child in accordion mode.
    pub expanded_weight: u16,
    /// Height threshold to enter squashed mode.
    pub squash_threshold: u16,
    /// Height threshold to enter tiny mode.
    pub tiny_threshold: u16,
    /// When `true`, the VStack automatically remembers the last focused child
    /// and keeps it expanded when focus moves outside the stack entirely.
    ///
    /// Defaults to `true` - the right out-of-the-box behaviour for multi-panel
    /// layouts (lazygit-style). Set to `false` to disable.
    pub sticky: bool,
}

impl Default for FocusAccordion {
    fn default() -> Self {
        Self {
            focused_min: 7,
            collapsed: 3,
            tiny_collapsed: 1,
            expanded_weight: 2,
            squash_threshold: 28,
            tiny_threshold: 21,
            sticky: true,
        }
    }
}

impl TabVariant {
    pub(crate) fn separator(&self) -> &'static str {
        match self {
            Self::Classic => "|",
            Self::Minimal => " - ",
            Self::Custom { separator, .. } => separator,
        }
    }

    pub(crate) fn separator_width(&self) -> usize {
        UnicodeWidthStr::width(self.separator())
    }

    pub(crate) fn active_padding_width(&self) -> (usize, usize) {
        match self {
            Self::Classic => (2, 2),
            Self::Minimal => (0, 0),
            Self::Custom {
                active_brackets: (l, r),
                ..
            } => (
                UnicodeWidthChar::width(*l).unwrap_or(1),
                UnicodeWidthChar::width(*r).unwrap_or(1),
            ),
        }
    }

    pub(crate) fn inactive_padding_width(&self) -> (usize, usize) {
        match self {
            Self::Classic => (1, 1),
            Self::Minimal => (0, 0),
            Self::Custom { .. } => (0, 0),
        }
    }

    pub(crate) fn active_surround(
        &self,
    ) -> (
        std::borrow::Cow<'static, str>,
        std::borrow::Cow<'static, str>,
    ) {
        use std::borrow::Cow;
        match self {
            Self::Classic => (Cow::Borrowed("[ "), Cow::Borrowed(" ]")),
            Self::Minimal => (Cow::Borrowed(""), Cow::Borrowed("")),
            Self::Custom {
                active_brackets: (l, r),
                ..
            } => (Cow::Owned(l.to_string()), Cow::Owned(r.to_string())),
        }
    }

    pub(crate) fn inactive_surround(
        &self,
    ) -> (
        std::borrow::Cow<'static, str>,
        std::borrow::Cow<'static, str>,
    ) {
        use std::borrow::Cow;
        match self {
            Self::Classic => (Cow::Borrowed(" "), Cow::Borrowed(" ")),
            Self::Minimal => (Cow::Borrowed(""), Cow::Borrowed("")),
            Self::Custom { .. } => (Cow::Borrowed(""), Cow::Borrowed("")),
        }
    }
}

/// Shared properties for stack containers.
#[derive(Clone, Debug)]
pub(crate) struct StackProps {
    /// Gap between children.
    pub gap: u16,
    /// Padding applied to the inner area.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Base style (used for background fill).
    pub style: Style,
    /// Cross-axis alignment.
    /// Default: `Align::Start`.
    pub align: Align,
    /// Main-axis alignment.
    /// Default: `Justify::Start`.
    pub justify: Justify,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Flex(1)`.
    pub height: Length,
    /// Focus-aware sizing policy.
    pub focus_sizing: FocusSizing,
    /// Focus traversal behavior for this subtree.
    pub focus_scope: FocusScope,
    /// Draw a border.
    pub border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Distribute flex items evenly, ignoring remainders.
    pub even_flex: bool,
}

impl Default for StackProps {
    fn default() -> Self {
        Self {
            gap: 0,
            padding: Padding::default(),
            style: Style::default(),
            align: Align::Start,
            justify: Justify::Start,
            // Containers default to flex-like behavior.
            width: Length::Flex(1),
            height: Length::Flex(1),
            focus_sizing: FocusSizing::None,
            focus_scope: FocusScope::None,
            border: false,
            border_style: BorderStyle::Plain,
            even_flex: false,
        }
    }
}

macro_rules! impl_stack_props {
    ($name:ident) => {
        impl $name {
            /// Add a child.
            pub fn child(mut self, child: impl Into<Element>) -> Self {
                self.children.push(child.into());
                self
            }

            /// Replace all children, discarding anything already added with
            /// [`child`](Self::child). Call `child` repeatedly to append instead.
            pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
                self.children = children.into_iter().collect();
                self
            }

            /// Set border.
            pub fn border(mut self, border: bool) -> Self {
                self.props.border = border;
                self
            }

            /// Set border style.
            pub fn border_style(mut self, border_style: BorderStyle) -> Self {
                self.props.border_style = border_style;
                self
            }

            /// Set padding.
            pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
                self.props.padding = padding.into();
                self
            }

            /// Set base style.
            pub fn style(mut self, style: Style) -> Self {
                self.props.style = style;
                self
            }

            /// Set gap.
            pub fn gap(mut self, gap: u16) -> Self {
                self.props.gap = gap;
                self
            }

            /// Set cross-axis alignment.
            pub fn align(mut self, align: Align) -> Self {
                self.props.align = align;
                self
            }

            /// Distribute flex items perfectly evenly, ignoring pixel remainder.
            pub fn even_flex(mut self, even: bool) -> Self {
                self.props.even_flex = even;
                self
            }

            /// Set main-axis alignment.
            ///
            /// `Justify::SpaceBetween`/`SpaceAround`/`SpaceEvenly` only show
            /// spacing when children have non-flex main-axis sizing. The stack's
            /// default child contribution is `Flex(1)` on the main axis, so
            /// children fill all available space and the layout looks identical
            /// to `Start`. Set each child's main-axis length to `Length::Auto`
            /// or a fixed `Length::Px(_)` to leave slack for the spacer math.
            pub fn justify(mut self, justify: Justify) -> Self {
                self.props.justify = justify;
                self
            }

            /// Override requested width.
            pub fn width(mut self, width: Length) -> Self {
                self.props.width = width;
                self
            }

            /// Override requested height.
            pub fn height(mut self, height: Length) -> Self {
                self.props.height = height;
                self
            }

            /// Set focus-aware sizing behavior.
            pub fn focus_sizing(mut self, sizing: FocusSizing) -> Self {
                self.props.focus_sizing = sizing;
                self
            }

            /// Set focus traversal behavior for this subtree.
            pub fn focus_scope(mut self, scope: FocusScope) -> Self {
                self.props.focus_scope = scope;
                self
            }
        }
    };
}

/// A vertical stack container.
#[derive(Clone, Default)]
pub struct VStack {
    /// Layout properties.
    pub(crate) props: StackProps,
    /// Children.
    pub(crate) children: Vec<Element>,
    /// Optional tab titles rendered in the top border.
    pub(crate) tab_titles: Vec<RichText>,
    /// Index of the active tab.
    pub(crate) active_tab: usize,
    /// Callback fired when a border tab is clicked.
    pub(crate) on_tab_change: Option<Callback<TabsEvent>>,
    /// Style applied to the active tab.
    pub(crate) active_tab_style: StyleSlot,
    /// Style applied to inactive tabs and separators.
    pub(crate) inactive_tab_style: Style,
    /// Visual variant for border tabs.
    pub(crate) tab_variant: TabVariant,
    /// Optional title prefix (rendered before tabs).
    pub(crate) title_prefix: Option<Arc<str>>,
}

impl_stack_props!(VStack);

impl VStack {
    /// Create an empty vertical stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set tab titles rendered in the top border.
    pub fn tab_titles<I, S>(mut self, titles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<RichText>,
    {
        self.tab_titles = titles.into_iter().map(Into::into).collect();
        self
    }

    /// Set the active tab index.
    pub fn active_tab(mut self, active_tab: usize) -> Self {
        self.active_tab = active_tab;
        self
    }

    /// Set the style for the active tab.
    pub fn active_tab_style(mut self, style: Style) -> Self {
        self.active_tab_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed active tab style with the given style.
    pub fn extend_active_tab_style(mut self, style: Style) -> Self {
        self.active_tab_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit active tab style from the active theme.
    pub fn inherit_active_tab_style(mut self) -> Self {
        self.active_tab_style = StyleSlot::Inherit;
        self
    }

    /// Set the active tab style slot directly.
    pub fn active_tab_style_slot(mut self, slot: StyleSlot) -> Self {
        self.active_tab_style = slot;
        self
    }

    /// Set the style for inactive tabs.
    pub fn inactive_tab_style(mut self, style: Style) -> Self {
        self.inactive_tab_style = style;
        self
    }

    /// Callback fired when the active tab changes via border tab clicks.
    pub fn on_tab_change(mut self, cb: Callback<TabsEvent>) -> Self {
        self.on_tab_change = Some(cb);
        self
    }

    /// Set the visual variant for border tabs.
    pub fn tab_variant(mut self, variant: TabVariant) -> Self {
        self.tab_variant = variant;
        self
    }

    /// Set an optional prefix rendered before the tabs.
    pub fn title_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.title_prefix = Some(prefix.into());
        self
    }

    pub(crate) fn border_index_at_col(
        tab_titles: &[RichText],
        active_tab: usize,
        tab_variant: TabVariant,
        col: usize,
    ) -> Option<usize> {
        let mut x = 0usize;

        for (i, title) in tab_titles.iter().enumerate() {
            let title_w = title.width();

            let (pad_l, pad_r) = if i == active_tab {
                tab_variant.active_padding_width()
            } else {
                tab_variant.inactive_padding_width()
            };

            let w = title_w.saturating_add(pad_l).saturating_add(pad_r);

            if col < x.saturating_add(w) {
                return Some(i);
            }
            x = x.saturating_add(w);

            // Add separator width if not the last tab.
            if i + 1 < tab_titles.len() {
                let sep_w = tab_variant.separator_width();
                if col < x.saturating_add(sep_w) {
                    return None; // Click on separator.
                }
                x = x.saturating_add(sep_w);
            }
        }

        None
    }
}

impl From<VStack> for Element {
    fn from(value: VStack) -> Self {
        let has_children = !value.children.is_empty();
        // For Flex containers, use minimal chrome size to prevent
        // claiming full content size during min-size calculations.
        let is_flex_h = matches!(value.props.height, Length::Flex(_));
        let is_flex_w = matches!(value.props.width, Length::Flex(_));
        let is_auto_h = matches!(value.props.height, Length::Auto);
        let chrome_h = value.props.padding.vertical() + if value.props.border { 2 } else { 0 };
        let auto_h_depends_on_width = is_auto_h
            && value
                .children
                .iter()
                .any(crate::widgets::scroll_child_height_depends_on_width);

        let (min_w, measured_min_h) = match (is_flex_w, is_flex_h) {
            (true, true) => {
                let mut w = value.props.padding.horizontal();
                let mut h = value.props.padding.vertical();
                if value.props.border {
                    w += 2;
                    h += 2;
                }
                if has_children {
                    w = w.max(1);
                    h = h.max(1);
                }
                (w, h)
            }
            (true, false) => {
                let (_w, h) =
                    measure_stack(&value.props, &value.children, Axis::Vertical, None, None);
                let mut min_w = value.props.padding.horizontal();
                if value.props.border {
                    min_w += 2;
                }
                (min_w, h)
            }
            (false, true) => {
                let (w, content_h) =
                    measure_stack(&value.props, &value.children, Axis::Vertical, None, None);
                let mut chrome_h = value.props.padding.vertical();
                if value.props.border {
                    chrome_h += 2;
                }
                (w, chrome_h.max(content_h))
            }
            (false, false) => {
                measure_stack(&value.props, &value.children, Axis::Vertical, None, None)
            }
        };

        let min_h = if auto_h_depends_on_width {
            chrome_h
        } else {
            measured_min_h
        };

        Element::new(ElementKind::VStack(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for VStack {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        crate::layout::hash::hash_stack_props(&self.props, hasher);
        crate::layout::hash::hash_children(&self.children, hasher, recurse)
    }
}

/// A horizontal stack container.
#[derive(Clone)]
pub struct HStack {
    /// Layout properties.
    pub(crate) props: StackProps,
    /// Children.
    pub(crate) children: Vec<Element>,
}

impl Default for HStack {
    fn default() -> Self {
        Self {
            props: StackProps {
                align: Align::Center,
                ..StackProps::default()
            },
            children: Vec::new(),
        }
    }
}

impl_stack_props!(HStack);

impl HStack {
    /// Create an empty horizontal stack.
    pub fn new() -> Self {
        Self::default()
    }
}

impl From<HStack> for Element {
    fn from(value: HStack) -> Self {
        let has_children = !value.children.is_empty();
        // For Flex containers, use minimal chrome size to prevent
        // claiming full content size during min-size calculations.
        let is_flex_h = matches!(value.props.height, Length::Flex(_));
        let is_flex_w = matches!(value.props.width, Length::Flex(_));
        let is_auto_h = matches!(value.props.height, Length::Auto);
        let chrome_h = value.props.padding.vertical() + if value.props.border { 2 } else { 0 };
        let auto_h_depends_on_width = is_auto_h
            && value
                .children
                .iter()
                .any(crate::widgets::scroll_child_height_depends_on_width);

        let (min_w, measured_min_h) = match (is_flex_w, is_flex_h) {
            (true, true) => {
                // Both axes are Flex - only chrome size matters for min constraints.
                // Matches VStack (true, true) behavior. Avoids a full measure_stack
                // call that would recursively measure all children.
                let mut chrome_w = value.props.padding.horizontal();
                let mut chrome_h = value.props.padding.vertical();
                if value.props.border {
                    chrome_w += 2;
                    chrome_h += 2;
                }
                if has_children {
                    chrome_w = chrome_w.max(1);
                    chrome_h = chrome_h.max(1);
                }
                (chrome_w, chrome_h)
            }
            (true, false) => {
                let (_w, h) =
                    measure_stack(&value.props, &value.children, Axis::Horizontal, None, None);
                let mut min_w = value.props.padding.horizontal();
                if value.props.border {
                    min_w += 2;
                }
                (min_w, h)
            }
            (false, true) => {
                let (w, content_h) =
                    measure_stack(&value.props, &value.children, Axis::Horizontal, None, None);
                let mut chrome_h = value.props.padding.vertical();
                if value.props.border {
                    chrome_h += 2;
                }
                (w, chrome_h.max(content_h))
            }
            (false, false) => {
                measure_stack(&value.props, &value.children, Axis::Horizontal, None, None)
            }
        };

        let min_h = if auto_h_depends_on_width {
            chrome_h
        } else {
            measured_min_h
        };

        Element::new(ElementKind::HStack(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for HStack {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        crate::layout::hash::hash_stack_props(&self.props, hasher);
        crate::layout::hash::hash_children(&self.children, hasher, recurse)
    }
}
