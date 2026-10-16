use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct ExpansionTree {
    composed: bool,
    controlled: bool,
    content_gap: u16,
    indent: u16,
    icons: bool,
    /// Whether the group keeps its disclosure column. Off makes it a heading.
    column: bool,
}
struct State {
    mode: TreeExpansionMode,
    group_open: bool,
    root_open: bool,
    loaded: bool,
    selected: usize,
    toggles: usize,
    activations: usize,
}
enum Msg {
    Select(usize),
    Toggle(TreeToggleEvent),
    Activate,
}
impl Component for ExpansionTree {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State {
            mode: TreeExpansionMode::AlwaysOpen,
            group_open: false,
            root_open: true,
            loaded: true,
            selected: 1,
            toggles: 0,
            activations: 0,
        }
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(index) => ctx.state.selected = index,
            Msg::Toggle(event) => {
                ctx.state.toggles += 1;
                if event.path.segments().is_empty() {
                    ctx.state.root_open = event.expanded;
                } else if event.path.segments() == [0] {
                    ctx.state.group_open = event.expanded;
                }
            }
            Msg::Activate => ctx.state.activations += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut group = TreeNode::new("group")
            .indent(self.indent)
            .expansion_mode(ctx.state.mode)
            .show_disclosure(self.column)
            .expanded(ctx.state.group_open);
        if self.composed {
            group = group.content(Text::new("group"));
        }
        if ctx.state.loaded {
            group = group.child(TreeNode::new("leaf").indent(self.indent));
        }
        Tree::new(
            TreeNode::new("root")
                .expanded(ctx.state.root_open)
                .children([
                    group,
                    TreeNode::new("sibling")
                        .indent(self.indent)
                        .child(TreeNode::new("sibling-child")),
                ]),
        )
        .content_gap(self.content_gap)
        .show_icons(self.icons)
        .controlled_expansion(self.controlled)
        .selected(ctx.state.selected)
        .indent_style(IndentStyle::Branch)
        .focus_key("expansion-tree")
        .on_select(
            ctx.link()
                .callback(|event: TreeEvent| Msg::Select(event.index)),
        )
        .on_toggle(ctx.link().callback(Msg::Toggle))
        .on_activate(ctx.link().callback(|_| Msg::Activate))
        .into()
    }
}
fn backend(composed: bool, controlled: bool) -> TestBackend<ExpansionTree> {
    let mut backend = TestBackend::new(ExpansionTree {
        composed,
        controlled,
        content_gap: 1,
        indent: 2,
        icons: true,
        column: true,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    });
    backend.render();
    backend.focus_key(&"expansion-tree".into());
    backend
}
fn key(backend: &mut TestBackend<ExpansionTree>, code: KeyCode) {
    backend
        .send_key(KeyEvent {
            code,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
}
fn lines(backend: &TestBackend<ExpansionTree>) -> Vec<String> {
    backend.capture_frame().to_fixed_grid_lines()
}

#[test]
fn always_open_nodes_keep_guides_and_children_without_disclosure_or_toggle_events() {
    for composed in [false, true] {
        for controlled in [false, true] {
            let mut backend = backend(composed, controlled);
            let rows = lines(&backend);
            assert!(rows[1].starts_with("├── group"), "{rows:?}");
            assert!(
                rows[2].contains("leaf"),
                "expanded(false) cannot close an AlwaysOpen group"
            );
            for code in [KeyCode::Char(' '), KeyCode::Enter] {
                key(&mut backend, code);
            }
            let y = lines(&backend)
                .iter()
                .position(|row| row.contains("group"))
                .unwrap() as u16;
            for kind in [
                MouseKind::Down(MouseButton::Left),
                MouseKind::Up(MouseButton::Left),
            ] {
                backend
                    .send_mouse(MouseEvent {
                        x: 3,
                        y,
                        kind,
                        mods: KeyMods::NONE,
                    })
                    .unwrap();
                backend.render();
            }
            assert_eq!(
                backend.state().activations,
                2,
                "Enter and row click still activate"
            );
            assert_eq!(backend.state().toggles, 0);
            key(&mut backend, KeyCode::Right);
            assert_eq!(backend.state().selected, 2, "Right enters the group");
            key(&mut backend, KeyCode::Left);
            assert_eq!(
                backend.state().selected,
                1,
                "Left on child returns to group"
            );
            key(&mut backend, KeyCode::Left);
            assert_eq!(backend.state().selected, 0, "Left on group goes to parent");
            assert_eq!(backend.state().toggles, 0);
            key(&mut backend, KeyCode::Left);
            assert_eq!(
                backend.state().toggles,
                1,
                "its collapsible ancestor can still close"
            );
            assert!(!lines(&backend).iter().any(|row| row.contains("leaf")));
        }
    }
}

#[test]
fn expansion_modes_handle_empty_nodes_lazy_children_and_live_mode_changes() {
    for composed in [false, true] {
        for controlled in [false, true] {
            let mut backend = backend(composed, controlled);
            backend.state_mut().loaded = false;
            backend.render();
            assert!(lines(&backend)[1].starts_with("├── group"));
            key(&mut backend, KeyCode::Char(' '));
            assert_eq!(backend.state().toggles, 0);
            backend.state_mut().loaded = true;
            backend.render();
            assert!(lines(&backend).iter().any(|row| row.contains("leaf")));
            backend.state_mut().mode = TreeExpansionMode::Auto;
            backend.render();
            assert!(!lines(&backend).iter().any(|row| row.contains("leaf")));
            key(&mut backend, KeyCode::Right);
            assert!(lines(&backend).iter().any(|row| row.contains("leaf")));
            key(&mut backend, KeyCode::Left);
            assert!(!lines(&backend).iter().any(|row| row.contains("leaf")));
            backend.state_mut().mode = TreeExpansionMode::Collapsible;
            backend.state_mut().loaded = false;
            backend.render();
            assert!(
                lines(&backend)[1].contains("▶ group"),
                "lazy empty parents retain their arrow"
            );
            key(&mut backend, KeyCode::Right);
            assert!(lines(&backend)[1].contains("▼ group"));
            backend.state_mut().mode = TreeExpansionMode::AlwaysOpen;
            backend.render();
            let toggles = backend.state().toggles;
            key(&mut backend, KeyCode::Char(' '));
            assert_eq!(backend.state().toggles, toggles);
            assert!(lines(&backend)[1].starts_with("├── group"));
        }
    }
}

/// Every combination the geometry tests sweep: renderer, expansion ownership, icons, gap, indent.
fn geometries(column: bool) -> impl Iterator<Item = ExpansionTree> {
    [false, true].into_iter().flat_map(move |composed| {
        [false, true].into_iter().flat_map(move |controlled| {
            [false, true].into_iter().flat_map(move |icons| {
                [0, 1, 3].into_iter().flat_map(move |content_gap| {
                    [0, 1, 2, 4].into_iter().map(move |indent| ExpansionTree {
                        composed,
                        controlled,
                        content_gap,
                        indent,
                        icons,
                        column,
                    })
                })
            })
        })
    })
}

#[test]
fn an_always_open_row_lines_up_with_a_collapsible_sibling_in_both_renderers() {
    for tree in geometries(true) {
        let context = format!(
            "composed={} controlled={} icons={} gap={} indent={}",
            tree.composed, tree.controlled, tree.icons, tree.content_gap, tree.indent
        );
        let mut backend = TestBackend::new(tree);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        });
        backend.render();
        let rows = lines(&backend);
        let label = |name: &str| {
            let row = rows.iter().find(|row| row.contains(name)).unwrap();
            row[..row.find(name).unwrap()].chars().count()
        };
        assert_eq!(label("group"), label("sibling"), "{context}: {rows:?}");
    }
}

