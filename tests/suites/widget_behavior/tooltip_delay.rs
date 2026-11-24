use std::time::Duration;

use tui_lipan::{
    TestBackend,
    core::event::{MouseButton, MouseKind},
    prelude::*,
};

const DELAY: Duration = Duration::from_millis(1000);

struct DelayedApp;

/// Alpha's tooltip settings, changed by tests while the pointer rests on it. Beta is fixed.
struct Alpha {
    delay: Duration,
    open: Option<bool>,
    auto: bool,
    skip_delay: Duration,
}

impl Default for Alpha {
    fn default() -> Self {
        Self {
            delay: DELAY,
            open: None,
            auto: true,
            skip_delay: DEFAULT_TOOLTIP_SKIP_DELAY,
        }
    }
}

impl Component for DelayedApp {
    type Properties = ();
    type State = Alpha;
    type Message = ();
    fn create_state(&self, _: &()) -> Alpha {
        Alpha::default()
    }
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let alpha = &ctx.state;
        let mut first = Tooltip::new("tip-a")
            .delay(alpha.delay)
            .skip_delay(alpha.skip_delay)
            .auto(alpha.auto)
            .placement(PopoverPlacement::BelowStart)
            .child(Text::new("Alpha"));
        if let Some(open) = alpha.open {
            first = first.open(open);
        }
        HStack::new()
            .gap(2)
            .height(Length::Px(1))
            .child(first)
            .child(
                Tooltip::new("tip-b")
                    .delay(DELAY)
                    .placement(PopoverPlacement::BelowStart)
                    .child(Button::new("Beta").key("beta")),
            )
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

fn configure(backend: &mut TestBackend<DelayedApp>, change: impl FnOnce(&mut Alpha)) {
    change(backend.state_mut());
    backend.render();
    backend.pump().unwrap();
    backend.render();
}

const STEP: Duration = Duration::from_millis(10);

#[test]
fn a_controlled_hidden_tooltip_never_warms_its_neighbours() {
    let mut backend = backend();
    configure(&mut backend, |alpha| alpha.open = Some(false));
    hover(&mut backend, ALPHA);
    wait(&mut backend, DELAY + STEP);
    assert!(!shows(&backend, "tip-a"));
    hover(&mut backend, BETA);
    assert!(!shows(&backend, "tip-b"), "beta must wait its full delay");
}

#[test]
fn a_delay_changed_mid_hover_counts_from_the_original_entry() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    wait(&mut backend, Duration::from_millis(200));
    configure(&mut backend, |alpha| {
        alpha.delay = Duration::from_millis(5000)
    });
    wait(&mut backend, DELAY);
    assert!(
        !shows(&backend, "tip-a"),
        "the old 1 s timer must not reveal"
    );

    configure(&mut backend, |alpha| {
        alpha.delay = Duration::from_millis(300)
    });
    assert!(
        shows(&backend, "tip-a"),
        "1.2 s already waited exceeds 300 ms"
    );
}

#[test]
fn turning_auto_off_mid_hover_cancels_the_reveal() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    wait(&mut backend, DELAY / 2);
    configure(&mut backend, |alpha| alpha.auto = false);
    wait(&mut backend, DELAY);
    assert!(!shows(&backend, "tip-a"));
}

#[test]
fn a_zero_skip_delay_always_waits() {
    let mut backend = backend();
    hover(&mut backend, BETA);
    wait(&mut backend, DELAY + STEP);
    assert!(shows(&backend, "tip-b"));
    configure(&mut backend, |alpha| alpha.skip_delay = Duration::ZERO);
    hover(&mut backend, ALPHA);
    assert!(!shows(&backend, "tip-a"));
    wait(&mut backend, DELAY + STEP);
    assert!(shows(&backend, "tip-a"));
}

#[test]
fn focus_shows_a_delayed_tooltip_at_once() {
    let mut backend = backend();
    assert!(backend.focus_key(&"beta".into()));
    backend.render();
    assert!(shows(&backend, "tip-b"));
}

#[test]
fn only_the_current_hover_can_reveal() {
    let mut backend = backend();
    hover(&mut backend, ALPHA);
    wait(&mut backend, DELAY / 2);
    hover(&mut backend, EMPTY);
    wait(&mut backend, Duration::from_millis(200));
    hover(&mut backend, ALPHA);
    // The first hover's timer comes due here.
    wait(&mut backend, Duration::from_millis(300) + STEP);
    assert!(!shows(&backend, "tip-a"));
    wait(&mut backend, Duration::from_millis(700));
    assert!(shows(&backend, "tip-a"));
}

/// A trigger with its own state, so a remount would show as a reset count.
struct Counter;

