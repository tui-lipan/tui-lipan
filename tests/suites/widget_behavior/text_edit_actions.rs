//! Bindable text-editing actions, driven through the app's real key dispatch into each text
//! widget, including the filter a `SearchPalette` renders for itself.

use std::str::FromStr;
use std::sync::Arc;

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const TARGET: &str = "target";

fn app_binding(action: FrameworkAction, keys: &str) -> App {
    App::new()
        .user_keymap_policy(UserKeymapPolicy::Disabled)
        .framework_keymap(
            FrameworkKeymap::default().bind(action, KeyBindings::from_str(keys).unwrap()),
        )
}

fn app_with_ctrl_u() -> App {
    app_binding(FrameworkAction::DeleteToLineStart, "ctrl-u")
}

fn app_without_binding() -> App {
    App::new().user_keymap_policy(UserKeymapPolicy::Disabled)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(c),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    }
}

fn plain(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

fn shift_left() -> KeyEvent {
    KeyEvent {
        code: KeyCode::Left,
        mods: KeyMods {
            shift: true,
            ..KeyMods::default()
        },
    }
}

fn send<C: Component>(backend: &mut TestBackend<C>, keys: &[KeyEvent]) {
    for key in keys {
        backend.send_key(*key).unwrap();
    }
    backend.render();
}

/// `count` presses of the left arrow.
fn lefts(count: usize) -> Vec<KeyEvent> {
    vec![plain(KeyCode::Left); count]
}

struct Palette {
    initial_query: &'static str,
    claim_ctrl_u: bool,
}

impl Palette {
    fn new(initial_query: &'static str) -> Self {
        Self {
            initial_query,
            claim_ctrl_u: false,
        }
    }
}

impl Component for Palette {
    type Message = Arc<str>;
    type Properties = ();
    type State = Vec<Arc<str>>;

    fn create_state(&self, _: &()) -> Self::State {
        vec![Arc::from(self.initial_query)]
    }

    fn update(&mut self, query: Arc<str>, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(query);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut palette = SearchPalette::new()
            .items([SearchItem::new("hello", 0), SearchItem::new("world", 1)])
            .initial_query(self.initial_query)
            .input_key(TARGET)
            .on_query_change(ctx.link().callback(|query| query))
            .width(Length::Px(40))
            .height(Length::Px(6));
        if self.claim_ctrl_u {
            palette =
                palette.input_key_interceptor(KeyHandler::new(|key: KeyEvent| key == ctrl('u')));
        }
        palette.into()
    }
}

fn palette_backend(app: App, palette: Palette) -> TestBackend<Palette> {
    let mut backend = TestBackend::new_with_app(app, palette, ());
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));
    backend
}

fn query(backend: &TestBackend<Palette>) -> &str {
    backend.state().last().expect("an initial query")
}

/// Cursor moves also report through `on_query_change`, so compare against the reports so far.
fn assert_ctrl_u_changes_nothing(backend: &mut TestBackend<Palette>) {
    let reports = backend.state().clone();
    send(backend, &[ctrl('u')]);
    assert_eq!(backend.state(), &reports);
}

#[test]
fn palette_filter_keeps_text_after_the_cursor_and_refilters() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("helloworld"));
    assert!(backend.capture_frame().plain_text().contains("0/2"));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('u')]);

    assert_eq!(query(&backend), "world");
    let frame = backend.capture_frame().plain_text();
    assert!(frame.contains("1/2"), "filter should match world:\n{frame}");
}

#[test]
fn palette_filter_leaves_the_cursor_where_the_deleted_text_began() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("helloworld"));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('u'), plain(KeyCode::Char('x'))]);

    assert_eq!(query(&backend), "xworld");
}

#[test]
fn palette_filter_removes_only_a_selection() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("helloworld"));

    send(&mut backend, &lefts(3));
    send(&mut backend, &[shift_left(), shift_left(), ctrl('u')]);

    assert_eq!(query(&backend), "hellorld");
}

#[test]
fn palette_filter_keeps_multibyte_text_intact() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("zażółć🦀gęślą"));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('u')]);

    assert_eq!(query(&backend), "gęślą");
}

