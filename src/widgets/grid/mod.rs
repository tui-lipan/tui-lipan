//! Explicit row/column grid layout (`Grid`).

mod layout;
mod node;
mod reconcile;

pub(crate) use layout::measure_grid;
pub(crate) use node::GridNode;
pub(crate) use reconcile::{GridReconcile, reconcile_grid};

use std::hash::Hash;

use crate::core::element::{Element, ElementKind};
use crate::layout::hash::LayoutHash;
use crate::style::{Align, BorderStyle, Justify, LayoutConstraints, Length, Padding, Style};

/// Track sizing and chrome configuration for a [`Grid`].
#[derive(Clone, Debug)]
pub struct GridProps {
    /// Column track sizes. An empty list lets the grid infer columns from item placement.
    pub columns: Vec<Length>,
    /// Row track sizes. An empty list lets the grid infer rows from item placement.
    pub rows: Vec<Length>,
    /// Horizontal gap between columns, in cells.
    pub gap_x: u16,
    /// Vertical gap between rows, in cells.
    pub gap_y: u16,
    /// Inner padding applied inside the grid (and inside the border, if any).
    pub padding: Padding,
    /// Base style for the grid container.
    pub style: Style,
    /// Cross-axis alignment of each item within its cell.
    pub align: Align,
    /// Single-child cell alignment on the main axis. `SpaceBetween`, `SpaceAround`, and
    /// `SpaceEvenly` match `Start` (no siblings to distribute between).
    pub justify: Justify,
    /// Width of the grid container.
    pub width: Length,
    /// Height of the grid container.
    pub height: Length,
    /// Whether to draw a border around the grid.
    pub border: bool,
    /// Border line style used when [`border`](Self::border) is enabled.
    pub border_style: BorderStyle,
}

impl Default for GridProps {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            gap_x: 0,
            gap_y: 0,
            padding: Padding::default(),
            style: Style::default(),
            align: Align::Start,
            justify: Justify::Start,
            width: Length::Flex(1),
            height: Length::Flex(1),
            border: false,
            border_style: BorderStyle::Plain,
        }
    }
}

/// A single child placed in a [`Grid`], with optional explicit cell placement and span.
#[derive(Clone)]
pub struct GridItem {
    pub(crate) element: Element,
    pub(crate) placement: Option<(u16, u16)>,
    pub(crate) span: (u16, u16),
}

/// Explicit row/column grid container.
///
/// Children are placed either in flow order ([`child`](Self::child)) or at explicit
/// cells ([`cell`](Self::cell)), and may span multiple tracks
/// ([`span`](Self::span) / [`cell_span`](Self::cell_span)).
#[derive(Clone)]
pub struct Grid {
    pub(crate) props: GridProps,
    pub(crate) items: Vec<GridItem>,
}

impl Grid {
    /// Creates an empty grid with default props (flex width/height, no tracks).
    pub fn new() -> Self {
        Self {
            props: GridProps::default(),
            items: Vec::new(),
        }
    }

    /// Sets explicit column track sizes.
    pub fn columns<I: IntoIterator<Item = Length>>(mut self, columns: I) -> Self {
        self.props.columns = columns.into_iter().collect();
        self
    }

    /// Sets explicit row track sizes.
    pub fn rows<I: IntoIterator<Item = Length>>(mut self, rows: I) -> Self {
        self.props.rows = rows.into_iter().collect();
        self
    }

    /// Sets `n` equally sized [`Auto`](Length::Auto) columns (at least one).
    pub fn uniform_columns(mut self, n: usize) -> Self {
        self.props.columns = vec![Length::Auto; n.max(1)];
        self
    }

    /// Sets both the horizontal and vertical gap between tracks.
    pub fn gap(mut self, gap: u16) -> Self {
        self.props.gap_x = gap;
        self.props.gap_y = gap;
        self
    }