impl Component for Counter {
    type Properties = ();
    type State = u32;
    type Message = ();
    fn create_state(&self, _: &()) -> u32 {
        0
    }
    fn update(&mut self, (): (), ctx: &mut Context<Self>) -> Update {
        ctx.state += 1;
        Update::render()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        Button::new(format!("count {}", ctx.state))
            .on_click(ctx.link().callback(|_| ()))
            .into()
    }
}

struct StatefulTriggerApp;

impl Component for StatefulTriggerApp {
    type Properties = ();
    type State = Alpha;
    type Message = ();
    fn create_state(&self, _: &()) -> Alpha {
        Alpha::default()
    }
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let alpha = &ctx.state;
        let mut tooltip = Tooltip::new("tip")
            .delay(alpha.delay)
            .auto(alpha.auto)
            .child(tui_lipan::child(|| Counter, ()));
        if let Some(open) = alpha.open {
            tooltip = tooltip.open(open);
        }
        VStack::new().height(Length::Px(1)).child(tooltip).into()
    }
}

#[test]
fn changing_the_configuration_keeps_a_stateful_trigger_mounted() {
    let mut backend = TestBackend::new(StatefulTriggerApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 6,
    });
    backend.render();
    let rect = backend
        .capture_ui_snapshot()
        .widgets
        .iter()
        .find(|widget| widget.label.as_deref() == Some("count 0"))
        .expect("counter must render")
        .rect;
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: rect.x as u16,
                y: rect.y as u16,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    backend.pump().unwrap();
    backend.render();
    assert!(backend.capture_frame().plain_text().contains("count 1"));

    let changes: [fn(&mut Alpha); 4] = [
        |alpha| alpha.delay = Duration::ZERO,
        |alpha| alpha.delay = DELAY,
        |alpha| alpha.auto = false,
        |alpha| alpha.open = Some(true),
    ];
    for change in changes {
        change(backend.state_mut());
        backend.render();
        backend.pump().unwrap();
        backend.render();
        assert!(
            backend.capture_frame().plain_text().contains("count 1"),
            "the trigger was remounted:\n{}",
            backend.capture_frame().plain_text()
        );
    }
}

/// A tooltip as the root element, so its auto-sized trigger is handed the whole viewport.
struct RootTooltip {
    delay: Duration,
    open: Option<bool>,
}

impl Component for RootTooltip {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, (): (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let mut tooltip = Tooltip::new("Description")
            .delay(self.delay)
            .placement(PopoverPlacement::BelowStart)
            .child(Text::new("Hello").width(Length::Auto).height(Length::Auto));
        if let Some(open) = self.open {
            tooltip = tooltip.open(open);
        }
        tooltip.into()
    }
}

fn root_tooltip(delay: Duration, open: Option<bool>) -> TestBackend<RootTooltip> {
    let mut backend = TestBackend::new(RootTooltip { delay, open });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 6,
    });
    backend.render();
    backend
}

fn move_to<C: Component>(backend: &mut TestBackend<C>, x: u16, y: u16) {
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

fn rect_of<C: Component>(backend: &TestBackend<C>, label: &str) -> Option<Rect> {
    backend
        .capture_ui_snapshot()
        .widgets
        .iter()
        .find(|widget| widget.label.as_deref() == Some(label))
        .map(|widget| widget.rect)
}

#[test]
fn an_auto_sized_trigger_anchors_the_tooltip_to_itself_not_its_allocation() {
    let backend = root_tooltip(Duration::ZERO, Some(true));
    let hello = rect_of(&backend, "Hello").unwrap();
    let tooltip = rect_of(&backend, "Description").expect("tooltip must show");
    assert_eq!((hello.w, hello.h), (5, 1));
    assert_eq!(
        tooltip.y,
        hello.y + 1,
        "directly below the text: {tooltip:?}"
    );
    assert!(tooltip.h > 0);
}

#[test]
fn a_hover_revealed_tooltip_lands_where_an_open_one_does() {
    let open = rect_of(&root_tooltip(Duration::ZERO, Some(true)), "Description").unwrap();

    let mut backend = root_tooltip(DELAY, None);
    let hello = rect_of(&backend, "Hello").unwrap();
    move_to(&mut backend, hello.x as u16, hello.y as u16);
    backend.advance(DELAY + STEP);
    backend.pump().unwrap();
    backend.render();
    assert_eq!(rect_of(&backend, "Description"), Some(open));
}

#[test]
fn hovering_the_allocation_outside_the_trigger_opens_nothing() {
    let mut backend = root_tooltip(DELAY, None);
    let hello = rect_of(&backend, "Hello").unwrap();
    move_to(&mut backend, 20, hello.y as u16 + 3);
    backend.advance(DELAY + STEP);
    backend.pump().unwrap();
    backend.render();
    assert!(rect_of(&backend, "Description").is_none());
}
