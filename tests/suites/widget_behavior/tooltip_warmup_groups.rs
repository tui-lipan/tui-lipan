//! `TooltipConfig::warmup_group`: a closing tooltip warms only tooltips in its own group.
use std::time::Duration;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const DELAY: Duration = Duration::from_millis(1000);

struct Toolbar;

fn tip(label: &'static str, group: Option<&'static str>, skip: Duration) -> Element {
    let mut tooltip = Tooltip::new(format!("{label}-tip"))
        .delay(DELAY)
        .skip_delay(skip)
        .placement(PopoverPlacement::BelowStart);
    if let Some(group) = group {
        tooltip = tooltip.warmup_group(group);
    }
    tooltip.child(Text::new(label)).into()
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
        HStack::new()
            .gap(2)
            .height(Length::Px(1))
            .child(tip("CardA", Some("cards"), Duration::ZERO))
            .child(tip("CardB", Some("cards"), Duration::ZERO))
            .child(tip("ChipA", Some("chips"), DEFAULT_TOOLTIP_SKIP_DELAY))
            .child(tip("ChipB", Some("chips"), DEFAULT_TOOLTIP_SKIP_DELAY))
            .child(tip("Save", None, DEFAULT_TOOLTIP_SKIP_DELAY))
            .into()
    }
}

fn backend() -> TestBackend<Toolbar> {
    let mut backend = TestBackend::new(Toolbar);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 4,
    });
    backend.render();
    backend
}

fn hover(backend: &mut TestBackend<Toolbar>, label: &str) {
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

fn wait(backend: &mut TestBackend<Toolbar>) {
    backend.advance(DELAY);
    backend.pump().unwrap();
    backend.render();
}

fn shows(backend: &TestBackend<Toolbar>, tip: &str) -> bool {
    backend.capture_frame().plain_text().contains(tip)
}

#[test]
fn closing_a_tooltip_in_one_group_does_not_warm_another_group() {
    let mut backend = backend();
    hover(&mut backend, "CardA");
    wait(&mut backend);
    assert!(shows(&backend, "CardA-tip"));

    hover(&mut backend, "Save");
    assert!(!shows(&backend, "Save-tip"));
    wait(&mut backend);
    assert!(shows(&backend, "Save-tip"));
}

#[test]
fn a_group_without_a_skip_window_waits_for_every_tooltip() {
    let mut backend = backend();
    hover(&mut backend, "CardA");
    wait(&mut backend);
    hover(&mut backend, "CardB");
    assert!(!shows(&backend, "CardB-tip"));
    wait(&mut backend);
    assert!(shows(&backend, "CardB-tip"));
}

#[test]
fn tooltips_in_one_group_still_warm_each_other() {
    let mut backend = backend();
    hover(&mut backend, "ChipA");
    wait(&mut backend);
    hover(&mut backend, "ChipB");
    assert!(shows(&backend, "ChipB-tip"));
}
