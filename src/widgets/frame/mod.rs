//! Frame widget.

pub mod box_metrics;
pub mod layout;
pub mod node;
pub mod reconcile;

pub(crate) use self::box_metrics::{FrameGeometry, FrameJoinOverlap, compute_frame_geometry};
pub(crate) use self::layout::{measure_frame, measure_frame_chrome};
pub(crate) use self::reconcile::reconcile_frame;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{
    Align, BorderEdges, BorderStyle, Edge, LayoutConstraints, Length, Padding, RichText, Style,
    StyleSlot,
};
use crate::widgets::{TabVariant, TabsEvent};

pub use self::node::FrameNode;
// Internal renderer alias.
pub(crate) use self::node::FrameProps;

/// A label rendered in one of a frame's border positions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FrameLabel {
    /// Label content.
    pub content: RichText,
    /// Optional style layered on top of the containing group style.
    pub style: Option<Style>,
    /// Optional style layered on top of the group focus style while focused.
    pub focused_style: Option<Style>,
}

impl FrameLabel {
    /// Create a border label.
    pub fn new(content: impl Into<RichText>) -> Self {
        Self {
            content: content.into(),
            style: None,
            focused_style: None,
        }
    }

    /// Set the normal label style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Set the label style while its frame is focused.
    pub fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = Some(style);
        self
    }
}

impl From<RichText> for FrameLabel {
    fn from(content: RichText) -> Self {
        Self::new(content)
    }
}

impl From<String> for FrameLabel {
    fn from(content: String) -> Self {
        Self::new(content)
    }
}

impl From<std::sync::Arc<str>> for FrameLabel {
    fn from(content: std::sync::Arc<str>) -> Self {
        Self::new(content)
    }
}

impl From<&str> for FrameLabel {
    fn from(content: &str) -> Self {
        Self::new(content.to_owned())
    }
}

impl From<crate::style::Span> for FrameLabel {
    fn from(content: crate::style::Span) -> Self {
        Self::new(content)
    }
}

/// Positional labels rendered in one border row of a frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BorderLabels {
    /// Left-aligned label.
    pub left: Option<FrameLabel>,
    /// Center-aligned label.
    pub center: Option<FrameLabel>,
    /// Right-aligned label.
    pub right: Option<FrameLabel>,
    /// Default style for labels in this group.
    pub style: Style,
    /// Optional style layered on top while the frame is focused.
    pub focused_style: Option<Style>,
    /// Horizontal padding around each label.
    pub padding: Padding,
}

impl BorderLabels {
    /// Create an empty label group.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the left label.
    pub fn left(mut self, label: impl Into<FrameLabel>) -> Self {
        self.left = Some(label.into());
        self
    }

    /// Set the centered label.
    pub fn center(mut self, label: impl Into<FrameLabel>) -> Self {
        self.center = Some(label.into());
        self
    }

    /// Set the right label.
    pub fn right(mut self, label: impl Into<FrameLabel>) -> Self {
        self.right = Some(label.into());
        self
    }

    /// Set the default style for labels in this group.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the group style while the frame is focused.
    pub fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = Some(style);
        self
    }

    /// Set horizontal padding around labels in this group.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub(crate) fn has_labels(&self) -> bool {
        [&self.left, &self.center, &self.right]
            .into_iter()
            .flatten()
            .any(|label| !label.content.is_empty())
    }

    pub(crate) fn min_width(&self) -> usize {
        [&self.left, &self.center, &self.right]
            .into_iter()
            .flatten()
            .map(|label| {
                label
                    .content
                    .width()
                    .saturating_add(self.padding.left as usize)
                    .saturating_add(self.padding.right as usize)
            })
            .sum()
    }
}

/// Strategy used when frame border symbols overlap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderMergeMode {
    /// Last write wins; no symbol merging.
    Replace,
    /// Merge only when an exact box-drawing symbol exists.
    #[default]
    Exact,
    /// Merge using closest match when exact merge is unavailable.
    Fuzzy,
}

