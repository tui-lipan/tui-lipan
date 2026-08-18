use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct ExcludedFileTree;

#[derive(Default)]
struct State {
    explorer_escapes: usize,
    explorer_focus: Vec<FileTreeExplorerFocusOrigin>,
    explorer_blurs: usize,
}

enum Msg {
    FocusTree,
    ExplorerFocused(FileTreeExplorerFocusOrigin),
    ExplorerBlurred,
    ExplorerEscape,
}

impl Component for ExcludedFileTree {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FocusTree => ctx.request_focus("__ft_tree"),
            Msg::ExplorerFocused(origin) => ctx.state.explorer_focus.push(origin),
            Msg::ExplorerBlurred => ctx.state.explorer_blurs += 1,
            Msg::ExplorerEscape => ctx.state.explorer_escapes += 1,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .border(false)
            .padding(0)
            .focus_scope(FocusScope::Exclude)
            .child(
                FileTree::new("/remote/repo")
                    .entry_source(FileTreeEntrySource::Provided(vec![
                        FileTreeDirectoryListing::new(
                            "/remote/repo",
                            [FileTreeEntry::file("main.rs")],
                        ),
                    ]))
                    .explorer(true)
                    .on_explorer_focus(ctx.link().callback(Msg::ExplorerFocused))
                    .on_explorer_blur(ctx.link().callback(|_| Msg::ExplorerBlurred))
                    .on_explorer_escape(ctx.link().callback(|_| Msg::ExplorerEscape)),
            )
            .into()
    }
}

fn mouse(kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x: 2,
        y: 0,
        kind,
        mods: KeyMods::NONE,
    }
}

fn tree_mouse(kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x: 2,
        y: 2,
        kind,
        mods: KeyMods::NONE,
    }
}

fn settle(backend: &mut TestBackend<ExcludedFileTree>) {
    for _ in 0..3 {
        backend.render();
        let _ = backend.pump();
    }
    backend.render();
}

#[test]
fn clicking_explorer_focuses_it_inside_an_excluded_scope() {
    let mut backend = TestBackend::new(ExcludedFileTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    settle(&mut backend);

    assert!(backend.focused().is_none());
    backend
        .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
        .expect("press explorer input");
    backend
        .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
        .expect("click explorer input");
    settle(&mut backend);

    assert_eq!(backend.focused_key().map(AsRef::as_ref), Some("__ft_input"));
    assert_eq!(
        backend.state().explorer_focus,
        vec![FileTreeExplorerFocusOrigin::Pointer]
    );
    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('m'),
            mods: KeyMods::NONE,
        })
        .expect("type into explorer input");
    settle(&mut backend);
    assert!(backend.capture_frame().plain_text().contains('m'));

    backend
        .send_key(KeyEvent {
            code: KeyCode::Esc,
            mods: KeyMods::NONE,
        })
        .expect("leave pointer-focused explorer");
    settle(&mut backend);
    assert_eq!(backend.state().explorer_escapes, 1);
    assert_eq!(backend.state().explorer_blurs, 0);
}

#[test]
fn slash_entered_explorer_returns_to_tree_without_external_escape() {
    let mut backend = TestBackend::new(ExcludedFileTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    settle(&mut backend);
    backend.dispatch(Msg::FocusTree).expect("focus tree");
    settle(&mut backend);

    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('/'),
            mods: KeyMods::NONE,
        })
        .expect("open explorer from tree");
    settle(&mut backend);
    assert_eq!(backend.focused_key().map(AsRef::as_ref), Some("__ft_input"));
    assert_eq!(
        backend.state().explorer_focus,
        vec![FileTreeExplorerFocusOrigin::Tree]
    );

    backend
        .send_key(KeyEvent {
            code: KeyCode::Esc,
            mods: KeyMods::NONE,
        })
        .expect("return to tree");
    settle(&mut backend);
    assert!(backend.focused().is_some());
    assert_ne!(backend.focused_key().map(AsRef::as_ref), Some("__ft_input"));
    assert_eq!(backend.state().explorer_escapes, 0);
    assert_eq!(backend.state().explorer_blurs, 1);

    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('/'),
            mods: KeyMods::NONE,
        })
        .expect("tree handles explorer shortcut again");
    settle(&mut backend);
    assert_eq!(backend.focused_key().map(AsRef::as_ref), Some("__ft_input"));
}

#[test]
fn enter_commits_explorer_and_focuses_the_tree() {
    let mut backend = TestBackend::new(ExcludedFileTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    settle(&mut backend);
    backend
        .send_mouse(tree_mouse(MouseKind::Down(MouseButton::Left)))
        .expect("press tree");
    backend
        .send_mouse(tree_mouse(MouseKind::Up(MouseButton::Left)))
        .expect("click tree");
    settle(&mut backend);

    // The standalone fixture has no app-level callback, so focus the input directly through its
    // pointer hit before testing the Enter handoff.
    backend
        .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
        .expect("press explorer input");
    backend
        .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
        .expect("click explorer input");
    settle(&mut backend);
    assert_eq!(backend.focused_key().map(AsRef::as_ref), Some("__ft_input"));
    backend
        .send_key(KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        })
        .expect("commit explorer");
    settle(&mut backend);

    assert_eq!(backend.focused_key().map(AsRef::as_ref), Some("__ft_tree"));
    assert_eq!(backend.state().explorer_blurs, 1);
}

#[test]
fn clicking_the_tree_emits_explorer_escape_when_input_is_pointer_focused() {
    let mut backend = TestBackend::new(ExcludedFileTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    settle(&mut backend);
    backend
        .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
        .expect("press explorer input");
    backend
        .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
        .expect("click explorer input");
    settle(&mut backend);

    backend
        .send_mouse(tree_mouse(MouseKind::Down(MouseButton::Left)))
        .expect("press tree outside explorer");
    settle(&mut backend);
    assert_eq!(backend.state().explorer_escapes, 1);
}
