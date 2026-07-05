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
// Alias for backward compatibility if needed, though we are updating usages.
pub(crate) use self::node::FrameProps;

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

    /// Set title (ignored when border tabs are set).
    pub fn title(mut self, title: impl Into<RichText>) -> Self {
        self.props.title = Some(title.into());
        self
    }

    /// Set an optional prefix rendered before the title or tabs.
    pub fn title_prefix(mut self, prefix: impl Into<RichText>) -> Self {
        self.props.title_prefix = Some(prefix.into());
        self
    }

    /// Set an optional suffix rendered after the title or tabs.
    pub fn title_suffix(mut self, suffix: impl Into<RichText>) -> Self {
        self.props.title_suffix = Some(suffix.into());
        self
    }

    /// Set the title alignment in the top border.
    pub fn title_alignment(mut self, align: Align) -> Self {
        self.props.title_alignment = align;
        self
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

    /// Set the visual variant for border tabs.
    pub fn tab_variant(mut self, variant: TabVariant) -> Self {
        self.props.tab_variant = variant;
        self
    }

    /// Set a status line shown at the bottom of the inner area (left-aligned).
    pub fn status(mut self, status: impl Into<RichText>) -> Self {
        self.props.status = Some(status.into());
        self
    }

    /// Set a left-aligned status segment.
    pub fn status_left(self, status: impl Into<RichText>) -> Self {
        self.status(status)
    }

    /// Set a centered status segment.
    pub fn status_center(mut self, status: impl Into<RichText>) -> Self {
        self.props.status_center = Some(status.into());
        self
    }

    /// Set a right-aligned status segment.
    pub fn status_right(mut self, status: impl Into<RichText>) -> Self {
        self.props.status_right = Some(status.into());
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

    /// Set title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.props.title_style = style;
        self
    }

    /// Set style applied to the title when focused.
    pub fn focus_title_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_title_style = Some(style);
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

    /// Set status line style.
    pub fn status_style(mut self, style: Style) -> Self {
        self.props.status_style = style;
        self
    }

    /// Set style applied to the status line when focused.
    pub fn focus_status_style(mut self, style: Style) -> Self {
        self.props.overrides_mut().focus_status_style = Some(style);
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

    /// Set the frame header element.
    /// Set the header element.
    pub fn header(mut self, header: impl Into<Element>) -> Self {
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

    /// Set padding for the header (title/tabs). Top/bottom are ignored.
    pub fn header_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.header_padding = padding.into();
        self
    }

    /// Set padding for the footer (status). Top/bottom are ignored.
    pub fn footer_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.footer_padding = padding.into();
        self
    }
    /// Make the frame focusable even if it has no child or tabs.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.props.focusable = focusable;
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
        self.props.header_padding.hash(hasher);
        self.props.footer_padding.hash(hasher);
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
        hash_optional_rich_text_content(self.props.title.as_ref(), hasher);
        hash_optional_rich_text_content(self.props.title_prefix.as_ref(), hasher);
        hash_optional_rich_text_content(self.props.title_suffix.as_ref(), hasher);
        self.props.tab_titles.len().hash(hasher);
        for tab in &self.props.tab_titles {
            crate::layout::hash::hash_spans_content(&tab.spans, hasher);
        }
        self.props.active_tab.hash(hasher);
        self.props.tab_variant.hash(hasher);
        hash_optional_rich_text_content(self.props.status.as_ref(), hasher);
        hash_optional_rich_text_content(self.props.status_center.as_ref(), hasher);
        hash_optional_rich_text_content(self.props.status_right.as_ref(), hasher);
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
