//! A controlled capturing overlay the app closes by re-rendering it (`Select` picking an option)
//! hands focus back like a dismissal does: to the trigger, unless the same update asked for focus
//! elsewhere.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Picker {
    focus_after_pick: bool,
}

#[derive(Clone, Copy)]
enum Msg {
    Toggle(bool),
    Highlight(usize),
    Pick(usize),
}

#[derive(Default)]
struct State {
    selected: usize,
    highlighted: usize,
    expanded: bool,
}

impl Component for Picker {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle(open) => {
                ctx.state.expanded = open;
                ctx.state.highlighted = ctx.state.selected;
            }
            Msg::Highlight(index) => ctx.state.highlighted = index,
            Msg::Pick(index) => {
                ctx.state.selected = index;
                if self.focus_after_pick {
                    ctx.request_focus("after");
                }
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let state = &ctx.state;
        let shown = if state.expanded {
            state.highlighted
        } else {
            state.selected
        };
        VStack::new()
            .child(Button::new("before").key("before"))
            .child(
                Select::new()
                    .options(["one", "two", "three"])
                    .selected(Some(shown))
                    .expanded(state.expanded)
                    .on_toggle(ctx.link().callback(Msg::Toggle))
                    .on_change(ctx.link().callback(Msg::Highlight))
                    .on_select(ctx.link().callback(Msg::Pick))
                    .key("select"),
            )
            .child(Button::new("after").key("after"))
            .into()
    }
}

/// The picker, focused on the select's (unkeyed) trigger, and the trigger's node.
fn picker(focus_after_pick: bool) -> (TestBackend<Picker>, Option<tui_lipan::NodeId>) {
    let mut backend = TestBackend::new(Picker { focus_after_pick });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("before")));
    backend.focus_next();
    backend.render();
    let trigger = backend.focused();
    assert!(trigger.is_some() && backend.focused_key().is_none());
    (backend, trigger)
}

fn press(backend: &mut TestBackend<Picker>, code: KeyCode) {
    backend
        .send_key(KeyEvent {
            code,
            mods: KeyMods::NONE,
        })
        .expect("key dispatches");
    backend.pump().expect("messages pump");
    backend.render();
}

#[test]
fn picking_an_option_returns_focus_to_the_trigger() {
    let (mut backend, trigger) = picker(false);

    press(&mut backend, KeyCode::Enter);
    assert!(backend.state().expanded);
    press(&mut backend, KeyCode::Esc);
    assert!(!backend.state().expanded);
    assert_eq!(backend.focused(), trigger, "dismissed");

    press(&mut backend, KeyCode::Enter);
    press(&mut backend, KeyCode::Enter);
    assert!(!backend.state().expanded);
    assert_eq!(backend.focused(), trigger, "picked the same");

    press(&mut backend, KeyCode::Enter);
    press(&mut backend, KeyCode::Down);
    press(&mut backend, KeyCode::Enter);
    assert_eq!(backend.state().selected, 1);
    assert!(!backend.state().expanded);
    assert_eq!(backend.focused(), trigger, "picked another");
}

#[test]
fn a_focus_request_made_while_closing_wins_over_the_restore() {
    let (mut backend, _) = picker(true);

    press(&mut backend, KeyCode::Enter);
    press(&mut backend, KeyCode::Enter);
    assert!(!backend.state().expanded);
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));
}
