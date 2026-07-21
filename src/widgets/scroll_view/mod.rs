//! Scroll view widget.

pub mod layout;
pub mod node;
pub mod reconcile;
pub(crate) mod utils;

pub(crate) use self::layout::measure_scroll_view;
pub(crate) use self::reconcile::{ScrollViewReconcile, reconcile_scroll_view};

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind, Key};
use crate::style::{Align, BorderStyle, Length, Padding, ScrollbarConfig, Style};
use crate::widgets::internal::StackProps;

pub(crate) use self::node::RememberedScrollAnchor;
pub use self::node::ScrollViewNode;

pub use crate::widgets::scroll::{
    ScrollAxis, ScrollBehavior, ScrollChildExitDirection, ScrollChildVisibility, ScrollClip,
    ScrollDistanceConfig, ScrollEvent, ScrollExitedChild, ScrollKeymap, ScrollMetrics,
    ScrollRequest, ScrollTarget, ScrollViewportEvent, ScrollVisibleChild, ScrollWheelBehavior,
    ScrollWheelConfig,
};

/// A scrollable vertical container.
#[derive(Clone)]
pub struct ScrollView {
    /// Layout properties.
    pub(crate) props: StackProps,
    /// Row index scrolled from top (0 = at top).
    pub(crate) offset: Option<usize>,
    /// Horizontal content range to reveal with the smallest necessary scroll.
    pub(crate) horizontal_reveal_range: Option<(usize, usize)>,
    /// One-shot scroll request applied relative to the current viewport.
    pub(crate) scroll_request: Option<ScrollRequest>,
    /// Framework-owned semantic scroll target.
    pub(crate) scroll_target: Option<ScrollTarget>,
    /// How semantic scroll targets are applied.
    pub(crate) scroll_behavior: ScrollBehavior,
    /// Key bindings to move the viewport.
    pub(crate) scroll_keys: ScrollKeymap,
    /// Enable mouse wheel scrolling.
    pub(crate) scroll_wheel: bool,
    /// Widget-local mouse wheel step multiplier, overriding the app default when set.
    pub(crate) scroll_wheel_multiplier: Option<u16>,
    /// Widget-local horizontal (Shift+wheel) step multiplier. Falls back to
    /// `scroll_wheel_multiplier`, then the app default, when unset.
    pub(crate) h_scroll_wheel_multiplier: Option<u16>,
    /// How mouse wheel deltas are applied.
    pub(crate) scroll_wheel_behavior: ScrollWheelBehavior,
    /// Allow PageUp/PageDown to target this view as an ambient fallback.
    pub(crate) ambient_page_scroll: bool,
    /// Whether the scroll view can receive focus.
    pub(crate) focusable: bool,
    /// Callback fired when the scroll offset changes.
    pub(crate) on_scroll: Option<Callback<ScrollEvent>>,
    /// Callback fired when the scrollbar is dragged/clicked.
    pub(crate) on_scroll_to: Option<Callback<usize>>,
    /// Callback fired when visible children or viewport metadata changes.
    pub(crate) on_viewport_change: Option<Callback<ScrollViewportEvent>>,
    /// Draw a vertical scrollbar when content overflows.
    pub(crate) scrollbar: bool,
    /// Scrollbar configuration.
    pub(crate) scrollbar_config: ScrollbarConfig,
    /// Show scroll indicators when content is clipped.
    pub(crate) show_scroll_indicators: bool,
    pub(crate) scroll_indicator_style: Style,
    pub(crate) clip_mode: ScrollClip,
    /// Hint for initial estimated height of unmeasured off-screen children.
    /// Only used as the cold-start fallback before a running average of
    /// measured children is available.
    pub(crate) estimated_child_height: u16,
    /// Stable key used for persisting scroll anchor state across remounts.
    /// When set, this key is used instead of the element key for storing
    /// and restoring remembered scroll anchors and bottom-pinning state.
    /// Useful when the element key must change (e.g. to force cache rebuild)
    /// but scroll position should be preserved.
    pub(crate) scroll_state_key: Option<Key>,
    /// Scroll axes enabled for this view.
    pub(crate) axis: ScrollAxis,
    /// Draw a horizontal scrollbar when content overflows horizontally.
    pub(crate) h_scrollbar: bool,
    /// Horizontal scrollbar configuration.
    pub(crate) h_scrollbar_config: ScrollbarConfig,
    /// Children.
    pub(crate) children: Vec<Element>,
}

