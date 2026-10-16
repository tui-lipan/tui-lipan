//! Tree rows FileTree has no counterpart for: always-open rows, headings, row icons on some rows
//! only, per-row arrow styles, and multi-line parents. `tree_file_tree_parity` covers the shared
//! layout.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const FOLDED: Color = Color::rgb(200, 40, 40);

fn render<C: Component<Properties = ()>>(component: C, w: u16, h: u16) -> TestBackend<C> {
    let mut backend = TestBackend::new(component);
    backend.set_viewport(Rect { x: 0, y: 0, w, h });
    backend.render();
    backend
}

fn lines<C: Component>(backend: &TestBackend<C>) -> Vec<String> {
    backend
        .capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

fn row(rows: &[String], label: &str) -> (usize, String) {
    rows.iter()
        .enumerate()
        .find(|(_, row)| row.contains(label))
        .map(|(index, row)| (index, row.clone()))
        .unwrap_or_else(|| panic!("{label} is on screen: {rows:#?}"))
}

fn click<C: Component>(backend: &mut TestBackend<C>, x: u16, y: u16) {
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    backend.render();
}

/// Siblings of every kind under one parent: an always-open group, a plain leaf, a collapsed
/// parent with its own arrow style, and a leaf with an icon.
struct Siblings {
    arrows: bool,
}

#[derive(Default)]
struct SiblingsState {
    unfolded: bool,
    toggles: usize,
}

impl Component for Siblings {
    type Message = TreeToggleEvent;
    type Properties = ();
    type State = SiblingsState;

    fn create_state(&self, _: &()) -> SiblingsState {
        SiblingsState::default()
    }

    fn update(&mut self, event: TreeToggleEvent, ctx: &mut Context<Self>) -> Update {
        ctx.state.toggles += 1;
        ctx.state.unfolded = event.expanded;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Tree::new(
            TreeNode::new("project")
                .expanded(true)
                .child(
                    TreeNode::new("group")
                        .expansion_mode(TreeExpansionMode::AlwaysOpen)
                        .child(TreeNode::new("inner").icon(Text::new("●"))),
                )
                .child(TreeNode::new("plain"))
                .child(
                    TreeNode::new("folded")
                        .expanded(ctx.state.unfolded)
                        .arrow_style(Style::new().fg(FOLDED))
                        .child(TreeNode::new("hidden").icon(Text::new("▲"))),
                )
                .child(TreeNode::new("marked").icon(Text::new("●"))),
        )
        .controlled_expansion(true)
        .indent_style(IndentStyle::Branch)
        .show_arrows(self.arrows)
        .on_toggle(ctx.link().callback(|event| event))
        .into()
    }
}

#[test]
fn every_kind_of_sibling_starts_its_label_in_one_column() {
    let backend = render(Siblings { arrows: true }, 24, 8);
    let rows = lines(&backend);
    assert_eq!(row(&rows, "project").1, "▼   project");
    // The always-open row keeps the arrow column a leaf has, and the branch running through it
    // turns down into its children.
    assert_eq!(row(&rows, "group").1, "├─┬   group");
    assert_eq!(row(&rows, "inner").1, "│ └── ● inner");
    assert_eq!(row(&rows, "plain").1, "├──   plain");
    assert_eq!(row(&rows, "folded").1, "├─▶   folded");
    assert_eq!(row(&rows, "marked").1, "└── ● marked");
}

/// `leaf_arrow` marks rows with nothing to expand. An always-open row is a parent, not a leaf: it
/// keeps its junction, in text-only trees and in trees with an icon column alike.
#[test]
fn an_always_open_row_keeps_its_junction_when_leaves_have_an_arrow() {
    struct LeafArrow {
        icons: bool,
    }
    impl Component for LeafArrow {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            let mut inner = TreeNode::new("inner");
            if self.icons {
                inner = inner.icon(Text::new("●"));
            }
            Tree::new(
                TreeNode::new("project")
                    .expanded(true)
                    .child(
                        TreeNode::new("group")
                            .expansion_mode(TreeExpansionMode::AlwaysOpen)
                            .child(inner),
                    )
                    .child(TreeNode::new("plain")),
            )
            .indent_style(IndentStyle::Branch)
            .leaf_arrow(Some("•"))
            .into()
        }
    }
    let text = lines(&render(LeafArrow { icons: false }, 24, 6));
    assert_eq!(row(&text, "group").1, "├─┬ group");
    assert_eq!(row(&text, "inner").1, "│ └─• inner");
    assert_eq!(row(&text, "plain").1, "└─• plain");

    let composed = lines(&render(LeafArrow { icons: true }, 24, 6));
    assert_eq!(row(&composed, "group").1, "├─┬   group");
    assert_eq!(row(&composed, "inner").1, "│ └─• ● inner");
    assert_eq!(row(&composed, "plain").1, "└─•   plain");
}

#[test]
fn hidden_arrows_remove_the_column_from_every_row_and_keep_labels_aligned() {
    let backend = render(Siblings { arrows: false }, 24, 8);
    let rows = lines(&backend);
    assert_eq!(row(&rows, "project").1, "  project");
    for label in ["group", "plain", "folded"] {
        assert_eq!(row(&rows, label).1, format!("├─   {label}"));
    }
    assert_eq!(row(&rows, "marked").1, "└─ ● marked");
    // Without an arrow, the group's children hang beneath its icon column.
    assert_eq!(row(&rows, "inner").1, "│  └─ ● inner");
}