    /// Sets the horizontal gap between columns.
    pub fn gap_x(mut self, gap: u16) -> Self {
        self.props.gap_x = gap;
        self
    }

    /// Sets the vertical gap between rows.
    pub fn gap_y(mut self, gap: u16) -> Self {
        self.props.gap_y = gap;
        self
    }

    /// Alias for [`gap_x`](Self::gap_x): the horizontal gap between columns.
    pub fn column_gap(mut self, gap: u16) -> Self {
        self.props.gap_x = gap;
        self
    }

    /// Alias for [`gap_y`](Self::gap_y): the vertical gap between rows.
    pub fn row_gap(mut self, gap: u16) -> Self {
        self.props.gap_y = gap;
        self
    }

    /// Appends a child in flow order (auto-placed into the next free cell).
    pub fn child(mut self, element: impl Into<Element>) -> Self {
        self.items.push(GridItem {
            element: element.into(),
            placement: None,
            span: (1, 1),
        });
        self
    }

    /// Sets the row/column span of the most recently added child (minimum 1 each).
    pub fn span(mut self, row_span: u16, col_span: u16) -> Self {
        if let Some(last) = self.items.last_mut() {
            last.span = (row_span.max(1), col_span.max(1));
        }
        self
    }

    /// Places a child at an explicit `(row, col)` cell.
    pub fn cell(mut self, row: u16, col: u16, element: impl Into<Element>) -> Self {
        self.items.push(GridItem {
            element: element.into(),
            placement: Some((row, col)),
            span: (1, 1),
        });
        self
    }

    /// Places a child at an explicit `(row, col)` cell spanning `row_span`×`col_span` tracks.
    pub fn cell_span(
        mut self,
        row: u16,
        col: u16,
        row_span: u16,
        col_span: u16,
        element: impl Into<Element>,
    ) -> Self {
        self.items.push(GridItem {
            element: element.into(),
            placement: Some((row, col)),
            span: (row_span.max(1), col_span.max(1)),
        });
        self
    }

