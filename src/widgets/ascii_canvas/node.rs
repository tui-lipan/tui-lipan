use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Color, Length, Style};
use crate::utils::gradient::{ColorGradient, GradientDirection};

use super::animation::FrameSequence;
use super::{AsciiCanvas, AsciiCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsciiCanvasRenderKey {
    hash: u64,
}

impl AsciiCanvasRenderKey {
    fn new(canvas: &AsciiCanvas) -> Self {
        let mut hasher = DefaultHasher::new();
        canvas.lines.hash(&mut hasher);
        canvas.cells.hash(&mut hasher);
        canvas.grid_size.hash(&mut hasher);
        canvas.style.hash(&mut hasher);
        canvas.background.hash(&mut hasher);
        canvas.gradient.hash(&mut hasher);
        // Hash color_map entries if present.
        if let Some(ref map) = canvas.color_map {
            map.hash(&mut hasher);
        }
        if let Some(ref map) = canvas.fg_color_map {
            map.hash(&mut hasher);
        }
        if let Some(ref map) = canvas.bg_color_map {
            map.hash(&mut hasher);
        }
        // Hash sequence identity + current frame for multi-frame mode
        if let Some(ref seq) = canvas.sequence {
            Arc::as_ptr(seq).hash(&mut hasher);
            canvas.current_frame.hash(&mut hasher);
        }
        Self {
            hash: hasher.finish(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AsciiCanvasWidgetKey {
    pub(crate) render_key: AsciiCanvasRenderKey,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl AsciiCanvasWidgetKey {
    fn new(canvas: &AsciiCanvas, render_key: AsciiCanvasRenderKey) -> Self {
        Self {
            render_key,
            width: canvas.width,
            height: canvas.height,
        }
    }
}

#[derive(Clone)]
pub struct AsciiCanvasNode {
    pub lines: Vec<Arc<str>>,
    pub cells: Option<Arc<[AsciiCell]>>,
    pub grid_size: Option<(u16, u16)>,
    pub sequence: Option<Arc<FrameSequence>>,
    pub current_frame: usize,
    pub style: Style,
    pub background: Option<Style>,
    pub width: Length,
    pub height: Length,
    pub gradient: Option<(ColorGradient, GradientDirection)>,
    /// Color remapping applied at render time (source → replacement), both channels.
    pub color_map: Option<Arc<[(Color, Color)]>>,
    /// Color remapping applied at render time, foreground only.
    pub fg_color_map: Option<Arc<[(Color, Color)]>>,
    /// Color remapping applied at render time, background only.
    pub bg_color_map: Option<Arc<[(Color, Color)]>>,
    pub(crate) render_key: AsciiCanvasRenderKey,
    pub(crate) widget_key: AsciiCanvasWidgetKey,
}

impl Default for AsciiCanvasNode {
    fn default() -> Self {
        let render_key = AsciiCanvasRenderKey { hash: 0 };
        Self {
            lines: Vec::new(),
            cells: None,
            grid_size: None,
            sequence: None,
            current_frame: 0,
            style: Style::default(),
            background: None,
            width: Length::Auto,
            height: Length::Auto,
            gradient: None,
            color_map: None,
            fg_color_map: None,
            bg_color_map: None,
            render_key,
            widget_key: AsciiCanvasWidgetKey {
                render_key,
                width: Length::Auto,
                height: Length::Auto,
            },
        }
    }
}

impl WidgetNode for AsciiCanvasNode {}

impl From<AsciiCanvas> for AsciiCanvasNode {
    fn from(canvas: AsciiCanvas) -> Self {
        let render_key = AsciiCanvasRenderKey::new(&canvas);
        let widget_key = AsciiCanvasWidgetKey::new(&canvas, render_key);
        Self {
            lines: canvas.lines,
            cells: canvas.cells,
            grid_size: canvas.grid_size,
            sequence: canvas.sequence,
            current_frame: canvas.current_frame,
            style: canvas.style,
            background: canvas.background,
            width: canvas.width,
            height: canvas.height,
            gradient: canvas.gradient,
            color_map: canvas.color_map,
            fg_color_map: canvas.fg_color_map,
            bg_color_map: canvas.bg_color_map,
            render_key,
            widget_key,
        }
    }
}

impl AsciiCanvasNode {
    pub(crate) fn render_key_for(canvas: &AsciiCanvas) -> AsciiCanvasRenderKey {
        AsciiCanvasRenderKey::new(canvas)
    }

    pub(crate) fn widget_key_for(
        canvas: &AsciiCanvas,
        render_key: AsciiCanvasRenderKey,
    ) -> AsciiCanvasWidgetKey {
        AsciiCanvasWidgetKey::new(canvas, render_key)
    }
}

impl From<AsciiCanvasNode> for NodeKind {
    fn from(node: AsciiCanvasNode) -> Self {
        NodeKind::AsciiCanvas(node)
    }
}
