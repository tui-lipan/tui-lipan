mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::{measure_flow, pack_rows};
pub use self::node::FlowNode;
pub(crate) use self::reconcile::reconcile_flow;

use crate::core::element::{Element, ElementKind};
use crate::style::{
    Align, BorderStyle, Justify, LayoutConstraints, Length, Padding, ShrinkPriority, Style,
};

/// A horizontal wrapping container.
///
/// Packs children left-to-right, starting a new row whenever the next child
/// would exceed the available width. Supports `gap`, `align` (cross-axis),
/// `justify` (main-axis, applied per wrapped row), `padding`, and `border` -
/// the same chrome primitives as [`crate::prelude::HStack`] /
/// [`crate::prelude::VStack`].
#[derive(Clone)]
pub struct Flow {
    pub(crate) children: Vec<Element>,
    pub(crate) gap: u16,
    /// Vertical gap between wrapped rows. When `None`, falls back to `gap` so
    /// the spacing is symmetric by default.
    pub(crate) row_gap: Option<u16>,
    pub(crate) align: Align,
    pub(crate) justify: Justify,
    pub(crate) padding: Padding,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) style: Style,
    pub(crate) width: Length,
    pub(crate) height: Length,
    /// When `true`, a parent stack may shrink this Flow below its widest child
    /// (its items then clip/ellipsize) so rigid siblings keep their width. When
    /// `false` (default) the Flow reserves at least its widest child as a
    /// main-axis floor and wraps onto more rows instead of truncating.
    pub(crate) shrinkable: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            gap: 0,
            row_gap: None,
            align: Align::Start,
            justify: Justify::Start,
            padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            style: Style::default(),
            width: Length::Flex(1),
            height: Length::Auto,
            shrinkable: false,
        }
    }
}

impl Flow {
    /// Create an empty Flow.
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

    /// Set gap between items and rows.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set the vertical gap between wrapped rows, independent of the horizontal
    /// item `gap`. Useful for hint/footer rows that want spacing between items
    /// but tightly stacked rows when they wrap.
    pub fn row_gap(mut self, row_gap: u16) -> Self {
        self.row_gap = Some(row_gap);
        self
    }

    /// Allow a parent stack to shrink this Flow below its widest child, clipping
    /// or ellipsizing items, so rigid siblings keep their width. By default a
    /// Flow reserves its widest child and wraps onto more rows instead. Use this
    /// for the lower-priority group in a competing row (e.g. hints beside
    /// must-stay-readable action buttons).
    pub fn shrinkable(mut self, shrinkable: bool) -> Self {
        self.shrinkable = shrinkable;
        self
    }

    /// Set cross-axis alignment within each row.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Set main-axis distribution of items within each wrapped row.
    ///
    /// Applied per row: each row distributes its own leftover width, so
    /// `SpaceBetween` pushes the first item of every row to the left edge and
    /// the last to the right edge. Unlike stacks, Flow children are always
    /// measured at their natural size, so the space variants work without any
    /// explicit child sizing.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Set padding around the inner content area.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Draw a border around the container.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Override requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Override requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<Flow> for Element {
    fn from(value: Flow) -> Self {
        let shrink_priority = if value.shrinkable {
            ShrinkPriority::First
        } else {
            ShrinkPriority::Normal
        };
        Element::new(ElementKind::Flow(value)).with_layout(
            LayoutConstraints::default()
                .reflows(true)
                .shrink_priority(shrink_priority),
        )
    }
}

impl crate::layout::hash::LayoutHash for Flow {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;

        self.gap.hash(hasher);
        self.row_gap.hash(hasher);
        self.align.hash(hasher);
        self.justify.hash(hasher);
        self.padding.hash(hasher);
        self.border.hash(hasher);
        self.border_style.hash(hasher);
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.shrinkable.hash(hasher);

        let non_portal_count = self
            .children
            .iter()
            .filter(|child| !matches!(child.kind, ElementKind::Portal(_)))
            .count();
        non_portal_count.hash(hasher);
        for child in &self.children {
            if matches!(child.kind, ElementKind::Portal(_)) {
                continue;
            }
            recurse(child)?.hash(hasher);
        }

        Some(())
    }
}
