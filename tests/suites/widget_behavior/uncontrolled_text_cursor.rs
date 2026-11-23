//! Uncontrolled cursor and selection preservation for `Input::new` and `TextArea::new`.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseKind};
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
    SetReject(bool),
    UnrelatedTick,
}

struct UncontrolledInputState {
    value: String,
    reject_edits: bool,
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
            reject_edits: false,
            ticks: 0,
        }
    }

    fn update(&mut self, msg: InputMsg, ctx: &mut Context<Self>) -> Update {
        match msg {
            InputMsg::Changed(v) => {
                if !ctx.state.reject_edits {
                    ctx.state.value = v;
                }
            }
            InputMsg::ReplaceValue(v) => ctx.state.value = v,
            InputMsg::SetReject(reject) => ctx.state.reject_edits = reject,
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

    // Accepted paste in the middle also keeps cursor right after the pasted text.
    backend.send_paste("?").unwrap();
    backend.render();
    assert_eq!(backend.state().value, "hell!?o");
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

    // Select "lo" so we verify external replacement clears selection too.
    backend.send_key(shift(KeyCode::Left)).unwrap();
    backend.send_key(shift(KeyCode::Left)).unwrap();
    backend.render();

    // Replace value externally (not via on_change).
    backend
        .dispatch(InputMsg::ReplaceValue("world".to_string()))
        .unwrap();
    backend.render();

    // Cursor should now be at the end of "world" with no selection.
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "world!");
}

#[test]
fn uncontrolled_input_rejected_insert_preserves_mid_text_cursor() {
    let mut backend = input_backend("abc");

    // Move from end (3) to pos 1 (between 'a' and 'b').
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Left)).unwrap();

    // Reject both typed and pasted inserts.
    backend.dispatch(InputMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abc");
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abc");

    // Accepting the next edit proves the cursor stayed at pos 1.
    backend.dispatch(InputMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bc");
}

#[test]
fn uncontrolled_input_rejected_delete_preserves_cursor() {
    let mut backend = input_backend("abc");

    // Move to pos 1 (between 'a' and 'b').
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Left)).unwrap();

    // Reject Backspace and Delete.
    backend.dispatch(InputMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Backspace)).unwrap();
    assert_eq!(backend.state().value, "abc");
    backend.send_key(plain(KeyCode::Delete)).unwrap();
    assert_eq!(backend.state().value, "abc");

    // Cursor stays at pos 1.
    backend.dispatch(InputMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bc");
}

#[test]
fn uncontrolled_input_rejected_selection_replacement_preserves_cursor_and_selection() {
    let mut backend = input_backend("abcd");

    // Select "bc" (pos 3 -> 1 with anchor 3).
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(shift(KeyCode::Left)).unwrap();
    backend.send_key(shift(KeyCode::Left)).unwrap();

    // Reject typed replacement, paste replacement, and Backspace delete.
    backend.dispatch(InputMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abcd");
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abcd");
    backend.send_key(plain(KeyCode::Backspace)).unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Selection "bc" is still intact: typing 'Z' replaces "bc".
    backend.dispatch(InputMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('Z'))).unwrap();
    assert_eq!(backend.state().value, "aZd");
}

