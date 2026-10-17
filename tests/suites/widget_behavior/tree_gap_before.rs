//! `TreeNode::gap_before` sets a top-level node apart with space that is not a row: it draws blank, takes
//! no hover, and leaves row indices as they are, in text and composed trees alike.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const HOVER: Color = Color::Rgb(70, 70, 70);

/// Two groups, `alpha` over `one` and `beta` over `two`, a line apart.
struct Groups {
    composed: bool,
}

impl Component for Groups {
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
        let node = |label: &'static str| {
            let node = TreeNode::new(label);
            if self.composed {
                node.content(Text::new(label))
            } else {
                node
            }
        };
        Tree::forest([
            node("alpha").expanded(true).child(node("one")),
            node("beta")
                .expanded(true)
                .gap_before(1)
                .child(node("two").gap_before(1)),
        ])
        .show_arrows(false)
        .item_hover_style(Style::new().bg(HOVER))
        .focus_key("groups")
        .on_select(ctx.link().callback(|event| event))
        .width(Length::Px(20))
        .height(Length::Px(8))
        .into()
    }
}

fn backend(composed: bool) -> TestBackend<Groups> {
    let mut backend = TestBackend::new(Groups { composed });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 8,
    });
    backend.render();
    backend
}

fn line_of(backend: &TestBackend<Groups>, label: &str) -> u16 {
    let lines = backend.capture_frame().to_fixed_grid_lines();
    lines
        .iter()
        .position(|line| line.contains(label))
        .unwrap_or_else(|| panic!("{label} is on screen: {lines:#?}")) as u16
}

fn pointer(backend: &mut TestBackend<Groups>, y: u16, kinds: &[MouseKind]) {
    for kind in kinds {
        backend
            .send_mouse(MouseEvent {
                x: 4,
                y,
                kind: *kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    backend.render();
}

fn hovered(backend: &TestBackend<Groups>) -> bool {
    let frame = backend.capture_frame();
    (0..8).any(|y| (0..20).any(|x| frame.cell(x, y).bg == HOVER))
}

#[test]
fn a_gap_before_is_a_blank_line_above_a_top_level_node_only() {
    for composed in [false, true] {
        let backend = backend(composed);
        let lines = backend.capture_frame().to_fixed_grid_lines();
        let context = format!("composed={composed}: {lines:#?}");
        assert_eq!(
            line_of(&backend, "alpha"),
            0,
            "no gap before the first: {context}"
        );
        assert_eq!(
            line_of(&backend, "one"),
            1,
            "none under a node, nor below the top level: {context}"
        );
        assert!(lines[2].trim().is_empty(), "{context}");
        assert_eq!(line_of(&backend, "beta"), 3, "{context}");
        assert_eq!(line_of(&backend, "two"), 4, "{context}");
    }
}

#[test]
fn the_pointer_over_a_gap_hovers_nothing() {
    for composed in [false, true] {
        let mut backend = backend(composed);
        // `one`, not `alpha`: a text tree starts with its first row selected, which outranks hover.
        pointer(&mut backend, 1, &[MouseKind::Moved]);
        assert!(hovered(&backend), "a row hovers, composed={composed}");
        pointer(&mut backend, 2, &[MouseKind::Moved]);
        assert!(
            !hovered(&backend),
            "the gap does not, composed={composed}: {:#?}",
            backend.capture_frame().to_fixed_grid_lines()
        );
    }
}

#[test]
fn rows_keep_their_tree_indices_across_a_gap() {
    let click = [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ];
    for composed in [false, true] {
        let mut backend = backend(composed);
        let beta = line_of(&backend, "beta");
        pointer(&mut backend, beta, &click);
        assert_eq!(*backend.state(), Some(2), "clicked, composed={composed}");

        let one = line_of(&backend, "one");
        pointer(&mut backend, one, &click);
        assert_eq!(*backend.state(), Some(1), "composed={composed}");
        assert!(backend.focus_key(&"groups".into()));
        backend
            .send_key(KeyEvent {
                code: KeyCode::Down,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
        assert_eq!(
            *backend.state(),
            Some(2),
            "Down steps over the gap, composed={composed}"
        );
    }
}

/// Ten one-line roots with a two-line gap above the fourth, in a five-line viewport.
struct Pager {
    composed: bool,
}

impl Component for Pager {
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
        Tree::forest((0..10).map(|index| {
            let label = format!("row {index}");
            let node = TreeNode::new(label.clone()).gap_before(if index == 3 { 2 } else { 0 });
            if self.composed {
                node.content(Text::new(label))
            } else {
                node
            }
        }))
        .focus_key("pager")
        .on_select(ctx.link().callback(|event| event))
        .into()
    }
}

/// Where each of PageDown, PageDown, PageUp, PageUp lands, from the first row.
fn page(composed: bool) -> Vec<usize> {
    let mut backend = TestBackend::new(Pager { composed });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 5,
    });
    backend.render();
    assert!(backend.focus_key(&"pager".into()));
    let mut seen = Vec::new();
    for code in [
        KeyCode::PageDown,
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::PageUp,
    ] {
        backend
            .send_key(KeyEvent {
                code,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
        seen.push(backend.state().expect("paging selects a row"));
    }
    seen
}

/// A page is a viewport of lines, gaps included, so paging across a gap lands on the same row in
/// both render paths.
#[test]
fn paging_counts_a_gap_as_the_lines_it_takes() {
    // Down from row 0: rows 1 and 2, then the two-line gap, fill the page to row 3. Up from row 7
    // crosses the same gap on the way back.
    assert_eq!(page(false), [3, 7, 3, 1], "text tree");
    assert_eq!(page(true), [3, 7, 3, 1], "composed tree");
}
