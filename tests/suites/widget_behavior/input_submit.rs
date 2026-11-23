//! `Input::on_submit` plain-Enter submission and fallback behavior.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

fn shift_enter() -> KeyEvent {
    KeyEvent {
        code: KeyCode::Enter,
        mods: KeyMods {
            shift: true,
            ..KeyMods::default()
        },
    }
}

enum Msg {
    Changed(String),
    Submitted(InputEvent),
    InputKey(KeyEvent),
}

#[derive(Default)]
struct State {
    value: String,
    submitted: Vec<InputEvent>,
    input_keys: Vec<KeyEvent>,
    parent_keys: Vec<KeyEvent>,
}

struct SubmitApp {
    initial: &'static str,
    wire_submit: bool,
    read_only: bool,
}

impl Component for SubmitApp {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> Self::State {
        State {
            value: self.initial.to_string(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Changed(v) => ctx.state.value = v,
            Msg::Submitted(ev) => ctx.state.submitted.push(ev),
            Msg::InputKey(k) => ctx.state.input_keys.push(k),
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        ctx.state.parent_keys.push(key);
        KeyUpdate::handled(Update::full())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut input = Input::new(ctx.state.value.clone())
            .read_only(self.read_only)
            .on_change(
                ctx.link()
                    .callback(|ev: InputEvent| Msg::Changed(ev.value.to_string())),
            )
            .on_key(ctx.link().key_handler(|k| Some(Msg::InputKey(k))));
        if self.wire_submit {
            input = input.on_submit(ctx.link().callback(Msg::Submitted));
        }
        input.key("input")
    }
}

fn make_backend(app: SubmitApp) -> TestBackend<SubmitApp> {
    let mut backend = TestBackend::new(app);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("input")));
    backend.render();
    backend
}

#[test]
fn plain_enter_fires_on_submit_and_consumes_key() {
    let mut backend = make_backend(SubmitApp {
        initial: "hello",
        wire_submit: true,
        read_only: false,
    });

    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Enter)).unwrap();
    backend.render();

    assert_eq!(backend.state().submitted.len(), 1);
    assert_eq!(backend.state().submitted[0].value.as_ref(), "hello");
    assert_eq!(backend.state().submitted[0].cursor, 4);
    assert!(backend.state().input_keys.is_empty());
    assert!(backend.state().parent_keys.is_empty());
}

#[test]
fn modified_enter_does_not_fire_on_submit() {
    let mut backend = make_backend(SubmitApp {
        initial: "hello",
        wire_submit: true,
        read_only: false,
    });

    backend.send_key(shift_enter()).unwrap();
    backend.render();

    assert!(backend.state().submitted.is_empty());
    assert_eq!(backend.state().input_keys, vec![shift_enter()]);
}

#[test]
fn without_on_submit_enter_behaves_as_before() {
    let mut backend = make_backend(SubmitApp {
        initial: "hello",
        wire_submit: false,
        read_only: false,
    });

    backend.send_key(plain(KeyCode::Enter)).unwrap();
    backend.render();

    assert!(backend.state().submitted.is_empty());
    assert_eq!(backend.state().input_keys, vec![plain(KeyCode::Enter)]);
}

#[test]
fn read_only_input_does_not_fire_on_submit() {
    let mut backend = make_backend(SubmitApp {
        initial: "hello",
        wire_submit: true,
        read_only: true,
    });

    backend.send_key(plain(KeyCode::Enter)).unwrap();
    backend.render();

    assert!(backend.state().submitted.is_empty());
    assert_eq!(backend.state().input_keys, vec![plain(KeyCode::Enter)]);
}
