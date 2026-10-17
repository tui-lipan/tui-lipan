//! A list row click says how it was made: the modifiers held and whether it was the second click
//! of a double-click, the one that activates.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct Clicks;

impl Component for Clicks {
    type Message = ListEvent;
    type Properties = ();
    type State = Vec<ListEvent>;
    fn create_state(&self, _: &()) -> Vec<ListEvent> {
        Vec::new()
    }
    fn update(&mut self, event: ListEvent, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(event);
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        List::new()
            .item("first")
            .item("second")
            .on_item_click(ctx.link().callback(|event| event))
            .into()
    }
}

fn click(backend: &mut TestBackend<Clicks>, mods: KeyMods) {
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: 2,
                y: 0,
                kind,
                mods,
            })
            .unwrap();
    }
    backend.render();
}

/// A third click in a run activates nothing, as a single click does not, so it reads as one.
#[test]
fn a_run_of_clicks_reads_as_single_double_single() {
    let mut backend = TestBackend::new(Clicks);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    });
    backend.render();
    for _ in 0..3 {
        click(&mut backend, KeyMods::NONE);
    }
    click(&mut backend, KeyMods::CTRL);
    let seen: Vec<(u8, KeyMods)> = backend
        .state()
        .iter()
        .map(|event| (event.clicks, event.mods))
        .collect();
    assert_eq!(seen.len(), 4, "{seen:?}");
    assert_eq!(
        seen.iter()
            .map(|(clicks, _)| *clicks)
            .take(3)
            .collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert_eq!(seen[3].1, KeyMods::CTRL);
    assert!(backend.state().iter().all(|event| event.index == 0));
}
