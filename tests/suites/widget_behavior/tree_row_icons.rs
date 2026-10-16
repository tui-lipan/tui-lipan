use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const FOLDED: Color = Color::rgb(200, 40, 40);

/// Siblings of every kind under one parent: an always-open group, a plain leaf, a collapsed
/// parent with a tinted arrow, and a leaf with a marker.
struct IconTree;

#[derive(Default)]
struct State {
    unfolded: bool,
    toggles: usize,
}

impl Component for IconTree {
    type Message = TreeToggleEvent;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State::default()
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
                        .disclosure_style(Style::new().fg(FOLDED))
                        .child(TreeNode::new("hidden").icon(Text::new("▲"))),
                )
                .child(TreeNode::new("marked").icon(Text::new("●"))),
        )
        .controlled_expansion(true)
        .indent_style(IndentStyle::Branch)
        .expanded_icon("▼")
        .collapsed_icon("▶")
        .on_toggle(ctx.link().callback(|event| event))
        .into()
    }
}

fn backend() -> TestBackend<IconTree> {
    let mut backend = TestBackend::new(IconTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 8,
    });
    backend.render();
    backend
}

fn row(rows: &[String], label: &str) -> (usize, String) {
    rows.iter()
        .enumerate()
        .find(|(_, row)| row.contains(label))
        .map(|(index, row)| (index, row.trim_end().to_string()))
        .unwrap_or_else(|| panic!("{label} is on screen: {rows:#?}"))
}

#[test]
fn always_open_rows_markers_and_arrows_share_one_label_column() {
    let backend = backend();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    assert_eq!(row(&rows, "group").1, "├─┬ group");
    assert_eq!(row(&rows, "inner").1, "│ └─● inner");
    assert_eq!(row(&rows, "plain").1, "├── plain");
    assert_eq!(row(&rows, "folded").1, "├─▶ folded");
    assert_eq!(row(&rows, "marked").1, "└─● marked");
}

#[test]
fn a_row_arrow_takes_its_own_style_and_still_toggles() {
    let mut backend = backend();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    let (y, _) = row(&rows, "folded");
    let y = y as u16;
    assert_eq!(backend.capture_frame().cell(2, y).fg, FOLDED);

    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: 2,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    backend.render();

    assert_eq!(backend.state().toggles, 1);
    let rows = backend.capture_frame().to_fixed_grid_lines();
    assert_eq!(row(&rows, "folded").1, "├─▼ folded");
    assert_eq!(row(&rows, "hidden").1, "│ └─▲ hidden");
}

/// A heading keeps the old geometry by opting out of the column explicitly.
#[test]
fn an_always_open_heading_without_a_column_hangs_children_under_its_label() {
    struct Heading;
    impl Component for Heading {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            Tree::forest([TreeNode::new("repo").expanded(true).child(
                TreeNode::new("heading")
                    .expansion_mode(TreeExpansionMode::AlwaysOpen)
                    .show_disclosure(false)
                    .child(TreeNode::new("leaf").icon(Text::new("●"))),
            )])
            .indent_style(IndentStyle::Branch)
            .expanded_icon("▼")
            .collapsed_icon("▶")
            .into()
        }
    }
    let mut backend = TestBackend::new(Heading);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    });
    backend.render();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    assert_eq!(row(&rows, "heading").1, "└─ heading");
    assert_eq!(row(&rows, "leaf").1, "   └─● leaf");
}

/// Once any row has a marker, rows without one keep its column: with arrows hidden, and for
/// top-level leaves, which otherwise draw no disclosure column at all.
#[test]
fn rows_without_markers_keep_the_column_with_arrows_hidden_and_at_the_top_level() {
    struct Mixed {
        arrows: bool,
    }
    impl Component for Mixed {
        type Message = ();
        type Properties = ();
        type State = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            Tree::forest([
                TreeNode::new("repo")
                    .expanded(true)
                    .child(
                        TreeNode::new("group")
                            .expansion_mode(TreeExpansionMode::AlwaysOpen)
                            .child(TreeNode::new("inner")),
                    )
                    .child(TreeNode::new("plain"))
                    .child(TreeNode::new("folded").child(TreeNode::new("hidden")))
                    .child(TreeNode::new("marked").icon(Text::new("●"))),
                TreeNode::new("loose"),
                TreeNode::new("flagged").icon(Text::new("▲")),
            ])
            .show_icons(self.arrows)
            .indent_style(IndentStyle::Branch)
            .expanded_icon("▼")
            .collapsed_icon("▶")
            .into()
        }
    }
    for arrows in [true, false] {
        let mut backend = TestBackend::new(Mixed { arrows });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 24,
            h: 10,
        });
        backend.render();
        let rows = backend.capture_frame().to_fixed_grid_lines();
        let column = |label: &str| {
            let (_, line) = row(&rows, label);
            line[..line.find(label).unwrap()].chars().count()
        };
        for label in ["plain", "folded", "marked"] {
            assert_eq!(column(label), column("group"), "arrows={arrows}: {rows:#?}");
        }
        assert_eq!(
            column("loose"),
            column("repo"),
            "arrows={arrows}: {rows:#?}"
        );
        assert_eq!(
            column("flagged"),
            column("repo"),
            "arrows={arrows}: {rows:#?}"
        );
    }
}

/// A parent's extra lines carry the guide from its junction or arrow down to its children.
#[test]
fn two_line_parents_connect_their_column_to_their_children() {
    struct TwoLine;
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
            Tree::forest([TreeNode::new("repo")
                .expanded(true)
                .child(
                    TreeNode::new("group")
                        .content(two_lines("group"))
                        .content_height(2)
                        .expansion_mode(TreeExpansionMode::AlwaysOpen)
                        .child(TreeNode::new("inner")),
                )
                .child(
                    TreeNode::new("folder")
                        .content(two_lines("folder"))
                        .content_height(2)
                        .expanded(true)
                        .child(TreeNode::new("leaf")),
                )])
            .indent_style(IndentStyle::Branch)
            .expanded_icon("▼")
            .collapsed_icon("▶")
            .into()
        }
    }
    let mut backend = TestBackend::new(TwoLine);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 10,
    });
    backend.render();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    for (label, glyph) in [("group", '┬'), ("folder", '▼')] {
        let (y, line) = row(&rows, label);
        let x = line.chars().position(|ch| ch == glyph).unwrap();
        assert_eq!(rows[y + 1].chars().nth(x), Some('│'), "{label}: {rows:#?}");
        assert!(
            matches!(rows[y + 2].chars().nth(x), Some('└' | '├')),
            "{label}'s child hangs from its column: {rows:#?}"
        );
    }
}

/// A composed row without custom content draws its `ListItem` label through an inner list. That
/// list must not keep a click on the label from activating the row.
#[test]
fn clicking_the_label_of_a_row_with_a_marker_activates_it() {
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
    let mut backend = TestBackend::new(Clickable);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    });
    backend.render();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    for (index, label) in ["marked", "plain"].into_iter().enumerate() {
        let (y, line) = row(&rows, label);
        let x = line[..line.find(label).unwrap()].chars().count() + 2;
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
        assert_eq!(backend.state().last(), Some(&index), "{label}");
    }
}