#[test]
fn always_open_heading_child_guides_start_under_its_label_in_both_renderers() {
    for composed in [false, true] {
        for controlled in [false, true] {
            for icons in [false, true] {
                for content_gap in [0, 1, 3] {
                    for indent in [0, 1, 2, 4] {
                        let mut backend = TestBackend::new(ExpansionTree {
                            composed,
                            controlled,
                            content_gap,
                            indent,
                            icons,
                            column: false,
                        });
                        backend.set_viewport(Rect {
                            x: 0,
                            y: 0,
                            w: 40,
                            h: 8,
                        });
                        backend.render();
                        let rows = lines(&backend);
                        if indent == 0 {
                            assert!(!rows[2].contains('└'), "{rows:?}");
                        } else {
                            let label_column = rows[1].chars().position(|c| c == 'g').unwrap();
                            let guide_column = rows[2].chars().position(|c| c == '└').unwrap();
                            assert_eq!(
                                guide_column, label_column,
                                "composed={composed} controlled={controlled} icons={icons} gap={content_gap} indent={indent}: {rows:?}"
                            );
                            assert!(
                                rows[2].starts_with('│'),
                                "ancestor guide stays in place: {rows:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mode_changes_preserve_uncontrolled_user_expansion_of_siblings() {
    for composed in [false, true] {
        for initial_mode in [TreeExpansionMode::Auto, TreeExpansionMode::AlwaysOpen] {
            let mut backend = backend(composed, false);
            backend.state_mut().mode = initial_mode;
            backend.render();
            let sibling_index = lines(&backend)
                .iter()
                .position(|row| row.contains("sibling"))
                .unwrap();
            backend.state_mut().selected = sibling_index;
            backend.render();
            key(&mut backend, KeyCode::Right);
            assert!(
                lines(&backend)
                    .iter()
                    .any(|row| row.contains("sibling-child"))
            );
            for mode in [
                TreeExpansionMode::AlwaysOpen,
                TreeExpansionMode::Auto,
                TreeExpansionMode::AlwaysOpen,
                TreeExpansionMode::Auto,
            ] {
                backend.state_mut().mode = mode;
                backend.render();
                let rows = lines(&backend);
                assert!(
                    rows.iter().any(|row| row.contains("sibling-child")),
                    "composed={composed} initial={initial_mode:?} mode={mode:?}: {rows:?}"
                );
                assert_eq!(
                    rows.iter().any(|row| row.trim_end().ends_with(" leaf")),
                    mode == TreeExpansionMode::AlwaysOpen,
                    "mode change must still update the affected group"
                );
            }
        }
    }
}