/// Which border line a [`Frame`]'s tab strip is drawn on.
///
/// Tabs share their line with that border's labels, so `Bottom` puts them
/// beside [`Frame::footer_left`] / [`Frame::footer_right`] rather than beside
/// the header labels.
///
/// Bottom tabs are worth reaching for when the frame is anchored to the bottom
/// of its container: that edge is the one that stays put, so the tab strip
/// keeps its screen position even as the body above it changes height.
///
/// A `compact` frame has only one line and draws its tabs there whatever the
/// edge says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabEdge {
    /// Draw tabs on the top border, beside the header labels.
    #[default]
    Top,
    /// Draw tabs on the bottom border, beside the footer labels.
    Bottom,
}

/// Where an edge decoration is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DecorationPlacement {
    /// Draw on the frame border line (or outer edge if no border).
    #[default]
    Border,
    /// Draw inside the content area edge (after border + padding).
    Inside,
    /// Draw outside the frame content, growing the frame size.
    Outside,
}

/// Glyphs used for edge decorations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DecorationGlyph {
    /// Auto thin glyph (horizontal/vertical based on edge).
    AutoThin,
    /// Auto heavy glyph (horizontal/vertical based on edge).
    #[default]
    AutoHeavy,
    /// Auto double-line glyph (horizontal/vertical based on edge).
    AutoDouble,
    /// Auto block glyph (horizontal uses ▬, vertical uses ┃).
    AutoBlock,
    /// Horizontal thin line (─).
    HorizontalThin,
    /// Horizontal heavy line (━).
    HorizontalHeavy,
    /// Horizontal double line (═).
    HorizontalDouble,
    /// Horizontal block (▬).
    HorizontalBlock,
    /// Vertical thin line (│).
    VerticalThin,
    /// Vertical heavy line (┃).
    VerticalHeavy,
    /// Vertical double line (║).
    VerticalDouble,
    /// Auto half-block glyph (edge-based).
    HalfBlock,
    /// Half-block top glyph (▄).
    HalfBlockTop,
    /// Half-block bottom glyph (▀).
    HalfBlockBottom,
    /// Half-block left glyph (▌).
    HalfBlockLeft,
    /// Half-block right glyph (▐).
    HalfBlockRight,
    /// Vertical cap top glyph (╻).
    CapTop,
    /// Vertical cap bottom glyph (╹).
    CapBottom,
    /// Horizontal cap left glyph (╺).
    CapLeft,
    /// Horizontal cap right glyph (╸).
    CapRight,
    /// Vertical cap top heavy glyph (╿).
    CapTopHeavy,
    /// Vertical cap bottom heavy glyph (╽).
    CapBottomHeavy,
    /// Horizontal cap left heavy glyph (╾).
    CapLeftHeavy,
    /// Horizontal cap right heavy glyph (╼).
    CapRightHeavy,
    /// Custom single glyph.
    Custom(char),
}

impl DecorationGlyph {
    pub(crate) fn resolve(self, edge: Edge) -> char {
        match self {
            Self::AutoThin => match edge {
                Edge::Left | Edge::Right => '│',
                Edge::Top | Edge::Bottom => '─',
            },
            Self::AutoHeavy => match edge {
                Edge::Left | Edge::Right => '┃',
                Edge::Top | Edge::Bottom => '━',
            },
            Self::AutoDouble => match edge {
                Edge::Left | Edge::Right => '║',
                Edge::Top | Edge::Bottom => '═',
            },
            Self::AutoBlock => match edge {
                Edge::Left | Edge::Right => '┃',
                Edge::Top | Edge::Bottom => '▬',
            },
            Self::HorizontalThin => '─',
            Self::HorizontalHeavy => '━',
            Self::HorizontalDouble => '═',
            Self::HorizontalBlock => '▬',
            Self::VerticalThin => '│',
            Self::VerticalHeavy => '┃',
            Self::VerticalDouble => '║',
            Self::HalfBlock => match edge {
                Edge::Top => '▄',
                Edge::Bottom => '▀',
                Edge::Left => '▌',
                Edge::Right => '▐',
            },
            Self::HalfBlockTop => '▄',
            Self::HalfBlockBottom => '▀',
            Self::HalfBlockLeft => '▌',
            Self::HalfBlockRight => '▐',
            Self::CapTop => '╻',
            Self::CapBottom => '╹',
            Self::CapLeft => '╺',
            Self::CapRight => '╸',
            Self::CapTopHeavy => '╿',
            Self::CapBottomHeavy => '╽',
            Self::CapLeftHeavy => '╾',
            Self::CapRightHeavy => '╼',
            Self::Custom(ch) => ch,
        }
    }
}

