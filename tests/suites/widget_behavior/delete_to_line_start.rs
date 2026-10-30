//! The `delete_to_line_start` keymap action, driven through the app's real key dispatch into each
//! text widget, including the filter a `SearchPalette` renders for itself.

use std::str::FromStr;
use std::sync::Arc;

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const TARGET: &str = "target";

fn app_with_ctrl_u() -> App {
    App::new()
        .user_keymap_policy(UserKeymapPolicy::Disabled)
        .framework_keymap(FrameworkKeymap::default().bind(
            FrameworkAction::DeleteToLineStart,
            KeyBindings::from_str("ctrl-u").unwrap(),
        ))
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
    let mut backend = TestBackend::new_with_app(app_with_ctrl_u(), BoundTextArea { vim }, ());
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
