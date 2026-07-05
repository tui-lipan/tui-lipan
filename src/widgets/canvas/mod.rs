//! Absolute-positioned child container (`Canvas`).

mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_canvas;
pub(crate) use self::node::CanvasNode;
pub(crate) use self::reconcile::{CanvasReconcile, reconcile_canvas};

use crate::core::element::{Element, ElementKind};
use crate::layout::hash::LayoutHash;
use crate::style::{Length, Rect, Style};

/// A single child placed at a Canvas-local rectangle.
#[derive(Clone)]
pub struct CanvasItem {
    /// Placement rectangle in the containing [`Canvas`]'s local coordinates.
    pub rect: Rect,
    /// Child element to reconcile and render inside the placement rectangle.
    pub element: Element,
}

impl CanvasItem {
    /// Creates a Canvas item from a local rectangle and child element.
    pub fn new(rect: Rect, element: impl Into<Element>) -> Self {
        Self {
            rect,
            element: element.into(),
        }
    }
}

impl std::borrow::Borrow<Element> for CanvasItem {
    fn borrow(&self) -> &Element {
        &self.element
    }
}

/// Absolute-positioned child container.
///
/// Child rectangles are local to the Canvas allocation. During reconciliation they are
/// translated by the Canvas origin; during rendering descendants are clipped to the Canvas rect.
/// Children are painted in declaration order, so later children appear visually on top.
#[derive(Clone)]
pub struct Canvas {
    pub(crate) items: Vec<CanvasItem>,
    pub(crate) style: Style,
    pub(crate) passthrough: bool,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            style: Style::default(),
            passthrough: false,
            width: Length::Flex(1),
            height: Length::Flex(1),
        }
    }
}

impl Canvas {
    /// Create an empty Canvas that fills available width and height by default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a child at a Canvas-local rectangle.
    pub fn child_at(mut self, rect: Rect, child: impl Into<Element>) -> Self {
        self.items.push(CanvasItem::new(rect, child));
        self
    }

    /// Replace all positioned children.
    pub fn items(mut self, items: impl IntoIterator<Item = CanvasItem>) -> Self {
        self.items = items.into_iter().collect();
        self
    }

    /// Set base style for the Canvas background/effects.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Allow pointer events to pass through non-interactive top layers.
    pub fn passthrough(mut self, passthrough: bool) -> Self {
        self.passthrough = passthrough;
        self
    }

    /// Set requested Canvas width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested Canvas height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<Canvas> for Element {
    fn from(value: Canvas) -> Self {
        Element::new(ElementKind::Canvas(value))
    }
}

impl LayoutHash for Canvas {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;

        self.passthrough.hash(hasher);
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.items.len().hash(hasher);
        for item in &self.items {
            item.rect.hash(hasher);
            recurse(&item.element)?.hash(hasher);
        }
        Some(())
    }
}
