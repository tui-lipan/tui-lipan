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

/// A parent with two children, collapsed. `composed` draws rows through composed content.
#[derive(Clone, Copy)]
struct Folder {
    arrows: bool,
    composed: bool,
    toggle_on_activate: bool,
}

impl Component for Folder {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let node = |label: &'static str| {
            let node = TreeNode::new(label);
            if self.composed {
                node.content(Text::new(label))
            } else {
                node
            }
        };
        Tree::forest([node("folder").child(node("first")).child(node("second"))])
            .show_arrows(self.arrows)
            .toggle_on_activate(self.toggle_on_activate)
            .into()
    }
}

fn click_label(backend: &mut TestBackend<Folder>, label: &str) {
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
                mods: KeyMods::NONE,
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
            let mut backend = backend(Folder {
                arrows: false,
                composed,
                toggle_on_activate,
            });
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
        let mut backend = backend(Folder {
            arrows: true,
            composed,
            toggle_on_activate: false,
        });
        click_label(&mut backend, "folder");
        assert!(
            !shows(&backend, "first"),
            "the label activates without expanding, composed={composed}"
        );
    }
}