#[test]
fn palette_filter_undo_and_redo_restore_the_query_and_cursor() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("helloworld"));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('u')]);
    assert_eq!(query(&backend), "world");

    send(&mut backend, &[ctrl('z')]);
    assert_eq!(query(&backend), "helloworld");
    let frame = backend.capture_frame().plain_text();
    assert!(frame.contains("0/2"), "undo should refilter:\n{frame}");

    send(&mut backend, &[ctrl('y')]);
    assert_eq!(query(&backend), "world");

    send(&mut backend, &[ctrl('z'), plain(KeyCode::Char('x'))]);
    assert_eq!(query(&backend), "helloxworld");
}

#[test]
fn palette_filter_at_the_start_reports_no_query_change() {
    let mut backend = palette_backend(app_with_ctrl_u(), Palette::new("hello"));

    send(&mut backend, &lefts(5));
    assert_ctrl_u_changes_nothing(&mut backend);
}

#[test]
fn palette_filter_ignores_ctrl_u_until_an_app_binds_it() {
    let mut backend = palette_backend(app_without_binding(), Palette::new("helloworld"));

    send(&mut backend, &lefts(5));
    assert_ctrl_u_changes_nothing(&mut backend);
}

#[test]
fn palette_input_interceptor_outranks_the_binding() {
    let palette = Palette {
        claim_ctrl_u: true,
        ..Palette::new("helloworld")
    };
    let mut backend = palette_backend(app_with_ctrl_u(), palette);

    send(&mut backend, &lefts(5));
    assert_ctrl_u_changes_nothing(&mut backend);
}

#[test]
fn palette_filter_deletes_to_the_line_end() {
    let app = app_binding(FrameworkAction::DeleteToLineEnd, "ctrl-k");
    let mut backend = palette_backend(app, Palette::new("helloworld"));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('k')]);
    assert_eq!(query(&backend), "hello");
    let frame = backend.capture_frame().plain_text();
    assert!(frame.contains("1/2"), "filter should match hello:\n{frame}");

    send(&mut backend, &[ctrl('z')]);
    assert_eq!(query(&backend), "helloworld");
}

#[test]
fn palette_filter_deletes_a_word_with_a_rebound_existing_action() {
    let app = app_binding(FrameworkAction::DeleteWordLeft, "ctrl-w, alt-backspace");
    let mut backend = palette_backend(app, Palette::new("hello world"));

    send(&mut backend, &[ctrl('w')]);

    assert_eq!(query(&backend), "hello ");
}

#[test]
fn rebound_key_outranks_its_default_action_in_the_filter() {
    let app = app_binding(FrameworkAction::MoveHome, "ctrl-a");
    let mut backend = palette_backend(app, Palette::new("hello"));

    send(&mut backend, &[ctrl('a'), plain(KeyCode::Char('x'))]);

    assert_eq!(query(&backend), "xhello");
}

struct BoundInput {
    mask: Option<char>,
}

impl Component for BoundInput {
    type Message = InputEvent;
    type Properties = ();
    type State = TextInput;

    fn create_state(&self, _: &()) -> TextInput {
        TextInput::new("helloworld")
    }

    fn update(&mut self, event: InputEvent, ctx: &mut Context<Self>) -> Update {
        event.apply_to(&mut ctx.state);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Input::bound(&ctx.state)
            .mask(self.mask)
            .on_change(ctx.link().callback(|event| event))
            .key(TARGET)
    }
}

#[test]
fn plain_and_masked_inputs_keep_text_after_the_cursor() {
    for mask in [None, Some('*')] {
        let mut backend = TestBackend::new_with_app(app_with_ctrl_u(), BoundInput { mask }, ());
        backend.render();
        assert!(backend.focus_key(&Key::from(TARGET)));

        send(&mut backend, &lefts(5));
        send(&mut backend, &[ctrl('u')]);

        assert_eq!(backend.state().text(), "world", "mask {mask:?}");
        assert_eq!(backend.state().cursor(), 0, "mask {mask:?}");
    }
}

#[test]
fn chord_runs_its_action_without_typing_the_final_key() {
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-g u");
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('g'), plain(KeyCode::Char('u'))]);

    assert_eq!(backend.state().text(), "world");
    assert_eq!(backend.state().cursor(), 0);
}

