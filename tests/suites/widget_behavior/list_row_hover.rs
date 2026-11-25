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
            let _ = self.composed;
            ListItem::new(label)
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