#[test]
fn uncontrolled_input_cursor_move_then_rejected_edit_preserves_moved_cursor() {
    let mut backend = input_backend("abcd");

    // Move cursor to pos 2 via arrow keys, then reject an edit.
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.send_key(plain(KeyCode::Left)).unwrap();
    backend.dispatch(InputMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Move cursor again via mouse click (padding left = 1, so x = 2 lands at col 1 -> pos 1),
    // then reject a paste.
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Accepting an insert lands at pos 1 (where the click moved the cursor).
    backend.dispatch(InputMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bcd");
}

#[test]
fn uncontrolled_input_drag_selection_then_rejected_paste_preserves_selection() {
    let mut backend = input_backend("abcd");

    // Drag from col 1 (x = 2 with left padding 1) to col 3 (x = 4) to select "bc".
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 4,
            y: 0,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 4,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();

    backend.dispatch(InputMsg::SetReject(true)).unwrap();
    backend.send_paste("x").unwrap();
    assert_eq!(backend.state().value, "abcd");

    backend.dispatch(InputMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('Z'))).unwrap();
    assert_eq!(backend.state().value, "aZd");
}

// ── Uncontrolled TextArea ───────────────────────────────────────────────────

enum AreaMsg {
    Changed(String),
    ReplaceValue(String),
    SetReject(bool),
}

struct UncontrolledAreaState {
    value: String,
    reject_edits: bool,
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
            reject_edits: false,
        }
    }

    fn update(&mut self, msg: AreaMsg, ctx: &mut Context<Self>) -> Update {
        match msg {
            AreaMsg::Changed(v) => {
                if !ctx.state.reject_edits {
                    ctx.state.value = v;
                }
            }
            AreaMsg::ReplaceValue(v) => ctx.state.value = v,
            AreaMsg::SetReject(reject) => ctx.state.reject_edits = reject,
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

    // TextArea starts at 0; move right once (between 'a' and 'c') and insert 'b', '!', and paste "?".
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.send_key(plain(KeyCode::Char('b'))).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.send_paste("?").unwrap();
    backend.render();
    assert_eq!(backend.state().value, "ab!?c");
}

#[test]
fn uncontrolled_text_area_external_value_replacement_moves_cursor_to_end() {
    let mut backend = area_backend("hello");

    // Select "he" so we verify external replacement clears selection too.
    backend.send_key(shift(KeyCode::Right)).unwrap();
    backend.send_key(shift(KeyCode::Right)).unwrap();
    backend.render();

    // External value replacement moves cursor to the end and clears selection.
    backend
        .dispatch(AreaMsg::ReplaceValue("world".to_string()))
        .unwrap();
    backend.render();

    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    backend.render();
    assert_eq!(backend.state().value, "world!");
}

#[test]
fn uncontrolled_text_area_rejected_insert_preserves_mid_text_cursor() {
    let mut backend = area_backend("abc");

    // Move from 0 to pos 1 (between 'a' and 'b').
    backend.send_key(plain(KeyCode::Right)).unwrap();

    // Reject both typed and pasted inserts (paste goes through sync_text_area_node_from_editor).
    backend.dispatch(AreaMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abc");
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abc");

    // Accepting the next edit proves the cursor stayed at pos 1 rather than jumping to the end.
    backend.dispatch(AreaMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bc");
}

#[test]
fn uncontrolled_text_area_rejected_delete_preserves_cursor() {
    let mut backend = area_backend("abc");

    // Move to pos 1 (between 'a' and 'b').
    backend.send_key(plain(KeyCode::Right)).unwrap();

    // Reject Backspace and Delete.
    backend.dispatch(AreaMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Backspace)).unwrap();
    assert_eq!(backend.state().value, "abc");
    backend.send_key(plain(KeyCode::Delete)).unwrap();
    assert_eq!(backend.state().value, "abc");

    // Cursor stays at pos 1.
    backend.dispatch(AreaMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bc");
}

#[test]
fn uncontrolled_text_area_rejected_selection_replacement_preserves_cursor_and_selection() {
    let mut backend = area_backend("abcd");

    // Select "bc" (pos 1 -> 3 with anchor 1).
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.send_key(shift(KeyCode::Right)).unwrap();
    backend.send_key(shift(KeyCode::Right)).unwrap();

    // Reject typed replacement, paste replacement, and Backspace delete.
    backend.dispatch(AreaMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abcd");
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abcd");
    backend.send_key(plain(KeyCode::Backspace)).unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Selection "bc" is still intact: typing 'Z' replaces "bc".
    backend.dispatch(AreaMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('Z'))).unwrap();
    assert_eq!(backend.state().value, "aZd");
}

#[test]
fn uncontrolled_text_area_cursor_move_then_rejected_edit_preserves_moved_cursor() {
    let mut backend = area_backend("abcd");

    // Move cursor to pos 2 via arrows, then reject an edit.
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.send_key(plain(KeyCode::Right)).unwrap();
    backend.dispatch(AreaMsg::SetReject(true)).unwrap();
    backend.send_key(plain(KeyCode::Char('x'))).unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Move cursor via mouse click (border=false, padding=0 -> x = 1 lands at pos 1),
    // then reject a paste.
    backend
        .send_mouse(MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend.send_paste("y").unwrap();
    assert_eq!(backend.state().value, "abcd");

    // Accepting an insert lands at pos 1 (where the click moved the cursor).
    backend.dispatch(AreaMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('!'))).unwrap();
    assert_eq!(backend.state().value, "a!bcd");
}

#[test]
fn uncontrolled_text_area_drag_selection_then_rejected_paste_preserves_selection() {
    let mut backend = area_backend("abcd");

    // Drag from col 1 to col 3 (border=false, padding=0) to select "bc".
    backend
        .send_mouse(MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 3,
            y: 0,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 3,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::default(),
        })
        .unwrap();

    backend.dispatch(AreaMsg::SetReject(true)).unwrap();
    backend.send_paste("x").unwrap();
    assert_eq!(backend.state().value, "abcd");

    backend.dispatch(AreaMsg::SetReject(false)).unwrap();
    backend.send_key(plain(KeyCode::Char('Z'))).unwrap();
    assert_eq!(backend.state().value, "aZd");
}
