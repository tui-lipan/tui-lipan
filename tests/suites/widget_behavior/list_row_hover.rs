//! `List::on_hover` reports every row the pointer enters and leaves, in either rendering mode.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct Rows {
    composed: bool,
}

impl Component for Rows {
    type Message = ListHoverEvent;
    type Properties = ();
    type State = Vec<(usize, bool)>;
    fn create_state(&self, _: &()) -> Self::State {
        Vec::new()
    }
    fn update(&mut self, event: ListHoverEvent, ctx: &mut Context<Self>) -> Update {
        ctx.state.push((event.index, event.hovered));
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let item = |label: &'static str| {
            let item = ListItem::new(label);
            if self.composed {
                item.content(Text::new(label))
            } else {
                item
            }
        };
        List::new()
            .item(ListItem::header("Group"))
            .item(item("first"))
            .item(item("second"))
            .on_hover(ctx.link().callback(|event| event))
            .into()
    }
}

fn hover_trail(composed: bool) -> Vec<(usize, bool)> {
    let mut backend = TestBackend::new(Rows { composed });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    });
    backend.render();
    for y in [0, 1, 2, 2, 5] {
        backend
            .send_mouse(MouseEvent {
                x: 3,
                y,
                kind: MouseKind::Moved,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    backend.state().clone()
}

const TRAIL: [(usize, bool); 6] = [
    (0, true),
    (0, false),
    (1, true),
    (1, false),
    (2, true),
    (2, false),
];

#[test]
fn text_rows_report_entry_and_exit() {
    assert_eq!(hover_trail(false), TRAIL);
}

#[test]
fn composed_rows_report_entry_and_exit() {
    assert_eq!(hover_trail(true), TRAIL);
}

/// Keyed rows that start as `a` (two lines) and `b`, with `b` on line 2 at index 1, then change to
/// `after`.
struct Reordered {
    after: &'static [(&'static str, u16)],
}

impl Component for Reordered {
    type Message = Option<ListHoverEvent>;
    type Properties = ();
    type State = (bool, Vec<(usize, bool)>);
    fn create_state(&self, _: &()) -> Self::State {
        (false, Vec::new())
    }
    fn update(&mut self, event: Option<ListHoverEvent>, ctx: &mut Context<Self>) -> Update {
        match event {
            Some(event) => ctx.state.1.push((event.index, event.hovered)),
            None => ctx.state.0 = true,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let rows: &[(&str, u16)] = if ctx.state.0 {
            self.after
        } else {
            &[("a", 2), ("b", 1)]
        };
        List::new()
            .items(rows.iter().map(|&(label, height)| {
                ListItem::new(label)
                    .key(label)
                    .content(Text::new(label))
                    .content_height(height)
            }))
            .on_hover(ctx.link().callback(Some))
            .into()
    }
}

/// What the app hears while the pointer rests on line 2 and the rows change to `after`.
fn reorder_trail(after: &'static [(&'static str, u16)]) -> Vec<(usize, bool)> {
    let mut backend = TestBackend::new(Reordered { after });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    });
    backend.render();
    backend
        .send_mouse(MouseEvent {
            x: 0,
            y: 2,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    backend.dispatch(None).unwrap();
    backend.render();
    backend.pump().unwrap();
    backend.state().1.clone()
}

/// A hovered keyed row that moves to another index while staying under the pointer reports the
/// move, so the index the app last heard is the row's.
#[test]
fn a_hovered_row_that_moves_under_the_pointer_reports_its_new_index() {
    assert_eq!(
        reorder_trail(&[("x", 1), ("y", 1), ("b", 1)]),
        [(1, true), (1, false), (2, true)]
    );
}

/// A hovered keyed row that moves out from under the pointer, leaving it over empty space, only
/// leaves.
#[test]
fn a_hovered_row_that_moves_away_only_leaves() {
    assert_eq!(
        reorder_trail(&[("b", 1), ("c", 1)]),
        [(1, true), (1, false)]
    );
}
