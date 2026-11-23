//! `FileTree::show_root(false)` omits the root directory row and renders top-level entries at
//! depth 0 as a forest while keeping paths, selection, and events rooted at `root`.

use std::cell::RefCell;
use std::rc::Rc;

use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods};
use tui_lipan::prelude::*;
use tui_lipan::{FileTreeChange, FileTreeChangeSource, FileTreeChangeStatus, FileTreeChangeView};

struct ForestTree {
    selected: Rc<RefCell<Vec<String>>>,
    activated: Rc<RefCell<Vec<String>>>,
}

impl Component for ForestTree {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let selected = self.selected.clone();
        let activated = self.activated.clone();
        FileTree::new("/remote/repo")
            .change_source(FileTreeChangeSource::provided([
                FileTreeChange::new("dir1/dir2/nested.rs", FileTreeChangeStatus::Modified),
                FileTreeChange::new("README.md", FileTreeChangeStatus::Added),
            ]))
            .change_view(FileTreeChangeView::ChangedOnly)
            .compact_directories(true)
            .show_root(false)
            .show_icons(false)
            .show_arrows(false)
            .git_status(false)
            .indent_style(IndentStyle::Branch)
            .indent_width(1)
            .expanded_paths(["/remote/repo/dir1/dir2".to_string()])
            .selected_path("README.md")
            .on_select(Callback::new(move |event: FileTreeEvent| {
                selected.borrow_mut().push(event.path.to_string());
            }))
            .on_activate(Callback::new(move |event: FileTreeEvent| {
                activated.borrow_mut().push(event.path.to_string());
            }))
            .into()
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::NONE,
    }
}

#[test]
fn show_root_false_omits_root_row_and_keeps_selection_and_events_aligned() {
    let selected = Rc::new(RefCell::new(Vec::new()));
    let activated = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ForestTree {
        selected: selected.clone(),
        activated: activated.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();

    let lines = backend.capture_frame().to_fixed_grid_lines();
    let text = lines.join("\n");
    assert!(
        !text.contains("/remote/repo") && !text.contains("repo"),
        "root row must not be rendered:\n{text}"
    );
    assert_eq!(lines[0].trim_end(), "dir1/dir2");
    assert_eq!(lines[1].trim_end(), "└ nested.rs");
    assert_eq!(lines[2].trim_end(), "README.md");

    backend.focus_next();
    backend.send_key(key(KeyCode::Enter)).unwrap();
    backend.render();
    assert_eq!(activated.borrow().as_slice(), &["/remote/repo/README.md"]);

    backend.send_key(key(KeyCode::Up)).unwrap();
    backend.render();
    assert_eq!(
        selected.borrow().as_slice(),
        &["/remote/repo/dir1/dir2/nested.rs"]
    );
}

struct UncontrolledChangedTree {
    activated: Rc<RefCell<Vec<String>>>,
}

impl Component for UncontrolledChangedTree {
    type Message = Vec<FileTreeChange>;
    type Properties = ();
    type State = Vec<FileTreeChange>;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        vec![FileTreeChange::new("b.rs", FileTreeChangeStatus::Modified)]
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = msg;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let activated = self.activated.clone();
        FileTree::new("/remote/repo")
            .change_source(FileTreeChangeSource::provided(ctx.state.clone()))
            .change_view(FileTreeChangeView::ChangedOnly)
            .show_root(false)
            .show_icons(false)
            .show_arrows(false)
            .git_status(false)
            .on_activate(Callback::new(move |event: FileTreeEvent| {
                activated.borrow_mut().push(event.path.to_string());
            }))
            .into()
    }
}

#[test]
fn show_root_false_changed_only_preserves_uncontrolled_selection_when_earlier_row_is_inserted() {
    let activated = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(UncontrolledChangedTree {
        activated: activated.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();

    backend
        .dispatch(vec![
            FileTreeChange::new("a.rs", FileTreeChangeStatus::Added),
            FileTreeChange::new("b.rs", FileTreeChangeStatus::Modified),
        ])
        .unwrap();
    backend.render();

    backend.focus_next();
    backend.send_key(key(KeyCode::Enter)).unwrap();
    backend.render();
    assert_eq!(activated.borrow().as_slice(), &["/remote/repo/b.rs"]);
}
