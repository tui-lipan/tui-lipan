//! A Tree pages with PageUp/PageDown like any list, and a row with no arrow to click expands and
//! collapses on a click of its own.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

/// Thirty leaves in composed rows, so the Tree draws through its own scroll view.
struct Rows;

impl Component for Rows {
    type Message = TreeEvent;
    type Properties = ();
    type State = Option<usize>;
    fn create_state(&self, _: &()) -> Option<usize> {
        None
    }
    fn update(&mut self, event: TreeEvent, ctx: &mut Context<Self>) -> Update {
        ctx.state = Some(event.index);
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        Tree::forest((0..30).map(|index| {
            let label = format!("row {index}");
            TreeNode::new(label.clone()).content(Text::new(label))
        }))
        .focus_key("rows")
        .on_select(ctx.link().callback(|event| event))
        .into()
    }
}

#[test]
fn page_down_moves_a_composed_tree_selection_by_a_page() {
    let mut backend = TestBackend::new(Rows);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 10,
    });
    backend.render();
    assert!(backend.focus_key(&"rows".into()));
    backend
        .send_key(KeyEvent {
            code: KeyCode::PageDown,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    let selected = backend.state().expect("PageDown selects a row");
    assert!(
        (5..=10).contains(&selected),
        "one page down from the first row lands about a viewport further: {selected}"
    );
    backend
        .send_key(KeyEvent {
            code: KeyCode::PageUp,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert_eq!(*backend.state(), Some(0), "and PageUp pages back");
}

#[test]
fn moving_down_a_composed_tree_scrolls_only_as_the_cursor_nears_the_bottom() {
    let mut backend = TestBackend::new(Rows);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 10,
    });
    backend.render();
    assert!(backend.focus_key(&"rows".into()));
    let down = KeyEvent {
        code: KeyCode::Down,
        mods: KeyMods::NONE,
    };
    for _ in 0..8 {
        backend.send_key(down).unwrap();
        backend.render();
    }
    assert_eq!(*backend.state(), Some(8));
    let frame = backend.capture_frame().plain_text();
    let top = frame.lines().next().unwrap_or_default().trim().to_owned();
    assert_eq!(
        top, "row 1",
        "the cursor keeps two rows below it instead of jumping to the top:\n{frame}"
    );
    for _ in 0..3 {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Up,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    let frame = backend.capture_frame().plain_text();
    assert_eq!(
        frame.lines().next().unwrap_or_default().trim(),
        "row 1",
        "moving back up inside the viewport leaves it still:\n{frame}"
    );
}

/// A parent with two children, collapsed. `composed` draws rows through composed content. Counts
/// the expansion changes and Ctrl activations it is told about.
#[derive(Clone, Copy)]
struct Folder {
    arrows: bool,
    composed: bool,
    toggle_on_activate: bool,
    activate_on_click: bool,
    ctrl: bool,
}

impl Folder {
    fn new(arrows: bool, composed: bool, toggle_on_activate: bool) -> Self {
        Self {
            arrows,
            composed,
            toggle_on_activate,
            activate_on_click: true,
            ctrl: false,
        }
    }
}

#[derive(Default)]
struct Seen {
    toggles: usize,
    ctrl_activations: usize,
}

enum Event {
    Toggled,
    CtrlActivated,
}

impl Component for Folder {
    type Message = Event;
    type Properties = ();
    type State = Seen;
    fn create_state(&self, _: &()) -> Seen {
        Seen::default()
    }
    fn update(&mut self, event: Event, ctx: &mut Context<Self>) -> Update {
        match event {
            Event::Toggled => ctx.state.toggles += 1,
            Event::CtrlActivated => ctx.state.ctrl_activations += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let node = |label: &'static str| {
            let node = TreeNode::new(label);
            if self.composed {
                node.content(Text::new(label))
            } else {
                node
            }
        };
        let mut tree = Tree::forest([node("folder").child(node("first")).child(node("second"))])
            .show_arrows(self.arrows)
            .toggle_on_activate(self.toggle_on_activate)
            .activate_on_click(self.activate_on_click)
            .on_toggle(ctx.link().callback(|_| Event::Toggled));
        if self.ctrl {
            tree = tree.on_ctrl_activate(ctx.link().callback(|_| Event::CtrlActivated));
        }
        tree.into()
    }
}

fn click_label(backend: &mut TestBackend<Folder>, label: &str) {
    click_with(backend, label, KeyMods::NONE);
}

fn click_with(backend: &mut TestBackend<Folder>, label: &str, mods: KeyMods) {
    let lines = backend.capture_frame().to_fixed_grid_lines();
    let (y, line) = lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap_or_else(|| panic!("{label} is on screen: {lines:#?}"));
    let x = line[..line.find(label).unwrap()].chars().count() + 1;
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: x as u16,
                y: y as u16,
                kind,
                mods,
            })
            .unwrap();
    }
    backend.render();
}

fn shows(backend: &TestBackend<Folder>, label: &str) -> bool {
    backend.capture_frame().plain_text().contains(label)
}

fn backend(folder: Folder) -> TestBackend<Folder> {
    let mut backend = TestBackend::new(folder);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 6,
    });
    backend.render();
    backend
}

#[test]
fn with_arrows_hidden_a_click_expands_and_collapses_a_row() {
    for composed in [false, true] {
        for toggle_on_activate in [false, true] {
            let mut backend = backend(Folder::new(false, composed, toggle_on_activate));
            let context = format!("composed={composed} toggle_on_activate={toggle_on_activate}");
            assert!(!shows(&backend, "first"), "{context}");
            click_label(&mut backend, "folder");
            assert!(shows(&backend, "first"), "one click expands: {context}");
            click_label(&mut backend, "folder");
            assert!(!shows(&backend, "first"), "the next collapses: {context}");
        }
    }
}

#[test]
fn with_arrows_shown_the_arrow_alone_expands_a_row_that_does_not_toggle_on_activate() {
    for composed in [false, true] {
        let mut backend = backend(Folder::new(true, composed, false));
        click_label(&mut backend, "folder");
        assert!(
            !shows(&backend, "first"),
            "the label activates without expanding, composed={composed}"
        );
    }
}

/// Ctrl+click is Ctrl activation and only that, arrow or no arrow: it never also folds the row.
#[test]
fn with_arrows_hidden_a_ctrl_click_activates_without_expanding() {
    for composed in [false, true] {
        let mut backend = backend(Folder {
            ctrl: true,
            ..Folder::new(false, composed, false)
        });
        click_with(&mut backend, "folder", KeyMods::CTRL);
        assert_eq!(backend.state().ctrl_activations, 1, "composed={composed}");
        assert_eq!(backend.state().toggles, 0, "composed={composed}");
        assert!(!shows(&backend, "first"), "composed={composed}");
    }
}

/// Without click activation, a double-click is one activation. Each of its clicks folds the row
/// once: the first by the click, the second by the activation, never by both.
#[test]
fn with_arrows_hidden_each_click_of_a_double_click_folds_the_row_once() {
    let mut backend = backend(Folder {
        activate_on_click: false,
        ..Folder::new(false, false, true)
    });
    click_label(&mut backend, "folder");
    assert_eq!(backend.state().toggles, 1, "the first click expands");
    click_label(&mut backend, "folder");
    assert_eq!(
        backend.state().toggles,
        2,
        "the second click's activation collapses, and the click itself adds nothing"
    );
    assert!(!shows(&backend, "first"));
}