impl Default for ScrollView {
    fn default() -> Self {
        Self {
            props: StackProps::default(),
            offset: None,
            horizontal_reveal_range: None,
            scroll_request: None,
            scroll_target: None,
            scroll_behavior: ScrollBehavior::default(),
            scroll_keys: ScrollKeymap::default(),
            scroll_wheel: true,
            scroll_wheel_multiplier: None,
            h_scroll_wheel_multiplier: None,
            scroll_wheel_behavior: ScrollWheelBehavior::default(),
            ambient_page_scroll: false,
            focusable: false,
            on_scroll: None,
            on_scroll_to: None,
            on_viewport_change: None,
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
            show_scroll_indicators: false,
            scroll_indicator_style: Style::default(),
            clip_mode: ScrollClip::default(),
            estimated_child_height: 3,
            scroll_state_key: None,
            axis: ScrollAxis::default(),
            h_scrollbar: false,
            h_scrollbar_config: ScrollbarConfig::default(),
            children: Vec::new(),
        }
    }
}

impl ScrollView {
    /// Create an empty scroll view.
    pub fn new() -> Self {
        Self::default()
    }

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

    /// Set requested width (cross-axis for a vertical `ScrollView`).
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set requested height (main-axis for a vertical `ScrollView`).
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Set cross-axis alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.props.align = align;
        self
    }

    /// Set scroll offset (row index from top).
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Reveal a horizontal content range with the smallest necessary scroll.
    ///
    /// The request is reapplied when the range or viewport width changes. User scrolling remains
    /// authoritative while both stay unchanged.
    pub fn reveal_horizontal_range(mut self, start: usize, end: usize) -> Self {
        self.horizontal_reveal_range = Some((start.min(end), start.max(end)));
        self
    }

    /// Apply a one-shot scroll request relative to the current viewport.
    ///
    /// This is useful for command-driven navigation such as page up/down or
    /// jump-to-top/bottom without continuously controlling the settled offset.
    /// When set, it takes priority over `.offset(...)` but not over
    /// `.scroll_to_key(...)`.
    pub fn scroll_request(mut self, request: ScrollRequest) -> Self {
        self.scroll_request = Some(request);
        self
    }

    /// Scroll to a semantic target.
    ///
    /// Edge targets (`Top` / `Bottom`) resolve against the current content
    /// extent and do not require sentinel children. Key targets preserve the
    /// same behavior as [`Self::scroll_to_key`]. Target navigation uses
    /// [`Self::scroll_behavior`].
    pub fn scroll_to(mut self, target: ScrollTarget) -> Self {
        self.scroll_target = Some(target);
        self
    }

    /// Scroll to the start of the content.
    pub fn scroll_to_top(self) -> Self {
        self.scroll_to(ScrollTarget::Top)
    }

    /// Scroll to the end of the content.
    pub fn scroll_to_bottom(self) -> Self {
        self.scroll_to(ScrollTarget::Bottom)
    }

    /// Scroll so the first child subtree containing `key` is brought into view.
    ///
    /// This is useful for jump-to-result flows, such as scrolling a message list
    /// to a matched entry after search. When set, it takes priority over
    /// `.offset(...)`.
    pub fn scroll_to_key(mut self, key: impl Into<Key>) -> Self {
        self.scroll_target = Some(ScrollTarget::Key(key.into()));
        self
    }

    /// Scroll to `offset` rows below the first child subtree containing `key`.
    ///
    /// This is useful when a keyed row contains a large auto-height child and
    /// navigation needs to land inside that row, for example one auto-height
    /// `DiffView` per file with global hunk navigation.
    pub fn scroll_to_key_offset(mut self, key: impl Into<Key>, offset: usize) -> Self {
        self.scroll_target = Some(ScrollTarget::key_offset(key, offset));
        self
    }

    /// Configure how semantic target navigation is applied.
    ///
    /// This affects only framework-owned targets from `.scroll_to(...)`,
    /// `.scroll_to_key(...)`, `.scroll_to_top()`, and `.scroll_to_bottom()`;
    /// requests, controlled offsets, and user input remain immediate.
    pub fn scroll_behavior(mut self, behavior: ScrollBehavior) -> Self {
        self.scroll_behavior = behavior;
        self
    }

    /// Animate semantic target navigation with `config`.
    pub fn scroll_transition(mut self, config: crate::animation::TransitionConfig) -> Self {
        self.scroll_behavior = ScrollBehavior::smooth(config);
        self
    }

    /// Set a stable key for persisting scroll anchor state across remounts.
    ///
    /// When the element key must change (e.g. to force a layout cache rebuild
    /// after toggling child visibility), the scroll position is normally lost
    /// because the remembered anchor is stored under the old key. Setting a
    /// stable `scroll_state_key` ensures the anchor survives key changes.
    pub fn scroll_state_key(mut self, key: impl Into<Key>) -> Self {
        self.scroll_state_key = Some(key.into());
        self
    }

    /// Configure which keys move the viewport.
    pub fn scroll_keys(mut self, keys: ScrollKeymap) -> Self {
        self.scroll_keys = keys;
        if keys != ScrollKeymap::NONE {
            self.focusable = true;
        }
        self
    }

    /// Enable mouse wheel scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.scroll_wheel = enabled;
        self
    }

    /// Override the app-wide mouse wheel step multiplier for this scroll view.
    pub fn scroll_wheel_multiplier(mut self, multiplier: u16) -> Self {
        self.scroll_wheel_multiplier = Some(multiplier.max(1));
        self
    }

    /// Override the step multiplier for *horizontal* wheel panning (Shift+wheel).
    ///
    /// Horizontal scrolling moves in columns, which are finer-grained than the
    /// rows used for vertical scrolling, so a larger horizontal step usually
    /// feels better for wide content. Falls back to
    /// [`Self::scroll_wheel_multiplier`], then the app-wide multiplier.
    pub fn h_scroll_wheel_multiplier(mut self, multiplier: u16) -> Self {
        self.h_scroll_wheel_multiplier = Some(multiplier.max(1));
        self
    }

    /// Configure how mouse wheel input is applied.
    pub fn scroll_wheel_behavior(mut self, behavior: ScrollWheelBehavior) -> Self {
        self.scroll_wheel_behavior = behavior;
        self
    }

    /// Enable or disable smooth inertial wheel scrolling with default physics.
    pub fn smooth_wheel_scroll(mut self, enabled: bool) -> Self {
        self.scroll_wheel_behavior = if enabled {
            ScrollWheelBehavior::smooth_default()
        } else {
            ScrollWheelBehavior::Immediate
        };
        self
    }

    /// Enable smooth wheel scrolling and set the wheel acceleration impulse.
    pub fn scroll_acceleration(mut self, acceleration: f32) -> Self {
        let mut config = match self.scroll_wheel_behavior {
            ScrollWheelBehavior::Immediate => ScrollWheelConfig::default(),
            ScrollWheelBehavior::Smooth(config) => config,
        };
        config.acceleration = acceleration;
        self.scroll_wheel_behavior = ScrollWheelBehavior::Smooth(config);
        self
    }

    /// Allow PageUp/PageDown to target this scroll view even when it is not focused.
    ///
    /// This is an explicit fallback used only when normal focused-widget dispatch
    /// and component `on_key` bubbling do not handle the page key.
    pub fn ambient_page_scroll(mut self, enabled: bool) -> Self {
        self.ambient_page_scroll = enabled;
        self
    }

    /// Allow the scroll view to receive focus.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Callback fired on mouse wheel scrolling.
    pub fn on_scroll(mut self, cb: Callback<ScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Callback fired on scrollbar interaction (drag/click).
    pub fn on_scroll_to(mut self, cb: Callback<usize>) -> Self {
        self.on_scroll_to = Some(cb);
        self
    }

    /// Callback fired when visible children or viewport metadata changes.
    pub fn on_viewport_change(mut self, cb: Callback<ScrollViewportEvent>) -> Self {
        self.on_viewport_change = Some(cb);
        self
    }

    /// Draw a vertical scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }

    /// Enable "N more" scroll indicators when items are hidden.
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.show_scroll_indicators = show;
        self
    }

    /// Set style for scroll indicators.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.scroll_indicator_style = style;
        self
    }

    /// Control how children are clipped against the viewport.
    pub fn clip_mode(mut self, clip_mode: ScrollClip) -> Self {
        self.clip_mode = clip_mode;
        self
    }

    /// Hint for the initial estimated height of unmeasured off-screen children.
    ///
    /// Only used as the cold-start fallback before a running average of
    /// measured children is available. Default: `3`.
    pub fn estimated_child_height(mut self, height: u16) -> Self {
        self.estimated_child_height = height;
        self
    }

    /// Configure which scroll axes are active.
    ///
    /// Default is [`ScrollAxis::Vertical`] (historical behavior). Use
    /// [`ScrollAxis::Both`] to enable horizontal panning for content wider than
    /// the viewport.
    pub fn axis(mut self, axis: ScrollAxis) -> Self {
        self.axis = axis;
        self
    }

    /// Draw a horizontal scrollbar when content overflows horizontally.
    ///
    /// Only effective when the axis includes horizontal scrolling.
    pub fn h_scrollbar(mut self, h_scrollbar: bool) -> Self {
        self.h_scrollbar = h_scrollbar;
        self
    }

    /// Set horizontal scrollbar configuration.
    pub fn h_scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.h_scrollbar_config = config;
        self
    }
}

impl From<ScrollView> for Element {
    fn from(value: ScrollView) -> Self {
        Element::new(ElementKind::ScrollView(Box::new(value)))
    }
}

impl crate::layout::hash::LayoutHash for ScrollView {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;

        crate::layout::hash::hash_stack_props(&self.props, hasher);
        self.scrollbar.hash(hasher);
        self.scrollbar_config.variant.hash(hasher);
        self.scrollbar_config.gap.hash(hasher);
        self.show_scroll_indicators.hash(hasher);
        self.axis.hash(hasher);
        self.h_scrollbar.hash(hasher);
        self.h_scrollbar_config.variant.hash(hasher);
        self.h_scrollbar_config.gap.hash(hasher);
        crate::layout::hash::hash_children(&self.children, hasher, recurse)
    }
}