#[test]
fn chord_final_key_does_not_also_run_its_single_key_binding() {
    let app = App::new()
        .user_keymap_policy(UserKeymapPolicy::Disabled)
        .framework_keymap(FrameworkKeymap::default().bind(
            FrameworkAction::Undo,
            KeyBindings::from_str("ctrl-g ctrl-a").unwrap(),
        ));
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &[plain(KeyCode::Char('x'))]);
    send(&mut backend, &[ctrl('g'), ctrl('a')]);

    assert_eq!(backend.state().text(), "helloworld", "undone, not selected");
    assert_eq!(backend.state().anchor(), None);
}

#[test]
fn rust_binding_on_the_copy_key_edits_a_selected_input() {
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-c");
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &[shift_left(), shift_left(), ctrl('c')]);

    assert_eq!(backend.state().text(), "hellowor");
}

#[test]
fn chord_ending_in_the_copy_key_completes_on_a_selected_input() {
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-g ctrl-c");
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &[shift_left(), shift_left()]);
    send(&mut backend, &[ctrl('g'), ctrl('c')]);

    assert_eq!(backend.state().text(), "hellowor");
}

#[test]
fn copy_key_that_breaks_a_chord_cancels_the_chord() {
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-g u");
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &[shift_left(), shift_left()]);
    send(
        &mut backend,
        &[ctrl('g'), ctrl('c'), plain(KeyCode::Char('u'))],
    );

    assert_eq!(backend.state().text(), "helloworu");
}

#[test]
fn chord_for_an_action_a_widget_cannot_run_still_types_nothing() {
    let app = app_binding(FrameworkAction::Copy, "ctrl-g c");
    let mut backend = TestBackend::new_with_app(app, BoundInput { mask: None }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));

    send(&mut backend, &[ctrl('g'), plain(KeyCode::Char('c'))]);

    assert_eq!(backend.state().text(), "helloworld");
}

#[test]
fn chord_does_not_reach_the_filter_interceptor() {
    let palette = Palette {
        claim_ctrl_u: true,
        ..Palette::new("helloworld")
    };
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-g ctrl-u");
    let mut backend = palette_backend(app, palette);

    send(&mut backend, &lefts(5));
    send(&mut backend, &[ctrl('g'), ctrl('u')]);

    assert_eq!(query(&backend), "world");
}

struct BoundTextArea {
    vim: bool,
}

impl Component for BoundTextArea {
    type Message = TextAreaEvent;
    type Properties = ();
    type State = TextEditor;

    fn create_state(&self, _: &()) -> TextEditor {
        let mut editor = TextEditor::new("first\nhelloworld");
        editor.set_cursor("first\nhello".len());
        editor
    }

    fn update(&mut self, event: TextAreaEvent, ctx: &mut Context<Self>) -> Update {
        event.apply_to(&mut ctx.state);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::bound(&ctx.state)
            .vim_motions(self.vim)
            .on_change(ctx.link().callback(|event| event))
            .key(TARGET)
    }
}

fn text_area_backend(vim: bool) -> TestBackend<BoundTextArea> {
    text_area_backend_with(app_with_ctrl_u(), vim)
}

fn text_area_backend_with(app: App, vim: bool) -> TestBackend<BoundTextArea> {
    let mut backend = TestBackend::new_with_app(app, BoundTextArea { vim }, ());
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));
    backend
}

#[test]
fn text_area_deletes_only_within_the_current_line_and_undoes() {
    let mut backend = text_area_backend(false);

    send(&mut backend, &[ctrl('u')]);
    assert_eq!(backend.state().text(), "first\nworld");
    assert_eq!(backend.state().cursor(), 6);

    send(&mut backend, &[ctrl('u')]);
    assert_eq!(backend.state().text(), "first\nworld", "no line join");

    send(&mut backend, &[ctrl('z')]);
    assert_eq!(backend.state().text(), "first\nhelloworld");
    assert_eq!(backend.state().cursor(), 11);
}

#[test]
fn vim_text_area_deletes_only_in_insert_mode() {
    let mut backend = text_area_backend(true);

    send(&mut backend, &[ctrl('u')]);
    assert_eq!(backend.state().text(), "first\nhelloworld", "normal mode");

    send(&mut backend, &[plain(KeyCode::Char('i')), ctrl('u')]);
    assert_eq!(backend.state().text(), "first\nworld", "insert mode");
}

