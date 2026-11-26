//! A tooltip steps aside while the pointer is on the trigger of a tooltip nested in its own.
use std::time::Duration;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const DELAY: Duration = Duration::from_millis(1000);

/// A row with a card tooltip, holding a button with its own tooltip.
struct Row;

impl Component for Row {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let card = Tooltip::new("card-tip")
            .delay(DELAY)
            .placement(PopoverPlacement::BelowStart)
            .offset((0, 1))
            .child(
                HStack::new()
                    .height(Length::Px(1))
                    .child(Text::new("row text").width(Length::Px(10)))
                    .child(
                        Tooltip::new("button-tip")
                            .delay(DELAY)
                            .placement(PopoverPlacement::BelowStart)
                            .child(Button::new("Snooze")),
                    ),
            );
        VStack::new().child(card).into()
    }
}

fn backend() -> TestBackend<Row> {
    let mut backend = TestBackend::new(Row);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();
    backend
}

fn hover(backend: &mut TestBackend<Row>, label: &str) {
    let screen = backend.capture_frame().plain_text();
    let (x, y) = screen
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find(label).map(|x| (x as u16, y as u16)))
        .unwrap_or_else(|| panic!("`{label}` must be on screen: {screen:?}"));
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.pump().unwrap();
    backend.render();
}

fn wait(backend: &mut TestBackend<Row>) {
    backend.advance(DELAY + Duration::from_millis(50));
    backend.pump().unwrap();
    backend.render();
}

fn shows(backend: &TestBackend<Row>, tip: &str) -> bool {
    backend.capture_frame().plain_text().contains(tip)
}

#[test]
fn nested_trigger_hides_the_outer_tooltip_until_the_pointer_returns() {
    let mut backend = backend();
    hover(&mut backend, "row text");
    wait(&mut backend);
    assert!(shows(&backend, "card-tip"));

    hover(&mut backend, "Snooze");
    assert!(
        !shows(&backend, "card-tip"),
        "the card stayed over the button"
    );
    assert!(
        !shows(&backend, "button-tip"),
        "the button tooltip skipped its delay"
    );
    wait(&mut backend);
    assert!(shows(&backend, "button-tip"));
    assert!(!shows(&backend, "card-tip"));

    hover(&mut backend, "row text");
    assert!(
        shows(&backend, "card-tip"),
        "the card did not come back at once"
    );
    assert!(!shows(&backend, "button-tip"));

    backend
        .send_mouse(MouseEvent {
            x: 39,
            y: 5,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.pump().unwrap();
    backend.render();
    assert!(!shows(&backend, "card-tip"));
}