/// Edge decoration descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EdgeDecoration {
    /// Target edge for the decoration.
    pub edge: Edge,
    /// Placement relative to the frame content.
    pub placement: DecorationPlacement,
    /// Thickness in cells (width for vertical, height for horizontal).
    pub thickness: u16,
    /// Glyph used to draw the decoration.
    pub glyph: DecorationGlyph,
    /// Base style for the decoration.
    pub style: Style,
    /// Optional style applied when focused.
    pub focus_style: Option<Style>,
    /// Optional style applied when hovered.
    pub hover_style: Option<Style>,
    /// Optional glyph for the start cap (top/left).
    pub cap_start: Option<DecorationGlyph>,
    /// Optional glyph for the end cap (bottom/right).
    pub cap_end: Option<DecorationGlyph>,
}

impl EdgeDecoration {
    /// Create a new decoration targeting the given edge.
    pub fn new(edge: Edge) -> Self {
        Self {
            edge,
            placement: DecorationPlacement::Border,
            thickness: 1,
            glyph: DecorationGlyph::default(),
            style: Style::default(),
            focus_style: None,
            hover_style: None,
            cap_start: None,
            cap_end: None,
        }
    }

    /// Set the decoration glyph.
    pub fn glyph(mut self, glyph: DecorationGlyph) -> Self {
        self.glyph = glyph;
        self
    }

    /// Set the decoration thickness in cells.
    pub fn thickness(mut self, thickness: u16) -> Self {
        self.thickness = thickness.max(1);
        self
    }

    /// Set the base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style used when focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = Some(style);
        self
    }

    /// Set the style used when hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = Some(style);
        self
    }

    /// Set the start cap glyph.
    pub fn cap_start(mut self, glyph: DecorationGlyph) -> Self {
        self.cap_start = Some(glyph);
        self
    }

    /// Set the end cap glyph.
    pub fn cap_end(mut self, glyph: DecorationGlyph) -> Self {
        self.cap_end = Some(glyph);
        self
    }

    /// Set the placement relative to the frame.
    pub fn placement(mut self, placement: DecorationPlacement) -> Self {
        self.placement = placement;
        self
    }
}

/// A frame container (lazygit-style panel).
#[derive(Clone, Default)]
pub struct Frame {
    /// Frame properties.
    pub(crate) props: FrameNode,
    pub(crate) header: Option<Box<Element>>,
    /// Child.
    pub(crate) child: Option<Box<Element>>,
}

impl Frame {
    /// Create a frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set tab titles rendered in the top border.
    pub fn tab_titles<I, S>(mut self, titles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<RichText>,
    {
        self.props.tab_titles = titles.into_iter().map(Into::into).collect();
        self
    }

    /// Set the active tab index.
    pub fn active_tab(mut self, active_tab: usize) -> Self {
        self.props.active_tab = active_tab;
        self
    }

    /// Set the style for the active tab.
    pub fn active_tab_style(mut self, style: Style) -> Self {
        self.props.active_tab_style = style;
        self
    }

    /// Set the style for the active tab when focused.
    pub fn focus_active_tab_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_active_tab_style = Some(style);
        self
    }

    /// Set the style for inactive tabs.
    pub fn inactive_tab_style(mut self, style: Style) -> Self {
        self.props.inactive_tab_style = style;
        self
    }

