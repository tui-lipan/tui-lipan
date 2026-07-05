use crate::backend::ratatui_backend::renderers::simple_diagram::{
    SimpleDiagramRenderCtx, render_simple_diagram,
};
use crate::style::{Rect, Theme};
use crate::widgets::internal::ClassDiagramNode;
pub(crate) fn render_class_diagram(
    f: &mut ratatui::Frame<'_>,
    node: &ClassDiagramNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
) {
    render_simple_diagram(
        f,
        theme,
        rect,
        clip_rect,
        SimpleDiagramRenderCtx {
            style: node.style,
            padding: node.padding,
            node_padding: node.node_padding,
            boxes: &node.boxes,
            edges: &node.edges,
            output: &node.output,
        },
    );
}
