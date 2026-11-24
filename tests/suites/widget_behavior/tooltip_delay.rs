use std::time::Duration;

use tui_lipan::{TestBackend, core::event::MouseKind, prelude::*};

const DELAY: Duration = Duration::from_millis(1000);

struct DelayedApp;

impl Component for DelayedApp {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let tip = |label: &'static str, text: &'static str| -> Element {
            Tooltip::new(text)
                .delay(DELAY)
                .placement(PopoverPlacement::BelowStart)
                .child(Text::new(label))
                .into()
        };
        HStack::new()
            .gap(2)
            .height(Length::Px(1))
            .child(tip("Alpha", "tip-a"))
            .child(tip("Beta", "tip-b"))
            .into()
    }
}

fn backend() -> TestBackend<DelayedApp> {
    let mut backend = TestBackend::new(DelayedApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 6,
    });
    backend.render();
    backend
}

fn hover(backend: &mut TestBackend<DelayedApp>, label: Option<&str>) {
    let (x, y) = match label {
        Some(label) => {
            let snapshot = backend.capture_ui_snapshot();
            let rect = snapshot
                .widgets
                .iter()
                .find(|widget| widget.label.as_deref() == Some(label))
                .expect("trigger label must be rendered")
                .rect;
            (rect.x as u16, rect.y as u16)
        }
        None => (29, 5),
    };
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

fn wait(backend: &mut TestBackend<DelayedApp>, dt: Duration) {
    backend.advance(dt);
    backend.pump().unwrap();
    backend.render();
}

fn shows(backend: &TestBackend<DelayedApp>, text: &str) -> bool {
    backend.capture_frame().plain_text().contains(text)
}

const ALPHA: Option<&str> = Some("Alpha");
const BETA: Option<&str> = Some("Beta");
const EMPTY: Option<&str> = None;

#[test]
fn delayed_tooltip_waits_then_shows_and_hides_on_leave() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    assert!(!shows(&backend, "tip-a"));
    wait(&mut backend, DELAY / 2);
    assert!(!shows(&backend, "tip-a"));
    wait(&mut backend, DELAY / 2 + Duration::from_millis(10));
    assert!(shows(&backend, "tip-a"));
    hover(&mut backend, EMPTY);
    assert!(!shows(&backend, "tip-a"));
}

#[test]
fn sweeping_across_a_trigger_opens_nothing() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    wait(&mut backend, DELAY / 2);
    hover(&mut backend, EMPTY);
    wait(&mut backend, DELAY);
    assert!(!shows(&backend, "tip-a"));
}

#[test]
fn neighbour_opens_at_once_after_a_tooltip_was_shown_until_it_cools_down() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    wait(&mut backend, DELAY + Duration::from_millis(10));
    assert!(shows(&backend, "tip-a"));

    hover(&mut backend, BETA);
    assert!(
        shows(&backend, "tip-b"),
        "{}",
        backend.capture_frame().plain_text()
    );
    assert!(!shows(&backend, "tip-a"));

    hover(&mut backend, EMPTY);
    wait(&mut backend, DEFAULT_TOOLTIP_SKIP_DELAY * 2);
    hover(&mut backend, ALPHA);
    assert!(!shows(&backend, "tip-a"));
}