    /// Set the style for inactive tabs when focused.
    pub fn focus_inactive_tab_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_inactive_tab_style = Some(style);
        self
    }

    /// Callback fired when the active tab changes via border tab clicks.
    pub fn on_tab_change(mut self, cb: Callback<TabsEvent>) -> Self {
        self.props.on_tab_change = Some(cb);
        self
    }

    /// Set which border line the tab strip is drawn on.
    ///
    /// Default: `TabEdge::Top`.
    pub fn tab_edge(mut self, edge: TabEdge) -> Self {
        self.props.tab_edge = edge;
        self
    }

    /// Set the visual variant for border tabs.
    pub fn tab_variant(mut self, variant: TabVariant) -> Self {
        self.props.tab_variant = variant;
        self
    }

    /// Set labels rendered in the top border.
    pub fn header(mut self, header: BorderLabels) -> Self {
        self.props.header = Box::new(header);
        self
    }

    /// Set the left header label.
    pub fn header_left(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.header.left = Some(label.into());
        self
    }

    /// Set the centered header label.
    pub fn header_center(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.header.center = Some(label.into());
        self
    }

    /// Set the right header label.
    pub fn header_right(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.header.right = Some(label.into());
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.props.style = style;
        self
    }

    /// Set style for the inner content area (distinct from border).
    pub fn inner_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().inner_style = Some(style);
        self
    }

    /// Set style applied when the frame or its children have focus.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed focus style with the given style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.props.overrides_mut().focus_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set the focus style slot directly.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.overrides_mut().focus_style = Some(slot);
        self
    }

    /// Set style when hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed hover style with the given style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.props.overrides_mut().hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set the hover style slot directly.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.overrides_mut().hover_style = Some(slot);
        self
    }

    /// Set border style applied when focused.
    pub fn focus_border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.overrides_mut().focus_border_style = Some(border_style);
        self
    }

    /// Enable or disable border decoration.
    pub fn border(mut self, border: bool) -> Self {
        self.props.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.border_style = border_style;
        self
    }

    /// Set which border edges reserve layout space and render as frame chrome.
    ///
    /// `BorderEdges::HorizontalCaps` keeps the top and bottom border rows with
    /// corner caps, but does not consume left or right content columns.
    pub fn border_edges(mut self, border_edges: BorderEdges) -> Self {
        self.props.border_edges = border_edges;
        self
    }

    /// Set merge behavior for overlapping frame border symbols.
    pub fn border_merge_mode(mut self, merge_mode: BorderMergeMode) -> Self {
        self.props.border_merge_mode = merge_mode;
        self
    }

    /// Join borders with neighboring frames when edges touch.
    pub fn join_frame(mut self, join: bool) -> Self {
        self.props.join_frame = join;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.padding = padding.into();
        self
    }

    /// Add an edge decoration.
    pub fn decoration(mut self, decoration: EdgeDecoration) -> Self {
        self.props.decorations.push(decoration);
        self
    }

    /// Replace all decorations.
    pub fn decorations(mut self, decorations: Vec<EdgeDecoration>) -> Self {
        self.props.decorations = decorations;
        self
    }

    /// Set a content header element rendered inside the frame.
    pub fn header_content(mut self, header: impl Into<Element>) -> Self {
        self.header = Some(Box::new(header.into()));
        self.props.has_header = true;
        self
    }

    /// Set child.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
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

    /// Override requested height when not focused.
    pub fn unfocused_height(mut self, height: Length) -> Self {
        self.props.unfocused_height = Some(height);
        self
    }

    /// Set minimum height when focused (includes borders).
    pub fn focus_min_height(mut self, height: u16) -> Self {
        self.props.focus_min_height = Some(height);
        self
    }

    /// Enable compact single-line rendering mode.
    ///
    /// In compact mode, the frame renders as a single horizontal line with dashes
    /// and the title embedded: `-[1]-Status-----`. This is useful for collapsed
    /// panels in a dynamic layout.
    pub fn compact(mut self, compact: bool) -> Self {
        self.props.compact = compact;
        self
    }

    /// Allow the frame to collapse when space is constrained.
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.props.collapsible = collapsible;
        self
    }

    /// Set labels rendered in the bottom border.
    pub fn footer(mut self, footer: BorderLabels) -> Self {
        self.props.footer = Box::new(footer);
        self
    }

    /// Set the left footer label.
    pub fn footer_left(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.footer.left = Some(label.into());
        self
    }

    /// Set the centered footer label.
    pub fn footer_center(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.footer.center = Some(label.into());
        self
    }

    /// Set the right footer label.
    pub fn footer_right(mut self, label: impl Into<FrameLabel>) -> Self {
        self.props.footer.right = Some(label.into());
        self
    }

    /// Set the header group style.
    pub fn header_style(mut self, style: Style) -> Self {
        self.props.header.style = style;
        self
    }

    /// Set the focused header group style.
    pub fn focused_header_style(mut self, style: Style) -> Self {
        self.props.header.focused_style = Some(style);
        self
    }

    /// Set the footer group style.
    pub fn footer_style(mut self, style: Style) -> Self {
        self.props.footer.style = style;
        self
    }

    /// Set the focused footer group style.
    pub fn focused_footer_style(mut self, style: Style) -> Self {
        self.props.footer.focused_style = Some(style);
        self
    }

    /// Set header label padding.
    pub fn header_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.header.padding = padding.into();
        self
    }

    /// Set footer label padding.
    pub fn footer_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.footer.padding = padding.into();
        self
    }
    /// Make the frame focusable even if it has no child or tabs.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.props.focusable = focusable;
        self
    }

    /// Set focus traversal behavior for this subtree.
    pub fn focus_scope(mut self, scope: crate::widgets::FocusScope) -> Self {
        self.props.focus_scope = scope;
        self
    }

    /// Set alignment of child content within the frame's inner area.
    pub fn child_align(mut self, align: Align) -> Self {
        self.props.child_align = align;
        self
    }
}

