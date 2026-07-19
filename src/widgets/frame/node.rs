use crate::callback::Callback;
use crate::core::node::WidgetNode;
use crate::style::{
    Align, BorderEdges, BorderStyle, Edge, Length, Padding, Rect, RichText, Style, StyleSlot,
    Theme, ThemeRole,
};
use crate::widgets::frame::{DecorationPlacement, EdgeDecoration};
use crate::widgets::{BorderMergeMode, FocusScope, TabVariant, TabsEvent};

/// Rarely-used focus/hover style overrides for [`FrameNode`].
///
/// Boxed behind `Option<Box<…>>` to avoid paying ~500+ bytes per frame when
/// no style overrides are configured (the common case).
#[derive(Clone, Default)]
pub struct FrameStyleOverrides {
    /// Style for the inner content area (distinct from border).
    pub inner_style: Option<Style>,
    /// Style applied to the title when focused.
    pub focus_title_style: Option<Style>,
    /// Style applied when the frame or its children have focus.
    pub focus_style: Option<StyleSlot>,
    /// Style applied when the frame is hovered.
    pub hover_style: Option<StyleSlot>,
    /// Border style applied when focused.
    pub focus_border_style: Option<BorderStyle>,
    /// Style applied to the status line when focused.
    pub focus_status_style: Option<Style>,
    /// Style applied to the active tab when the frame is focused.
    pub focus_active_tab_style: Option<Style>,
    /// Style applied to inactive tabs when the frame is focused.
    pub focus_inactive_tab_style: Option<Style>,
}

/// Runtime state for a frame node.
#[derive(Clone)]
pub struct FrameNode {
    /// Optional title (used when no border tabs are set).
    pub title: Option<RichText>,
    /// Optional title prefix (rendered before title or tabs).
    pub title_prefix: Option<RichText>,
    /// Optional title suffix (rendered after title or tabs).
    pub title_suffix: Option<RichText>,
    /// Title alignment in the top border.
    /// Default: `Align::Start`.
    pub title_alignment: Align,
    /// Tab titles rendered in the top border.
    pub tab_titles: Vec<RichText>,
    /// Active tab index for border tabs.
    pub active_tab: usize,
    /// Active tab style.
    pub active_tab_style: Style,
    /// Inactive tab style.
    pub inactive_tab_style: Style,
    /// Border tab variant.
    pub tab_variant: TabVariant,
    /// Callback fired when a border tab is clicked.
    pub on_tab_change: Option<Callback<TabsEvent>>,
    /// Optional status line (left) shown at the bottom of the inner area.
    pub status: Option<RichText>,
    /// Optional centered status segment.
    pub status_center: Option<RichText>,
    /// Optional right-aligned status segment.
    pub status_right: Option<RichText>,
    /// Padding inside the border.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Edge decorations.
    pub decorations: Vec<EdgeDecoration>,
    /// Whether to render the border.
    pub border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Border edge geometry.
    /// Default: `BorderEdges::All`.
    pub border_edges: BorderEdges,
    /// Merge strategy for overlapping border symbols.
    pub border_merge_mode: BorderMergeMode,
    /// Join borders with neighboring frames when edges touch.
    pub join_frame: bool,
    /// Base style (border + background).
    pub style: Style,
    /// Title style.
    pub title_style: Style,
    /// Status line style.
    pub status_style: Style,
    /// Optional width override.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Optional height override.
    /// Default: `Length::Flex(1)`.
    pub height: Length,
    /// Optional height override when not focused.
    pub unfocused_height: Option<Length>,
    /// Minimum height when focused (includes borders).
    pub focus_min_height: Option<u16>,
    /// Compact single-line rendering mode.
    pub compact: bool,
    /// Allow the frame to collapse when space is constrained.
    pub collapsible: bool,
    /// Header (title/tabs) padding (top/bottom ignored).
    /// Default: `Padding::default()`.
    pub header_padding: Padding,
    /// Footer (status) padding (top/bottom ignored).
    /// Default: `Padding::default()`.
    pub footer_padding: Padding,
    pub has_header: bool,
    /// Explicitly control focusability.
    pub focusable: bool,
    /// Focus traversal behavior for this subtree.
    pub focus_scope: FocusScope,
    /// Alignment of child content within the frame's inner area.
    /// Default: `Align::Start`.
    pub child_align: Align,
    /// Boxed focus/hover style overrides (None when no overrides are set).
    pub style_overrides: Option<Box<FrameStyleOverrides>>,
}

