//! Uncontrolled cursor and selection preservation for `Input::new` and `TextArea::new`.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods {
            shift: true,
            ..KeyMods::default()
        },
    }
}

// ── Uncontrolled Input ──────────────────────────────────────────────────────

enum InputMsg {
    Changed(String),
    ReplaceValue(String),
    UnrelatedTick,
}

struct UncontrolledInputState {
    value: String,
    ticks: usize,
}

struct UncontrolledInputApp {
    initial: &'static str,
}

impl Component for UncontrolledInputApp {
    type Message = InputMsg;
    type Properties = ();
    type State = UncontrolledInputState;

    fn create_state(&self, _: &()) -> Self::State {
        UncontrolledInputState {
            value: self.initial.to_string(),
            ticks: 0,
        }
    }

    fn update(&mut self, msg: InputMsg, ctx: &mut Context<Self>) -> Update {
        match msg {
            InputMsg::Changed(v) | InputMsg::ReplaceValue(v) => ctx.state.value = v,
            InputMsg::UnrelatedTick => ctx.state.ticks += 1,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(
                Input::new(ctx.state.value.clone())
                    .on_change(
                        ctx.link()
                            .callback(|ev: InputEvent| InputMsg::Changed(ev.value.to_string())),
                    )
                    .key("input"),
            )
            .child(Text::new(format!("ticks: {}", ctx.state.ticks)))
            .into()
    }
}

fn input_backend(initial: &'static str) -> TestBackend<UncontrolledInputApp> {
    let mut backend = TestBackend::new(UncontrolledInputApp { initial });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("input")));
    backend.render();
    backend
}

#[test]
fn uncontrolled_input_typing_in_middle_preserves_cursor() {
    let mut backend = input_backend("helo");

    // Move left once (from end 4 -> 3, between 'l' and 'o') and insert 'l'.
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Char('l'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "hello");

    // Cursor stays right after the inserted 'l' (pos 4), so typing '!' inserts before 'o'.
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "hell!o");
}

#[test]
fn uncontrolled_input_preserves_selection_across_unrelated_rerender() {
    let mut backend = input_backend("abcd");

    // Select "cd" (cursor 4 -> 2, anchor 4).
    backend.send_key(shift(KeyCode::Left)).unwrap();
    backend.send_key(shift(KeyCode::Left)).unwrap();
    // Trigger an unrelated rerender before typing replacement.
    backend.dispatch(InputMsg::UnrelatedTick).unwrap();
    backend.render();

    // Typing 'X' replaces the preserved selection "cd".
    backend.send_key(plain(KeyCode::Char('X'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "abX");
}

#[test]
fn uncontrolled_input_external_value_replacement_moves_cursor_to_end() {
    let mut backend = input_backend("hello");

    // Move cursor to the middle.
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.render();

    // Replace value externally (not via on_change).
    backend
        .dispatch(InputMsg::ReplaceValue("world".to_string()))
        .unwrap();
    backend.render();

    // Cursor should now be at the end of "world".
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "world!");
}

// ── Uncontrolled TextArea ───────────────────────────────────────────────────

enum AreaMsg {
    Changed(String),
    ReplaceValue(String),
}

struct UncontrolledAreaState {
    value: String,
}

struct UncontrolledAreaApp {
    initial: &'static str,
}

impl Component for UncontrolledAreaApp {
    type Message = AreaMsg;
    type Properties = ();
    type State = UncontrolledAreaState;

    fn create_state(&self, _: &()) -> Self::State {
        UncontrolledAreaState {
            value: self.initial.to_string(),
        }
    }

    fn update(&mut self, msg: AreaMsg, ctx: &mut Context<Self>) -> Update {
        match msg {
            AreaMsg::Changed(v) | AreaMsg::ReplaceValue(v) => ctx.state.value = v,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::new(ctx.state.value.clone())
            .on_change(
                ctx.link()
                    .callback(|ev: TextAreaEvent| AreaMsg::Changed(ev.value.to_string())),
            )
            .key("area")
    }
}

fn area_backend(initial: &'static str) -> TestBackend<UncontrolledAreaApp> {
    let mut backend = TestBackend::new(UncontrolledAreaApp { initial });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("area")));
    backend.render();
    backend
}

#[test]
fn uncontrolled_text_area_typing_in_middle_preserves_cursor() {
    let mut backend = area_backend("ac");

    // TextArea starts at 0; move right once (between 'a' and 'c') and insert 'b', then '!'.
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.send_key(plain(KeyCode::Char('b'))).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "ab!c");
}

#[test]
fn uncontrolled_text_area_external_value_replacement_moves_cursor_to_end() {
    let mut backend = area_backend("hello");

    // Cursor starts at 0; move right once.
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.render();

    // External value replacement moves cursor to the end.
    backend
        .dispatch(AreaMsg::ReplaceValue("world".to_string()))
        .unwrap();
    backend.render();

    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "world!");
}