impl From<Frame> for Element {
    fn from(mut value: Frame) -> Self {
        if value.header.is_some() {
            value.props.has_header = true;
        }

        // For Flex frames, keep minimum width close to zero so sibling
        // frames with equal flex factors can share width evenly regardless
        // of border/title chrome differences.
        let is_flex_h = matches!(value.props.height, Length::Flex(_) | Length::Percent(_));
        let is_flex_w = matches!(value.props.width, Length::Flex(_));
        let is_auto_h = matches!(value.props.height, Length::Auto);
        let auto_h_depends_on_width = is_auto_h
            && (value
                .child
                .as_deref()
                .is_some_and(crate::widgets::scroll_child_height_depends_on_width)
                || value
                    .header
                    .as_deref()
                    .is_some_and(crate::widgets::scroll_child_height_depends_on_width));

        let geometry = measure_frame(&value, None, None);
        let (_, chrome_h) = measure_frame_chrome(&value);

        let min_w = if is_flex_w {
            value
                .props
                .decoration_outside_padding()
                .horizontal()
                .saturating_add(value.props.decoration_border_content_inset().horizontal())
        } else {
            geometry.outer_size().0
        };
        let min_h = if is_flex_h || auto_h_depends_on_width {
            chrome_h
        } else {
            geometry.outer_size().1
        };

        let mut layout = LayoutConstraints::default()
            .min_width(Length::Px(min_w))
            .min_height(Length::Px(min_h));

        let has_border = value.props.border;
        if has_border && value.props.collapsible {
            layout.collapse_h = Some(3);
        }
        if let Some(min_h) = value.props.focus_min_height {
            layout.focus_min_h = if let Length::Px(px) = layout.min_h {
                px.max(min_h)
            } else {
                min_h
            };
        }
        if value.props.compact {
            layout.force_compact = true;
            layout.collapse_h = Some(1);
        }
        Element::new(ElementKind::Frame(value)).with_layout(layout)
    }
}

