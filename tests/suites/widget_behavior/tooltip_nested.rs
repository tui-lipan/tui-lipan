//! A tooltip steps aside while the pointer is on the trigger of a tooltip nested in its own, and
//! the nested tooltip takes over at once when the outer one was showing.
use std::time::Duration;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const DELAY: Duration = Duration::from_millis(1000);

/// How the button's tooltip is set up.
#[derive(Clone, Copy, PartialEq)]
enum Button {
    /// A delayed tooltip.
    Tip,
    /// A tooltip that never opens on hover.
    AutoOff,
    /// A tooltip held closed.
    HeldClosed,
    /// The button is gone.
    Removed,
}

/// A row with a card tooltip, holding a button with its own tooltip; with `middle`, the button
/// sits in a group with a third tooltip between the two. The card warms up apart from the rest,
/// so a swap between them never rides on warm-up.
struct Row {
    middle: Option<Middle>,
}

/// The tooltip between the card and the button.
#[derive(Clone, Copy, PartialEq)]
enum Middle {
    Enabled,
    /// It never opens on hover, so it must stay out of the way.
    AutoOff,
}

impl Component for Row {
    type Properties = ();
    type State = Button;
    type Message = Button;
    fn create_state(&self, _: &()) -> Button {
        Button::Tip
    }
    fn update(&mut self, button: Button, ctx: &mut Context<Self>) -> Update {
        ctx.state = button;
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut line = HStack::new()
            .height(Length::Px(1))
            .child(Text::new("row text").width(Length::Px(10)));
        if ctx.state != Button::Removed {
            let mut tip = Tooltip::new("button-tip")
                .delay(DELAY)
                .placement(PopoverPlacement::BelowStart)
                .child(tui_lipan::prelude::Button::new("Snooze"));
            match ctx.state {
                Button::AutoOff => tip = tip.auto(false),
                Button::HeldClosed => tip = tip.open(false),
                Button::Tip | Button::Removed => {}
            }
            let tip: Element = if let Some(middle) = self.middle {
                Tooltip::new("group-tip")
                    .delay(DELAY)
                    .auto(middle == Middle::Enabled)
                    .warmup_group("groups")
                    .placement(PopoverPlacement::BelowStart)
                    .offset((0, 2))
                    .child(
                        HStack::new()
                            .height(Length::Px(1))
                            .child(Text::new("group ").width(Length::Px(6)))
                            .child(tip),
                    )
                    .into()
            } else {
                tip.into()
            };
            line = line.child(tip);
        }
        let card = Tooltip::new("card-tip")
            .delay(DELAY)
            .warmup_group("cards")
            .placement(PopoverPlacement::BelowStart)
            .offset((0, 1))
            .child(line);
        VStack::new().child(card).into()
    }
}

fn backend() -> TestBackend<Row> {
    backend_with(None)
}

