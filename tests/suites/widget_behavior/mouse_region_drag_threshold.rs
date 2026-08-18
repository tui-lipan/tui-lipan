//! `MouseRegion` drag activation through `TestBackend`: the default threshold is looser on
//! columns than on rows, and `drag_threshold` overrides it per region.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::style::Rect;

/// Pointer offset from the drag origin carried by each delivered drag event.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Travel {
    dx: i32,
    dy: i32,
}

struct DragApp {
    threshold: Option<(u16, u16)>,
}

#[derive(Default)]
struct State {
    travels: Vec<Travel>,
}

impl Component for DragApp {
    type Message = Travel;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.travels.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut region = MouseRegion::new()
            .on_drag(ctx.link().callback(|event: MouseDragEvent| Travel {
                dx: i32::from(event.x) - i32::from(event.from_x),
                dy: i32::from(event.y) - i32::from(event.from_y),
            }))
            .child(Text::new("").width(Length::Flex(1)).height(Length::Flex(1)));
        if let Some((columns, rows)) = self.threshold {
            region = region.drag_threshold(columns, rows);
        }
        region.into()
    }
}

/// Press at `(x, y)`, then step the pointer one cell at a time along `axis`.
fn drag_one_cell_at_a_time(threshold: Option<(u16, u16)>, horizontal: bool) -> Vec<Travel> {
    let mut backend = TestBackend::new(DragApp { threshold });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 20,
    });
    backend.render();

    let event = |x, y, kind| MouseEvent {
        x,
        y,
        kind,
        mods: Default::default(),
    };
    let (origin_x, origin_y) = (10u16, 10u16);
    backend
        .send_mouse(event(
            origin_x,
            origin_y,
            MouseKind::Down(MouseButton::Left),
        ))
        .expect("press");
    for step in 1..=4u16 {
        let (x, y) = if horizontal {
            (origin_x + step, origin_y)
        } else {
            (origin_x, origin_y + step)
        };
        backend
            .send_mouse(event(x, y, MouseKind::Drag(MouseButton::Left)))
            .expect("drag");
    }
    backend.state_mut().travels.clone()
}

/// The default is 3 columns or 1 row, so the first two columns of a horizontal gesture are
/// swallowed and the pointer arrives already three cells out.
#[test]
fn the_default_threshold_is_looser_on_columns_than_on_rows() {
    assert_eq!(
        drag_one_cell_at_a_time(None, true),
        vec![Travel { dx: 3, dy: 0 }, Travel { dx: 4, dy: 0 },],
        "a horizontal drag should not start before the third column"
    );
    assert_eq!(
        drag_one_cell_at_a_time(None, false),
        vec![
            Travel { dx: 0, dy: 1 },
            Travel { dx: 0, dy: 2 },
            Travel { dx: 0, dy: 3 },
            Travel { dx: 0, dy: 4 },
        ],
        "a vertical drag should start on the first row"
    );
}

/// A region whose only gesture is dragging - a resize handle, a split divider - opts into
/// tracking the pointer from its first step on either axis. Without this a horizontal drag
/// stalls for two cells and then jumps three at once.
#[test]
fn a_region_can_lower_its_own_drag_threshold() {
    assert_eq!(
        drag_one_cell_at_a_time(Some((1, 1)), true),
        vec![
            Travel { dx: 1, dy: 0 },
            Travel { dx: 2, dy: 0 },
            Travel { dx: 3, dy: 0 },
            Travel { dx: 4, dy: 0 },
        ]
    );
    assert_eq!(
        drag_one_cell_at_a_time(Some((1, 1)), false),
        vec![
            Travel { dx: 0, dy: 1 },
            Travel { dx: 0, dy: 2 },
            Travel { dx: 0, dy: 3 },
            Travel { dx: 0, dy: 4 },
        ]
    );
}

/// A raised threshold holds the gesture back on both axes.
#[test]
fn a_region_can_raise_its_own_drag_threshold() {
    assert_eq!(
        drag_one_cell_at_a_time(Some((4, 4)), false),
        vec![Travel { dx: 0, dy: 4 }],
        "a 4-row threshold should swallow the first three rows"
    );
}
