//! A press on a `DragSource` or `DropTarget` that never becomes a drag is a click on the row
//! holding it.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

#[derive(Clone, Copy)]
enum Wrapper {
    DragSource,
    DropTarget,
}

struct Rows(Wrapper);

impl Component for Rows {
    type Message = String;
    type Properties = ();
    type State = Vec<String>;
    fn create_state(&self, _: &()) -> Vec<String> {
        Vec::new()
    }
    fn update(&mut self, msg: String, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(msg);
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let row = |label: &'static str| -> Element {
            match self.0 {
                Wrapper::DragSource => DragSource::new()
                    .child(Text::new(label))
                    .drag_group("rows")
                    .on_drag_start(move |_| Some(Box::new(label) as Box<dyn DragPayload>))
                    .into(),
                Wrapper::DropTarget => DropTarget::new()
                    .child(Text::new(label))
                    .accept_group("rows")
                    .on_drop(ctx.link().callback(|_| "drop".to_string()))
                    .into(),
            }
        };
        let node = |label: &'static str| {
            TreeNode::new(ListItem::new(label))
                .content(row(label))
                .content_height(1)
                .show_arrow(false)
        };
        Tree::forest(vec![node("alpha"), node("beta")])
            .show_arrows(false)
            .on_select(ctx.link().callback(|e: TreeEvent| format!("select {}", e.index)))
            .into()
    }
}

fn press(wrapper: Wrapper, path: &[(u16, u16)]) -> Vec<String> {
    let mut backend = TestBackend::new(Rows(wrapper));
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    });
    backend.render();
    let (first, rest) = path.split_first().expect("a path");
    let last = *path.last().expect("a path");
    let mut send = |kind, (x, y): (u16, u16)| {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.pump().unwrap();
        backend.render();
    };
    send(MouseKind::Moved, *first);
    send(MouseKind::Down(MouseButton::Left), *first);
    for &point in rest {
        send(MouseKind::Drag(MouseButton::Left), point);
    }
    send(MouseKind::Up(MouseButton::Left), last);
    backend.state().clone()
}

#[test]
fn click_on_a_drag_source_selects_its_row() {
    assert_eq!(press(Wrapper::DragSource, &[(2, 1)]), ["select 1"]);
}

#[test]
fn click_on_a_drop_target_selects_its_row() {
    assert_eq!(press(Wrapper::DropTarget, &[(2, 1)]), ["select 1"]);
}

#[test]
fn drag_from_a_drag_source_does_not_select() {
    assert!(press(Wrapper::DragSource, &[(2, 1), (2, 2), (2, 3), (10, 3)]).is_empty());
}
