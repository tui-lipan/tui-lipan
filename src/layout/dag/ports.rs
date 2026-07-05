use crate::style::Rect;

use super::DagPoint;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PortSide {
    North,
    East,
    South,
    West,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DagPort {
    pub(crate) side: PortSide,
    pub(crate) point: DagPoint,
}

pub(super) fn connect(from: Rect, to: Rect) -> (DagPort, DagPort) {
    let from_center_x = center_x(from);
    let from_center_y = center_y(from);
    let to_center_x = center_x(to);
    let to_center_y = center_y(to);
    let dx = to_center_x - from_center_x;
    let dy = to_center_y - from_center_y;

    if dy.abs() >= dx.abs() {
        if dy >= 0 {
            (port(from, PortSide::South), port(to, PortSide::North))
        } else {
            (port(from, PortSide::North), port(to, PortSide::South))
        }
    } else if dx >= 0 {
        (port(from, PortSide::East), port(to, PortSide::West))
    } else {
        (port(from, PortSide::West), port(to, PortSide::East))
    }
}

pub(super) fn port(rect: Rect, side: PortSide) -> DagPort {
    let x = center_x(rect);
    let y = center_y(rect);
    let point = match side {
        PortSide::North => DagPoint::new(x, rect.y),
        PortSide::East => DagPoint::new(rect.x.saturating_add(rect.w.saturating_sub(1) as i16), y),
        PortSide::South => DagPoint::new(x, rect.y.saturating_add(rect.h.saturating_sub(1) as i16)),
        PortSide::West => DagPoint::new(rect.x, y),
    };
    DagPort { side, point }
}

/// Returns a port positioned at `target_position` along the requested face,
/// clamped to the face's interior cells (corners excluded). Used when the
/// caller knows where the OPPOSITE end of the edge sits and wants the source
/// port column-aligned with it so the edge can drop straight down with no
/// horizontal jog.
pub(super) fn port_aligned(rect: Rect, side: PortSide, target_position: i16) -> DagPort {
    let point = match side {
        PortSide::North | PortSide::South => {
            let interior_left = rect.x.saturating_add(1);
            let interior_right = rect.x.saturating_add(rect.w.saturating_sub(2) as i16);
            let x = target_position.clamp(interior_left, interior_right);
            let y = if matches!(side, PortSide::North) {
                rect.y
            } else {
                rect.y.saturating_add(rect.h.saturating_sub(1) as i16)
            };
            DagPoint::new(x, y)
        }
        PortSide::East | PortSide::West => {
            let interior_top = rect.y.saturating_add(1);
            let interior_bottom = rect.y.saturating_add(rect.h.saturating_sub(2) as i16);
            let y = target_position.clamp(interior_top, interior_bottom);
            let x = if matches!(side, PortSide::West) {
                rect.x
            } else {
                rect.x.saturating_add(rect.w.saturating_sub(1) as i16)
            };
            DagPoint::new(x, y)
        }
    };
    DagPort { side, point }
}

/// Returns the `index`-th port (0-based) out of `count` ports distributed
/// evenly across the requested face, skipping the corner cells. When
/// `count <= 1`, falls back to the centered [`port`] position so single-edge
/// cases stay visually centered on their box face.
pub(super) fn port_distributed(rect: Rect, side: PortSide, index: u16, count: u16) -> DagPort {
    if count <= 1 {
        return port(rect, side);
    }
    let point = match side {
        PortSide::North | PortSide::South => {
            let interior = rect.w.saturating_sub(2);
            let offset =
                ((u32::from(interior) * u32::from(index + 1)) / u32::from(count + 1)) as i16;
            let x = rect.x.saturating_add(1).saturating_add(offset);
            let y = if matches!(side, PortSide::North) {
                rect.y
            } else {
                rect.y.saturating_add(rect.h.saturating_sub(1) as i16)
            };
            DagPoint::new(x, y)
        }
        PortSide::East | PortSide::West => {
            let interior = rect.h.saturating_sub(2);
            let offset =
                ((u32::from(interior) * u32::from(index + 1)) / u32::from(count + 1)) as i16;
            let y = rect.y.saturating_add(1).saturating_add(offset);
            let x = if matches!(side, PortSide::West) {
                rect.x
            } else {
                rect.x.saturating_add(rect.w.saturating_sub(1) as i16)
            };
            DagPoint::new(x, y)
        }
    };
    DagPort { side, point }
}

fn center_x(rect: Rect) -> i16 {
    rect.x.saturating_add((rect.w / 2) as i16)
}

fn center_y(rect: Rect) -> i16 {
    rect.y.saturating_add((rect.h / 2) as i16)
}
