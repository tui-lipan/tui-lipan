use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct EmptyTransfer;

#[derive(Default)]
struct State {
    left: Vec<String>,
    right: Vec<String>,
    selected: Option<usize>,
}

#[derive(Clone)]
enum Msg {
    Transfer(DraggableTabTransferEvent),
    Select(TabsEvent),
}

impl Component for EmptyTransfer {
    type State = State;
    type Message = Msg;
    type Properties = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            left: vec!["Agents".to_string()],
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Transfer(event) => {
                let tab = ctx.state.left.remove(event.from);
                ctx.state.right.insert(event.to, tab);
            }
            Msg::Select(event) => ctx.state.selected = Some(event.index),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let bar = |id: &'static str, tabs: &[String]| {
            DraggableTabBar::new()
                .tabs(tabs.iter().map(|tab| DraggableTab::new(tab.as_str())))
                .bar_id(id)
                .drag_group("sidebar")
                .reorder_mode(DragReorderMode::Live)
                .height(Length::Px(1))
                .on_change(ctx.link().callback(Msg::Select))
                .on_transfer(ctx.link().callback(Msg::Transfer))
        };
        VStack::new()
            .child(bar("top", &ctx.state.left))
            .child(Spacer::new().height(Length::Px(1)))
            .child(bar("bottom", &ctx.state.right))
            .into()
    }
}

fn mouse(x: u16, y: u16, kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind,
        mods: KeyMods::NONE,
    }
}

#[test]
fn live_drag_transfers_into_an_empty_grouped_bar() {
    let mut backend = TestBackend::new(EmptyTransfer);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();

    assert!(
        backend
            .send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    assert!(
        backend
            .send_mouse(mouse(2, 2, MouseKind::Drag(MouseButton::Left)))
            .unwrap()
    );

    assert!(
        backend.state().left.is_empty(),
        "{}",
        backend.capture_ui_snapshot().to_markdown()
    );
    assert_eq!(backend.state().right, ["Agents"]);
    assert_eq!(backend.state().selected, Some(0));
}