impl FrameNode {
    pub fn has_border(&self) -> bool {
        self.border
    }

    /// Padding consumed by the currently rendered border edges.
    pub fn border_padding(&self) -> Padding {
        if self.has_border() {
            self.border_edges.padding()
        } else {
            Padding::default()
        }
    }

    /// Access the inner content style override.
    pub fn inner_style(&self) -> Option<Style> {
        self.style_overrides.as_ref()?.inner_style
    }

    /// Access the focused title style override.
    pub fn focus_title_style(&self) -> Option<Style> {
        self.style_overrides.as_ref()?.focus_title_style
    }

    /// Access the focused frame style override.
    pub fn focus_style(&self) -> Option<Style> {
        match self.style_overrides.as_ref()?.focus_style? {
            StyleSlot::Inherit => Some(Style::default()),
            StyleSlot::Extend(style) | StyleSlot::Replace(style) => Some(style),
        }
    }

    /// Access the hover style override.
    pub fn hover_style(&self) -> Option<Style> {
        match self.style_overrides.as_ref()?.hover_style? {
            StyleSlot::Inherit => Some(Style::default()),
            StyleSlot::Extend(style) | StyleSlot::Replace(style) => Some(style),
        }
    }

    /// Access the focused border style override.
    pub fn focus_border_style(&self) -> Option<BorderStyle> {
        self.style_overrides.as_ref()?.focus_border_style
    }

    /// Access the focused status style override.
    pub fn focus_status_style(&self) -> Option<Style> {
        self.style_overrides.as_ref()?.focus_status_style
    }

    /// Access the focused active tab style override.
    pub fn focus_active_tab_style(&self) -> Option<Style> {
        self.style_overrides.as_ref()?.focus_active_tab_style
    }

    /// Access the focused inactive tab style override.
    pub fn focus_inactive_tab_style(&self) -> Option<Style> {
        self.style_overrides.as_ref()?.focus_inactive_tab_style
    }

    fn has_focus_chrome(&self) -> bool {
        self.focus_min_height.is_some()
            || self.style_overrides.as_ref().is_some_and(|overrides| {
                overrides
                    .focus_style
                    .is_some_and(|s| !matches!(s, StyleSlot::Inherit))
                    || overrides.focus_border_style.is_some()
                    || overrides.focus_title_style.is_some_and(|s| !s.is_empty())
                    || overrides.focus_status_style.is_some_and(|s| !s.is_empty())
                    || overrides
                        .focus_active_tab_style
                        .is_some_and(|s| !s.is_empty())
                    || overrides
                        .focus_inactive_tab_style
                        .is_some_and(|s| !s.is_empty())
            })
    }

    /// Get or initialize the style overrides box.
    pub fn overrides_mut(&mut self) -> &mut FrameStyleOverrides {
        self.style_overrides.get_or_insert_with(Default::default)
    }

    pub fn decoration_outside_padding(&self) -> Padding {
        let mut pad = Padding::default();
        for decoration in &self.decorations {
            if decoration.placement != crate::widgets::frame::DecorationPlacement::Outside {
                continue;
            }
            match decoration.edge {
                Edge::Left => pad.left = pad.left.saturating_add(decoration.thickness),
                Edge::Right => pad.right = pad.right.saturating_add(decoration.thickness),
                Edge::Top => pad.top = pad.top.saturating_add(decoration.thickness),
                Edge::Bottom => pad.bottom = pad.bottom.saturating_add(decoration.thickness),
            }
        }
        pad
    }

    /// Space to reserve inside the frame so [`DecorationPlacement::Border`] bands are not
    /// covered by child widgets when the frame has no drawn border.
    ///
    /// With `border: true`, the content rect is already inset like a border; Border-placement
    /// decorations align with that edge. With `border: false`, the same decorations still
    /// occupy cells on `body_rect` and must shrink the laid-out content area to match paint.
    pub fn decoration_border_content_inset(&self) -> Padding {
        if self.has_border() {
            return Padding::default();
        }
        let mut pad = Padding::default();
        for decoration in &self.decorations {
            if decoration.placement != DecorationPlacement::Border {
                continue;
            }
            let t = decoration.thickness;
            match decoration.edge {
                Edge::Left => pad.left = pad.left.saturating_add(t),
                Edge::Right => pad.right = pad.right.saturating_add(t),
                Edge::Top => pad.top = pad.top.saturating_add(t),
                Edge::Bottom => pad.bottom = pad.bottom.saturating_add(t),
            }
        }
        pad
    }
}

