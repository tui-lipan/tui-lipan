//! Animated tooltips never fade over one another: a tooltip opening ends any fade still running,
//! and one that swaps in without waiting appears at once.
use std::time::Duration;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const DELAY: Duration = Duration::from_millis(1000);
const FADE: TransitionConfig = TransitionConfig {
    duration: Duration::from_millis(200),
    easing: Easing::Linear,
};

struct Toolbar;

fn tip(label: &'static str) -> Element {
    Tooltip::new(format!("{label}-tip"))
        .delay(DELAY)
        .animation(VisibilityAnimation::new().enter(FADE).exit(FADE))
        .placement(PopoverPlacement::BelowStart)
        .child(Text::new(label))
        .into()
}

impl Component for Toolbar {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        VStack::new()
            .child(
                HStack::new()
                    .gap(4)
                    .height(Length::Px(1))
                    .child(tip("Snooze"))
                    .child(tip("Settle")),
            )
            .into()
    }
}

fn hover(backend: &mut TestBackend<Toolbar>, label: &str) {
    let screen = backend.capture_frame().plain_text();
    let x = screen
        .lines()
        .next()
        .and_then(|line| line.find(label))
        .unwrap_or_else(|| panic!("`{label}` must be on screen: {screen:?}"));
    backend
        .send_mouse(MouseEvent {
            x: x as u16,
            y: 0,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.pump().unwrap();
    backend.render();
}

fn frames(backend: &mut TestBackend<Toolbar>, dt: Duration) {
    backend.advance_frame(dt);
    backend.pump().unwrap();
    backend.render();
}

fn shows(backend: &TestBackend<Toolbar>, tip: &str) -> bool {
    backend.capture_frame().plain_text().contains(tip)
}

#[test]
fn swapping_tooltips_ends_the_old_fade_and_skips_the_new_one() {
    let mut backend = TestBackend::new(Toolbar);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    });
    backend.render();

    hover(&mut backend, "Settle");
    backend.advance(DELAY);
    backend.pump().unwrap();
    backend.render();
    for _ in 0..6 {
        frames(&mut backend, Duration::from_millis(50));
    }
    assert!(shows(&backend, "Settle-tip"));

    hover(&mut backend, "Snooze");
    frames(&mut backend, Duration::from_millis(16));
    assert!(
        !shows(&backend, "Settle-tip"),
        "the old tooltip kept fading over the new one"
    );
    assert!(shows(&backend, "Snooze-tip"));
}