#[test]
fn text_area_deletes_the_current_line_and_undoes() {
    let app = app_binding(FrameworkAction::DeleteLine, "ctrl-shift-d");
    let mut backend = text_area_backend_with(app, false);
    let ctrl_shift_d = KeyEvent {
        code: KeyCode::Char('d'),
        mods: KeyMods {
            ctrl: true,
            shift: true,
            ..KeyMods::default()
        },
    };

    send(&mut backend, &[ctrl_shift_d]);
    assert_eq!(backend.state().text(), "first");

    send(&mut backend, &[ctrl('z')]);
    assert_eq!(backend.state().text(), "first\nhelloworld");
    assert_eq!(backend.state().cursor(), 11);
}

#[test]
fn text_area_buffer_motions_cross_lines() {
    let app = app_binding(FrameworkAction::MoveBufferHome, "ctrl-g");
    let mut backend = text_area_backend_with(app, false);

    send(&mut backend, &[ctrl('g')]);

    assert_eq!(backend.state().cursor(), 0);
}

#[test]
fn vim_normal_mode_ignores_a_chord_edit_and_its_final_key() {
    let app = app_binding(FrameworkAction::DeleteToLineStart, "ctrl-g u");
    let mut backend = text_area_backend_with(app, true);

    send(&mut backend, &[plain(KeyCode::Char('x'))]);
    let after_vim_edit = backend.state().text().to_owned();
    send(&mut backend, &[ctrl('g'), plain(KeyCode::Char('u'))]);

    assert_eq!(
        backend.state().text(),
        after_vim_edit,
        "neither edit nor undo"
    );
}

/// A six-column `TextArea` holding `text` with the cursor at byte `cursor`.
struct WrappedTextArea {
    text: &'static str,
    cursor: usize,
    wrap: bool,
}

impl Component for WrappedTextArea {
    type Message = TextAreaEvent;
    type Properties = ();
    type State = TextEditor;

    fn create_state(&self, _: &()) -> TextEditor {
        let mut editor = TextEditor::new(self.text);
        editor.set_cursor(self.cursor);
        editor
    }

    fn update(&mut self, event: TextAreaEvent, ctx: &mut Context<Self>) -> Update {
        event.apply_to(&mut ctx.state);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::bound(&ctx.state)
            .wrap(self.wrap)
            .width(Length::Px(6))
            .height(Length::Px(6))
            .on_change(ctx.link().callback(|event| event))
            .key(TARGET)
    }
}

fn wrapped_backend(
    app: App,
    text: &'static str,
    cursor: usize,
    wrap: bool,
) -> TestBackend<WrappedTextArea> {
    let mut backend = TestBackend::new_with_app(app, WrappedTextArea { text, cursor, wrap }, ());
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 6,
        h: 6,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from(TARGET)));
    backend.render();
    backend
}

const WRAPS: &str = "abcdefghijklmnopqrstuvwxyz";

/// The byte each wrapped row starts at, found by walking down from the start with the arrow key.
fn wrapped_row_starts(backend: &mut TestBackend<WrappedTextArea>) -> Vec<usize> {
    let mut starts = vec![0];
    loop {
        send(backend, &[plain(KeyCode::Down)]);
        let cursor = backend.state().cursor();
        if cursor >= WRAPS.len() || starts.contains(&cursor) {
            return starts;
        }
        starts.push(cursor);
    }
}

#[test]
fn visual_line_home_and_end_stay_on_the_wrapped_row() {
    let app = App::new()
        .user_keymap_policy(UserKeymapPolicy::Disabled)
        .framework_keymap(
            FrameworkKeymap::default()
                .bind(
                    FrameworkAction::MoveVisualLineHome,
                    KeyBindings::from_str("ctrl-b").unwrap(),
                )
                .bind(
                    FrameworkAction::MoveVisualLineEnd,
                    KeyBindings::from_str("ctrl-e").unwrap(),
                ),
        );
    let mut backend = wrapped_backend(app, WRAPS, 0, true);
    let starts = wrapped_row_starts(&mut backend);
    assert!(starts.len() >= 3, "text should wrap: {starts:?}");
    let (row, next_row) = (starts[1], starts[2]);

    backend.state_mut().set_cursor(row + 1);
    backend.render();
    send(&mut backend, &[ctrl('b')]);
    assert_eq!(backend.state().cursor(), row, "row start, not line start");
    send(&mut backend, &[ctrl('b')]);
    assert_eq!(backend.state().cursor(), row, "stays put when repeated");

    send(&mut backend, &[ctrl('e')]);
    assert_eq!(
        backend.state().cursor(),
        next_row,
        "past the row's last character"
    );
    send(&mut backend, &[ctrl('e')]);
    assert_eq!(backend.state().cursor(), next_row, "stays when repeated");
    send(&mut backend, &[ctrl('b')]);
    assert_eq!(
        backend.state().cursor(),
        row,
        "home returns to the same row"
    );
}