fn backend_with(middle: Option<Middle>) -> TestBackend<Row> {
    let mut backend = TestBackend::new(Row { middle });
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
fn nested_tooltip_takes_over_from_the_outer_one_until_the_pointer_returns() {
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
        shows(&backend, "button-tip"),
        "the button tooltip did not take over from the card at once"
    );

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

#[test]
fn nested_tooltip_waits_its_delay_when_the_outer_one_never_opened() {
    let mut backend = backend();
    hover(&mut backend, "row text");
    hover(&mut backend, "Snooze");
    assert!(!shows(&backend, "button-tip"));
    assert!(!shows(&backend, "card-tip"));
    wait(&mut backend);
    assert!(shows(&backend, "button-tip"));
    assert!(!shows(&backend, "card-tip"));
}

#[test]
fn hovering_a_nested_trigger_restarts_the_outer_delay() {
    let half = Duration::from_millis(600);
    let mut backend = backend();
    hover(&mut backend, "row text");
    backend.advance(half);
    backend.pump().unwrap();
    hover(&mut backend, "Snooze");
    backend.advance(half);
    backend.pump().unwrap();
    hover(&mut backend, "row text");
    assert!(
        !shows(&backend, "card-tip"),
        "time on the nested trigger counted toward the card's delay"
    );

    backend.advance(half);
    backend.pump().unwrap();
    backend.render();
    assert!(
        !shows(&backend, "card-tip"),
        "the card did not wait its full delay again"
    );
    wait(&mut backend);
    assert!(shows(&backend, "card-tip"));
}

#[test]
fn outer_tooltip_takes_over_from_a_showing_nested_one() {
    let mut backend = backend();
    hover(&mut backend, "row text");
    hover(&mut backend, "Snooze");
    wait(&mut backend);
    assert!(shows(&backend, "button-tip"));

    hover(&mut backend, "row text");
    assert!(
        shows(&backend, "card-tip"),
        "the card waited although the button tooltip was showing"
    );
    assert!(!shows(&backend, "button-tip"));

    hover(&mut backend, "Snooze");
    assert!(shows(&backend, "button-tip"));
    assert!(!shows(&backend, "card-tip"));
}

/// With the card open, the pointer moves onto a button whose tooltip cannot open.
fn card_over_a_button_that_cannot_show(button: Button) -> bool {
    let mut backend = backend();
    backend.dispatch(button).unwrap();
    backend.render();
    hover(&mut backend, "row text");
    wait(&mut backend);
    assert!(shows(&backend, "card-tip"));
    hover(&mut backend, "Snooze");
    wait(&mut backend);
    shows(&backend, "card-tip")
}

#[test]
fn a_nested_tooltip_that_cannot_open_leaves_the_outer_one_showing() {
    assert!(
        card_over_a_button_that_cannot_show(Button::AutoOff),
        "a nested tooltip with auto off hid the card"
    );
    assert!(
        card_over_a_button_that_cannot_show(Button::HeldClosed),
        "a nested tooltip held closed hid the card"
    );
}

#[test]
fn removing_a_hovered_nested_tooltip_brings_the_outer_one_back() {
    let mut backend = backend();
    hover(&mut backend, "row text");
    wait(&mut backend);
    hover(&mut backend, "Snooze");
    assert!(!shows(&backend, "card-tip"));

    backend.dispatch(Button::Removed).unwrap();
    backend.pump().unwrap();
    backend.render();
    backend.pump().unwrap();
    backend.render();
    assert!(
        shows(&backend, "card-tip"),
        "the card stayed aside for a button that is gone"
    );
}

#[test]
fn a_deeply_nested_tooltip_takes_over_from_a_showing_ancestor() {
    let mut backend = backend_with(Some(Middle::Enabled));
    hover(&mut backend, "row text");
    wait(&mut backend);
    assert!(shows(&backend, "card-tip"));

    hover(&mut backend, "Snooze");
    let text = backend.capture_frame().plain_text();
    assert!(
        text.contains("button-tip"),
        "the innermost tooltip waited although an ancestor was showing: {text:?}"
    );
    assert!(!text.contains("card-tip") && !text.contains("group-tip"));
}

#[test]
fn a_tooltip_between_that_cannot_open_stays_out_of_the_way() {
    let mut backend = backend_with(Some(Middle::AutoOff));
    hover(&mut backend, "row text");
    wait(&mut backend);
    assert!(shows(&backend, "card-tip"));

    hover(&mut backend, "Snooze");
    let text = backend.capture_frame().plain_text();
    assert!(
        !text.contains("card-tip"),
        "the card stayed over the button: {text:?}"
    );
    assert!(text.contains("button-tip"));
}

/// With the card open and the pointer on a button whose tooltip cannot open, the tooltip is
/// enabled.
fn enable_the_hovered_button(from: Button) {
    let mut backend = backend();
    backend.dispatch(from).unwrap();
    backend.render();
    hover(&mut backend, "row text");
    wait(&mut backend);
    hover(&mut backend, "Snooze");
    assert!(shows(&backend, "card-tip"));

    backend.dispatch(Button::Tip).unwrap();
    backend.pump().unwrap();
    backend.render();
    backend.pump().unwrap();
    backend.render();
    let text = backend.capture_frame().plain_text();
    assert!(
        text.contains("button-tip") && !text.contains("card-tip"),
        "enabling the hovered button's tooltip left it closed: {text:?}"
    );
}

#[test]
fn enabling_a_hovered_nested_tooltip_opens_it() {
    enable_the_hovered_button(Button::AutoOff);
    enable_the_hovered_button(Button::HeldClosed);
}
