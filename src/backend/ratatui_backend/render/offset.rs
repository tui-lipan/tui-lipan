use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderOffset {
    x: i32,
    y: i32,
}

impl RenderOffset {
    pub(crate) const ZERO: Self = Self { x: 0, y: 0 };

    pub(crate) fn add_cells(self, (x, y): (i16, i16)) -> Self {
        Self {
            x: self.x.saturating_add(i32::from(x)),
            y: self.y.saturating_add(i32::from(y)),
        }
    }

    pub(crate) fn apply_to_rect(self, mut rect: Rect) -> Rect {
        rect.x = (i32::from(rect.x).saturating_add(self.x)).clamp(i16::MIN as i32, i16::MAX as i32)
            as i16;
        rect.y = (i32::from(rect.y).saturating_add(self.y)).clamp(i16::MIN as i32, i16::MAX as i32)
            as i16;
        rect
    }
}

pub(crate) fn translate_clip(clip: Option<Rect>, (dx, dy): (i16, i16)) -> Option<Rect> {
    if dx == 0 && dy == 0 {
        return clip;
    }
    clip.map(|c| Rect {
        x: (i32::from(c.x).saturating_add(i32::from(dx))).clamp(i16::MIN as i32, i16::MAX as i32)
            as i16,
        y: (i32::from(c.y).saturating_add(i32::from(dy))).clamp(i16::MIN as i32, i16::MAX as i32)
            as i16,
        w: c.w,
        h: c.h,
    })
}

pub(crate) fn render_offset_for_node(tree: &NodeTree, id: NodeId) -> RenderOffset {
    let mut chain = Vec::new();
    let mut current = Some(id);

    while let Some(node_id) = current {
        if !tree.is_valid(node_id) {
            break;
        }

        chain.push(node_id);

        if node_id == tree.root || tree.overlay_roots().iter().any(|root| root.id == node_id) {
            break;
        }

        current = tree.node(node_id).parent;
    }

    let mut offset = RenderOffset::ZERO;
    for node_id in chain.into_iter().rev() {
        if let NodeKind::Animated(animated) = &tree.node(node_id).kind {
            offset = offset.add_cells(animated.visual_position_offset_cells());
        }
    }

    offset
}