impl WidgetNode for FrameNode {
    fn focus_scope(&self) -> FocusScope {
        self.focus_scope
    }

    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn has_on_click(&self) -> bool {
        self.on_tab_change.is_some()
    }

    fn is_hoverable(&self) -> bool {
        if self.has_on_click() {
            return true;
        }
        self.hover_style().is_some_and(|s| !s.is_empty())
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        if self.has_on_click() {
            return true;
        }
        self.style_overrides
            .as_ref()
            .and_then(|overrides| overrides.hover_style.as_ref())
            .is_some_and(|slot| {
                matches!(slot, StyleSlot::Inherit)
                    || slot.resolves_non_empty(theme, ThemeRole::Hover)
            })
    }

    fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        let on_left = self.border_edges.has_left() && x == rect.x;
        let on_right = self.border_edges.has_right()
            && x == rect.x.saturating_add(rect.w as i16).saturating_sub(1);
        let on_top = self.border_edges.has_top() && y == rect.y;
        let on_bottom = self.border_edges.has_bottom()
            && y == rect.y.saturating_add(rect.h as i16).saturating_sub(1);
        let on_border = self.has_border()
            && rect.w > 0
            && rect.h > 0
            && (on_left || on_right || on_top || on_bottom);

        if !self.focusable && self.on_tab_change.is_some() {
            // Tabs are always on the top row (y == rect.y)
            if y != rect.y {
                return Some(on_border && self.has_focus_chrome());
            }
        }

        if !self.focusable && on_border && self.has_focus_chrome() {
            return Some(true);
        }

        None
    }
}

impl Default for FrameNode {
    fn default() -> Self {
        Self {
            title: None,
            title_prefix: None,
            title_suffix: None,
            title_alignment: Align::Start,
            tab_titles: Vec::new(),
            active_tab: 0,
            active_tab_style: Style::default(),
            inactive_tab_style: Style::default(),
            tab_variant: TabVariant::default(),
            on_tab_change: None,
            status: None,
            status_center: None,
            status_right: None,
            padding: Padding::default(),
            decorations: Vec::new(),
            border: true,
            border_style: BorderStyle::Plain,
            border_edges: BorderEdges::All,
            border_merge_mode: BorderMergeMode::Exact,
            join_frame: false,
            style: Style::default(),
            title_style: Style::default(),
            status_style: Style::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            unfocused_height: None,
            focus_min_height: None,
            compact: false,
            collapsible: true,
            header_padding: Padding::default(),
            footer_padding: Padding::default(),
            has_header: false,
            focusable: false,
            focus_scope: FocusScope::None,
            child_align: Align::Start,
            style_overrides: None,
        }
    }
}

// Backward compatibility alias if needed, but we are refactoring.
pub type FrameProps = FrameNode;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_focusable_frame_focus_chrome_is_hit_testable_on_border() {
        let mut frame = FrameNode {
            focusable: false,
            focus_min_height: Some(4),
            ..FrameNode::default()
        };
        frame.overrides_mut().focus_style = Some(StyleSlot::Replace(
            Style::new().fg(crate::style::Color::LightCyan),
        ));

        let rect = Rect {
            x: 2,
            y: 3,
            w: 10,
            h: 4,
        };

        assert_eq!(frame.hit_test_refinement(4, 3, rect), Some(true));
        assert_eq!(frame.hit_test_refinement(2, 5, rect), Some(true));
        assert_eq!(frame.hit_test_refinement(4, 5, rect), None);
    }

    #[test]
    fn inherited_frame_hover_slot_is_hoverable_even_before_theme_resolution() {
        let mut frame = FrameNode::default();
        frame.overrides_mut().hover_style = Some(StyleSlot::Inherit);

        assert!(frame.is_hoverable_for_theme(&Theme::default()));
    }
}