#[test]
fn a_row_arrow_takes_its_own_style_and_still_toggles() {
    let mut backend = render(Siblings { arrows: true }, 24, 8);
    let (y, line) = row(&lines(&backend), "folded");
    let x = line.chars().position(|ch| ch == '▶').unwrap() as u16;
    assert_eq!(backend.capture_frame().cell(x, y as u16).fg, FOLDED);

    click(&mut backend, x, y as u16);

    assert_eq!(backend.state().toggles, 1);
    let rows = lines(&backend);
    assert_eq!(row(&rows, "folded").1, "├─▼   folded");
    assert_eq!(row(&rows, "hidden").1, "│ └── ▲ hidden");
}

/// A heading has no arrow column: its label starts where its siblings' arrows do, and its
/// children hang beneath its first glyph, its icon or label, at any depth.
#[test]
fn headings_hang_their_children_beneath_their_first_glyph() {
    struct Headings;
    impl Component for Headings {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            let heading = |label: &'static str| {
                TreeNode::new(label)
                    .expansion_mode(TreeExpansionMode::AlwaysOpen)
                    .show_arrow(false)
            };
            Tree::forest([heading("repo")
                .child(heading("section").child(TreeNode::new("leaf").icon(Text::new("●"))))])
            .indent_style(IndentStyle::Branch)
            .into()
        }
    }
    let rows = lines(&render(Headings, 24, 4));
    assert_eq!(row(&rows, "repo").1, "  repo");
    assert_eq!(row(&rows, "section").1, "└─   section");
    // The leaf hangs beneath the section's icon column, its first glyph.
    assert_eq!(row(&rows, "leaf").1, "   └── ● leaf");
}

/// A parent's extra lines carry the guide from its arrow, junction, or heading label down to its
/// first child.
#[test]
fn two_line_parents_connect_to_their_children() {
    struct TwoLine {
        arrows: bool,
    }
    impl Component for TwoLine {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            let two_lines = |label: &'static str| {
                VStack::new()
                    .gap(0)
                    .child(Text::new(label))
                    .child(Text::new("detail"))
            };
            // Rows with icons: where a parent has no arrow, its children hang beneath its icon,
            // and the icon column is free on the parent's second line.
            let parent = |label: &'static str| {
                TreeNode::new(label)
                    .icon(Text::new("●"))
                    .content(two_lines(label))
                    .content_height(2)
            };
            Tree::forest([TreeNode::new("repo")
                .expanded(true)
                .child(
                    parent("group")
                        .expansion_mode(TreeExpansionMode::AlwaysOpen)
                        .child(TreeNode::new("inner")),
                )
                .child(parent("folder").expanded(true).child(TreeNode::new("leaf")))
                .child(
                    parent("section")
                        .expansion_mode(TreeExpansionMode::AlwaysOpen)
                        .show_arrow(false)
                        .child(TreeNode::new("item")),
                )])
            .indent_style(IndentStyle::Branch)
            .show_arrows(self.arrows)
            .into()
        }
    }
    for arrows in [true, false] {
        let rows = lines(&render(TwoLine { arrows }, 24, 12));
        for (parent, child) in [("group", "inner"), ("folder", "leaf"), ("section", "item")] {
            let (y, _) = row(&rows, parent);
            let (child_y, child_line) = row(&rows, child);
            assert_eq!(child_y, y + 2, "arrows={arrows}: {rows:#?}");
            let x = child_line.chars().position(|ch| ch == '└').unwrap();
            assert_eq!(
                rows[y + 1].chars().nth(x),
                Some('│'),
                "{parent}'s second line leads to {child}, arrows={arrows}: {rows:#?}"
            );
        }
    }
}

/// A composed row without custom content draws its `ListItem` label through an inner list. That
/// list must not keep a click on the label from activating the row.
#[test]
fn clicking_the_label_of_a_row_with_an_icon_activates_it() {
    struct Clickable;
    impl Component for Clickable {
        type Message = TreeEvent;
        type Properties = ();
        type State = Vec<usize>;
        fn create_state(&self, _: &()) -> Vec<usize> {
            Vec::new()
        }
        fn update(&mut self, event: TreeEvent, ctx: &mut Context<Self>) -> Update {
            ctx.state.push(event.index);
            Update::full()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            Tree::forest([
                TreeNode::new("marked").icon(Text::new("●")),
                TreeNode::new("plain"),
            ])
            .on_activate(ctx.link().callback(|event| event))
            .into()
        }
    }
    let mut backend = render(Clickable, 24, 4);
    let rows = lines(&backend);
    for (index, label) in ["marked", "plain"].into_iter().enumerate() {
        let (y, line) = row(&rows, label);
        let x = line[..line.find(label).unwrap()].chars().count() + 2;
        click(&mut backend, x as u16, y as u16);
        assert_eq!(backend.state().last(), Some(&index), "{label}");
    }
}

#[test]
fn hidden_icons_remove_the_icon_column() {
    struct Icons;
    impl Component for Icons {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            Tree::forest([
                TreeNode::new("marked").icon(Text::new("●")),
                TreeNode::new("plain"),
            ])
            .show_icons(false)
            .into()
        }
    }
    let rows = lines(&render(Icons, 24, 4));
    assert_eq!(row(&rows, "marked").1, "  marked");
    assert_eq!(row(&rows, "plain").1, "  plain");
}