impl crate::layout::hash::LayoutHash for Frame {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.props.width.hash(hasher);
        self.props.height.hash(hasher);
        self.props.unfocused_height.hash(hasher);
        self.props.focus_min_height.hash(hasher);
        self.props.border.hash(hasher);
        self.props.border_style.hash(hasher);
        self.props.border_edges.hash(hasher);
        self.props.border_merge_mode.hash(hasher);
        self.props.join_frame.hash(hasher);
        self.props.padding.hash(hasher);
        self.props.compact.hash(hasher);
        self.props.collapsible.hash(hasher);
        self.props.child_align.hash(hasher);
        self.props.decorations.len().hash(hasher);
        for decoration in &self.props.decorations {
            decoration.edge.hash(hasher);
            decoration.placement.hash(hasher);
            decoration.thickness.hash(hasher);
            decoration.glyph.hash(hasher);
            decoration.cap_start.hash(hasher);
            decoration.cap_end.hash(hasher);
        }
        hash_border_labels(&self.props.header, hasher);
        hash_border_labels(&self.props.footer, hasher);
        self.props.tab_titles.len().hash(hasher);
        for tab in &self.props.tab_titles {
            crate::layout::hash::hash_spans_content(&tab.spans, hasher);
        }
        self.props.active_tab.hash(hasher);
        self.props.tab_variant.hash(hasher);
        self.props.tab_edge.hash(hasher);
        self.header.is_some().hash(hasher);
        if let Some(header) = self.header.as_deref() {
            recurse(header)?.hash(hasher);
        } else {
            0u8.hash(hasher);
        }
        if let Some(child) = self.child.as_deref() {
            recurse(child)?.hash(hasher);
        } else {
            0u8.hash(hasher);
        }
        Some(())
    }
}

fn hash_optional_rich_text_content(text: Option<&RichText>, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    text.is_some().hash(hasher);
    if let Some(text) = text {
        crate::layout::hash::hash_spans_content(&text.spans, hasher);
    }
}

fn hash_border_labels(labels: &BorderLabels, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    for label in [&labels.left, &labels.center, &labels.right]
        .into_iter()
        .flatten()
    {
        hash_optional_rich_text_content(Some(&label.content), hasher);
    }
    labels.padding.hash(hasher);
}

#[cfg(test)]
mod tests {
    use super::{BorderLabels, Frame, FrameLabel};
    use crate::style::{Color, Padding, Style};

    #[test]
    fn grouped_frame_builder_stores_all_positions_and_styles() {
        let header_style = Style::new().fg(Color::Cyan);
        let focused_header_style = Style::new().bold();
        let left_style = Style::new().fg(Color::Yellow);
        let focused_left_style = Style::new().underline();
        let frame = Frame::new().header(
            BorderLabels::new()
                .left(
                    FrameLabel::new("left")
                        .style(left_style)
                        .focused_style(focused_left_style),
                )
                .center("center")
                .right("right")
                .style(header_style)
                .focused_style(focused_header_style)
                .padding(1),
        );

        let header = &frame.props.header;
        assert_eq!(
            header.left.as_ref().and_then(|label| label.style),
            Some(left_style)
        );
        assert_eq!(
            header.left.as_ref().and_then(|label| label.focused_style),
            Some(focused_left_style)
        );
        assert_eq!(
            header
                .center
                .as_ref()
                .map(|label| label.content.plain_content()),
            Some("center".into())
        );
        assert_eq!(
            header
                .right
                .as_ref()
                .map(|label| label.content.plain_content()),
            Some("right".into())
        );
        assert_eq!(header.style, header_style);
        assert_eq!(header.focused_style, Some(focused_header_style));
        assert_eq!(header.padding, Padding::from(1));
    }

    #[test]
    fn repeated_group_setters_replace_the_previous_group() {
        let frame = Frame::new()
            .header(BorderLabels::new().left("old").right("removed"))
            .footer(BorderLabels::new().center("old footer"))
            .header(BorderLabels::new().center("new"))
            .footer(BorderLabels::new().right("new footer"));

        assert!(frame.props.header.left.is_none());
        assert_eq!(
            frame
                .props
                .header
                .center
                .as_ref()
                .map(|label| label.content.plain_content()),
            Some("new".into())
        );
        assert!(frame.props.footer.center.is_none());
        assert_eq!(
            frame
                .props
                .footer
                .right
                .as_ref()
                .map(|label| label.content.plain_content()),
            Some("new footer".into())
        );
    }

    #[test]
    fn label_width_uses_display_width_and_padding() {
        let labels = BorderLabels::new().left("界").padding(1);

        assert_eq!(labels.min_width(), 4);
    }
}