#[test]
fn typing_after_visual_line_end_follows_the_rows_last_character() {
    let app = app_binding(FrameworkAction::MoveVisualLineEnd, "ctrl-e");
    let mut backend = wrapped_backend(app, WRAPS, 0, true);
    let starts = wrapped_row_starts(&mut backend);
    let next_row = starts[2];

    backend.state_mut().set_cursor(starts[1] + 1);
    backend.render();
    send(&mut backend, &[ctrl('e'), plain(KeyCode::Char('!'))]);

    assert_eq!(
        backend.state().text(),
        format!("{}!{}", &WRAPS[..next_row], &WRAPS[next_row..])
    );
}

#[test]
fn visual_line_end_selection_includes_the_rows_last_character() {
    let app = app_binding(FrameworkAction::SelectVisualLineEnd, "ctrl-e");
    let mut backend = wrapped_backend(app, WRAPS, 0, true);
    let starts = wrapped_row_starts(&mut backend);
    let (row, next_row) = (starts[1], starts[2]);

    backend.state_mut().set_cursor(row);
    backend.render();
    send(&mut backend, &[ctrl('e')]);
    assert_eq!(backend.state().selected_text(), Some(&WRAPS[row..next_row]));

    send(&mut backend, &[ctrl('e')]);
    assert_eq!(
        backend.state().selected_text(),
        Some(&WRAPS[row..next_row]),
        "stays when repeated"
    );
}

#[test]
fn visual_line_end_memory_ends_with_any_other_key() {
    let app = app_binding(FrameworkAction::MoveVisualLineEnd, "ctrl-e");
    let mut backend = wrapped_backend(app, WRAPS, 0, true);
    let starts = wrapped_row_starts(&mut backend);

    backend.state_mut().set_cursor(starts[1]);
    backend.render();
    send(&mut backend, &[ctrl('e')]);
    assert_eq!(backend.state().cursor(), starts[2]);

    send(
        &mut backend,
        &[plain(KeyCode::Right), plain(KeyCode::Left), ctrl('e')],
    );
    assert_eq!(
        backend.state().cursor(),
        starts[3],
        "from the start of the next row, end means that row's end"
    );
}

#[test]
fn visual_line_selection_extends_to_the_wrapped_row_edge() {
    let app = app_binding(FrameworkAction::SelectVisualLineHome, "ctrl-b");
    let mut backend = wrapped_backend(app, WRAPS, 0, true);
    let starts = wrapped_row_starts(&mut backend);
    let row = starts[1];

    backend.state_mut().set_cursor(row + 2);
    backend.render();
    send(&mut backend, &[ctrl('b')]);

    assert_eq!(backend.state().cursor(), row);
    assert_eq!(backend.state().anchor(), Some(row + 2));
}

#[test]
fn visual_line_end_reaches_the_end_of_the_final_row() {
    let app = app_binding(FrameworkAction::MoveVisualLineEnd, "ctrl-e");
    let mut backend = wrapped_backend(app, WRAPS, WRAPS.len() - 1, true);

    send(&mut backend, &[ctrl('e')]);

    assert_eq!(backend.state().cursor(), WRAPS.len());
}

#[test]
fn visual_line_motions_fall_back_to_the_line_when_not_wrapping() {
    let app = app_binding(FrameworkAction::MoveVisualLineHome, "ctrl-b");
    let mut backend = wrapped_backend(app, "first\nsecond", 9, false);

    send(&mut backend, &[ctrl('b')]);

    assert_eq!(backend.state().cursor(), 6);
}