    /// Sets the inner padding of the grid.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.padding = padding.into();
        self
    }

    /// Sets the base style of the grid container.
    pub fn style(mut self, style: Style) -> Self {
        self.props.style = style;
        self
    }

    /// Sets the cross-axis alignment of items within their cells.
    pub fn align(mut self, align: Align) -> Self {
        self.props.align = align;
        self
    }

    /// Sets the main-axis alignment of a single child within its cell.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.props.justify = justify;
        self
    }

    /// Sets the width of the grid container.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Sets the height of the grid container.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Toggles drawing a border around the grid.
    pub fn border(mut self, border: bool) -> Self {
        self.props.border = border;
        self
    }

    /// Sets the border line style (takes effect when [`border`](Self::border) is enabled).
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.border_style = border_style;
        self
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Grid> for Element {
    fn from(value: Grid) -> Self {
        let has_children = !value.items.is_empty();
        let is_flex_h = matches!(value.props.height, Length::Flex(_));
        let is_flex_w = matches!(value.props.width, Length::Flex(_));
        let chrome_h = value.props.padding.vertical() + if value.props.border { 2 } else { 0 };

        let (min_w, min_h) = match (is_flex_w, is_flex_h) {
            (true, true) => {
                let mut w = value.props.padding.horizontal();
                let mut h = chrome_h;
                if value.props.border {
                    w += 2;
                }
                if has_children {
                    w = w.max(1);
                    h = h.max(1);
                }
                (w, h)
            }
            (true, false) => {
                let (_, h) = measure_grid(&value.props, &value.items, None, None);
                let mut min_w = value.props.padding.horizontal();
                if value.props.border {
                    min_w += 2;
                }
                (min_w, h)
            }
            (false, true) => {
                let (w, _) = measure_grid(&value.props, &value.items, None, None);
                let mut chrome_h = value.props.padding.vertical();
                if value.props.border {
                    chrome_h += 2;
                }
                (w, chrome_h.max(1))
            }
            (false, false) => measure_grid(&value.props, &value.items, None, None),
        };

        let children_need_width = value
            .items
            .iter()
            .any(|i| crate::widgets::scroll_child_height_depends_on_width(&i.element));

        let min_h = if matches!(value.props.height, Length::Auto) && children_need_width {
            chrome_h
        } else {
            min_h
        };

        Element::new(ElementKind::Grid(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl LayoutHash for Grid {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        crate::layout::hash::hash_grid_props(&self.props, hasher);
        for item in &self.items {
            recurse(&item.element)?.hash(hasher);
            item.placement.hash(hasher);
            item.span.hash(hasher);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use crate::core::element::{Element, IntoElement, Key};
    use crate::core::node::{NodeId, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::layout::measure::min_size_constrained;
    use crate::style::{Align, Length, Rect};
    use crate::widgets::{Frame, Grid, Text};

    fn find_by_key(tree: &NodeTree, key: &str) -> NodeId {
        let key = Key::from(key.to_string());
        tree.iter()
            .find(|n| n.key.as_ref() == Some(&key))
            .map(|n| n.id)
            .unwrap_or(NodeId::INVALID)
    }

    #[test]
    fn grid_rows_measure_to_content_height() {
        let grid = Grid::new()
            .uniform_columns(2)
            .gap(1)
            .child(Text::new("left"))
            .child(Text::new("right"));

        let (_, h) = min_size_constrained(&grid.into(), Some(40), None);

        assert!(h >= 1, "grid height should include row content, got {h}");
    }

    #[test]
    fn grid_tracks_position_2x2_children() {
        let grid: Element = Grid::new()
            .columns([Length::Auto, Length::Auto])
            .rows([Length::Px(6), Length::Px(6)])
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .child(Frame::new().border(true).key("a"))
            .child(Frame::new().border(true).key("b"))
            .child(Frame::new().border(true).key("c"))
            .child(Frame::new().border(true).key("d"))
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &grid,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            None,
        );

        let a = tree.node(find_by_key(&tree, "a")).rect;
        let c = tree.node(find_by_key(&tree, "c")).rect;

        assert_eq!(a.y, 0);
        assert_eq!(c.y, 6);
        assert!(a.h <= 6 && c.h <= 6);
    }

    #[test]
    fn grid_spanning_cell_width_is_two_tracks_plus_gap_x() {
        let grid: Element = Grid::new()
            .columns([Length::Px(5), Length::Px(7)])
            .rows([Length::Px(10)])
            .gap_x(2)
            .align(Align::Stretch)
            .cell_span(0, 0, 1, 2, Frame::new().border(false).key("span"))
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &grid,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            None,
        );

        let span = tree.node(find_by_key(&tree, "span")).rect;
        assert_eq!(span.w, 5 + 2 + 7);
    }

    #[test]
    fn grid_auto_tracks_do_not_absorb_parent_slack() {
        let grid: Element = Grid::new()
            .uniform_columns(3)
            .rows([Length::Auto, Length::Auto, Length::Auto])
            .gap_x(2)
            .gap_y(0)
            .cell(
                0,
                0,
                Frame::new()
                    .border(true)
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .key("a"),
            )
            .cell(
                0,
                1,
                Frame::new()
                    .border(true)
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .key("b"),
            )
            .cell(
                1,
                0,
                Frame::new()
                    .border(true)
                    .width(Length::Px(5))
                    .height(Length::Px(3))
                    .key("c"),
            )
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &grid,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            None,
        );

        let a = tree.node(find_by_key(&tree, "a")).rect;
        let b = tree.node(find_by_key(&tree, "b")).rect;
        let c = tree.node(find_by_key(&tree, "c")).rect;

        assert_eq!(a.w, 5);
        assert_eq!(a.h, 3);
        assert_eq!(b.x, a.x + 5 + 2);
        assert_eq!(c.y, a.y + 3);
    }
}
