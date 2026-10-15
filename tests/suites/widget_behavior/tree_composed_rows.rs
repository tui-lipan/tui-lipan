use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct ComposedTree;
#[derive(Default)]
struct State {
    expanded: bool,
    selected: Option<usize>,
    activated: Vec<usize>,
    toggled: Vec<bool>,
    button: usize,
    reverse: bool,
    header: bool,
    child_header: bool,
    header_only: bool,
    reject_toggle: bool,
    lazy: bool,
    loaded: bool,
    hovered: Vec<(usize, bool)>,
}
#[derive(Clone)]
enum Msg {
    Select(usize),
    Activate(usize),
    Toggle(bool),
    Button,
    Reverse,
    Load,
    Hover(usize, bool),
}
impl Component for ComposedTree {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(index) => ctx.state.selected = Some(index),
            Msg::Activate(index) => ctx.state.activated.push(index),
            Msg::Toggle(expanded) => {
                if !ctx.state.reject_toggle {
                    ctx.state.expanded = expanded;
                }
                ctx.state.toggled.push(expanded);
            }
            Msg::Button => ctx.state.button += 1,
            Msg::Reverse => ctx.state.reverse = !ctx.state.reverse,
            Msg::Load => ctx.state.loaded = true,
            Msg::Hover(index, hovered) => ctx.state.hovered.push((index, hovered)),
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let content = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(Text::new("project"))
            .child(
                Button::new("×")
                    .tab_stop(false)
                    .on_click(ctx.link().callback(|_| Msg::Button))
                    .key("close"),
            );
        let mut children = if !ctx.state.lazy || ctx.state.loaded {
            vec![
                TreeNode::new("child")
                    .content(Text::new("child"))
                    .key("child"),
            ]
        } else {
            Vec::new()
        };
        if ctx.state.header_only {
            children.clear();
        }
        if ctx.state.child_header {
            children.insert(0, TreeNode::new(ListItem::header("Files")));
        }
        let project = TreeNode::new("project")
            .content(content)
            .key("project")
            .expanded(ctx.state.expanded)
            .indent(2)
            .expandable(true)
            .children(children);
        let second = TreeNode::new("second")
            .content(Text::new("second"))
            .key("second");
        let mut roots = if ctx.state.reverse {
            vec![second, project]
        } else {
            vec![project, second]
        };
        if ctx.state.header {
            roots.insert(
                0,
                TreeNode::new(ListItem::header("section"))
                    .content(Text::new("section"))
                    .show_disclosure(false),
            );
        }
        let mut tree = Tree::forest(roots)
            .controlled_expansion(true)
            .toggle_on_activate(false)
            .indent_style(IndentStyle::Branch)
            .content_gap(1)
            .focus_key("tree")
            .scrollbar(true)
            .scroll_keys(ScrollKeymap::DEFAULT | ScrollKeymap::VIM_JUMP)
            .on_hover(
                ctx.link()
                    .callback(|event: TreeHoverEvent| Msg::Hover(event.index, event.hovered)),
            )
            .on_select(ctx.link().callback(|e: TreeEvent| Msg::Select(e.index)))
            .on_activate(ctx.link().callback(|e: TreeEvent| Msg::Activate(e.index)))
            .on_toggle(
                ctx.link()
                    .callback(|e: TreeToggleEvent| Msg::Toggle(e.expanded)),
            );
        if let Some(index) = ctx.state.selected {
            tree = tree.selected(index);
        }
        tree.into()
    }
}
fn backend() -> TestBackend<ComposedTree> {
    let mut backend = TestBackend::new(ComposedTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 8,
    });
    backend.render();
    backend
}
fn click(backend: &mut TestBackend<ComposedTree>, x: u16, y: u16) {
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
fn key(backend: &mut TestBackend<ComposedTree>, code: KeyCode) {
    backend
        .send_key(KeyEvent {
            code,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
}
#[test]
fn forest_has_no_artificial_root_and_label_activation_does_not_toggle() {
    let mut backend = backend();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    assert!(rows[0].contains("▶ project"), "{rows:?}");
    assert!(rows[1].contains("second"), "{rows:?}");
    click(&mut backend, 4, 0);
    assert_eq!(backend.state().activated, [0]);
    assert!(backend.state().toggled.is_empty());
    click(&mut backend, 0, 0);
    assert_eq!(backend.state().toggled, [true]);
    assert_eq!(
        backend.state().activated,
        [0],
        "disclosure must not activate the row"
    );
    assert!(backend.capture_frame().to_fixed_grid_lines()[1].contains("child"));
}
#[test]
fn embedded_button_consumes_click_without_activating_or_toggling_row() {
    let mut backend = backend();
    let rect = backend.rect_of_key(&"close".into()).unwrap();
    click(&mut backend, rect.x as u16, rect.y as u16);
    assert_eq!(backend.state().button, 1);
    assert!(backend.state().activated.is_empty());
    assert!(backend.state().toggled.is_empty());
}
#[test]
fn navigation_reports_selection_when_entering_child_and_returning_to_parent() {
    let mut backend = backend();
    backend.focus_key(&"tree".into());
    key(&mut backend, KeyCode::Right);
    assert!(backend.state().expanded);
    key(&mut backend, KeyCode::Right);
    assert_eq!(backend.state().selected, Some(1));
    key(&mut backend, KeyCode::Left);
    assert_eq!(backend.state().selected, Some(0));
    assert_eq!(
        backend.state().toggled,
        [true],
        "moving to a parent is selection, not a toggle"
    );
    key(&mut backend, KeyCode::End);
    assert_eq!(backend.state().selected, Some(2));
    key(&mut backend, KeyCode::Home);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(backend.state().activated, [0]);
}
#[test]
fn controlled_expansion_follows_identity_when_roots_are_reordered() {
    let mut backend = backend();
    click(&mut backend, 0, 0);
    backend.dispatch(Msg::Reverse).unwrap();
    backend.render();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    assert!(rows[0].contains("second"), "{rows:?}");
    assert!(rows[1].contains("▼ project"), "{rows:?}");
    assert!(rows[2].contains("child"), "{rows:?}");
    click(&mut backend, 0, 1);
    assert_eq!(backend.state().toggled, [true, false]);
    assert!(
        !backend
            .capture_frame()
            .to_fixed_grid_lines()
            .join("\n")
            .contains("child")
    );
}

#[test]
fn composed_tree_skips_headers_for_initial_activation_and_navigation() {
    let mut backend = backend();
    backend.state_mut().header = true;
    backend.render();
    backend.focus_key(&"tree".into());
    key(&mut backend, KeyCode::Enter);
    assert_eq!(backend.state().activated, [1]);
    key(&mut backend, KeyCode::Down);
    assert_eq!(backend.state().selected, Some(2));
    key(&mut backend, KeyCode::Home);
    assert_eq!(backend.state().selected, Some(1));
    assert!(backend.capture_frame().to_fixed_grid_lines()[0].starts_with("section"));
}

#[test]
fn entering_an_expanded_subtree_skips_structural_headers() {
    for code in [KeyCode::Right, KeyCode::Char('l')] {
        let mut backend = backend();
        backend.state_mut().expanded = true;
        backend.state_mut().child_header = true;
        backend.render();
        backend.focus_key(&"tree".into());
        key(&mut backend, code);
        assert_eq!(backend.state().selected, Some(2));
        assert!(backend.state().toggled.is_empty());
    }
}

#[test]
fn entering_a_subtree_with_only_headers_does_not_select_the_next_root() {
    let mut backend = backend();
    backend.state_mut().expanded = true;
    backend.state_mut().child_header = true;
    backend.state_mut().header_only = true;
    backend.render();
    backend.focus_key(&"tree".into());
    key(&mut backend, KeyCode::Right);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(backend.state().activated, [0]);
    assert!(backend.state().selected.is_none());
    assert!(backend.state().toggled.is_empty());
}

struct LeadingOnlyTree;

impl Component for LeadingOnlyTree {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        Tree::new(
            TreeNode::new("project")
                .expanded(true)
                .child(TreeNode::new("child").leading(Text::new("●"))),
        )
        .into()
    }
}

#[test]
fn leading_content_renders_without_custom_row_content_anywhere_in_the_tree() {
    let mut backend = TestBackend::new(LeadingOnlyTree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 32,
        h: 8,
    });
    backend.render();
    let rows = backend.capture_frame().to_fixed_grid_lines();
    let child = rows.iter().find(|row| row.contains("child")).unwrap();
    assert!(child.starts_with('●'), "{rows:?}");
}

#[test]
fn controlled_expansion_can_reject_requests_without_temporarily_opening_the_row() {
    let mut backend = backend();
    backend.state_mut().reject_toggle = true;
    backend.render();
    click(&mut backend, 0, 0);
    click(&mut backend, 0, 0);
    assert_eq!(backend.state().toggled, [true, true]);
    assert!(
        !backend
            .capture_frame()
            .to_fixed_grid_lines()
            .join("\n")
            .contains("child")
    );
}

#[test]
fn composed_tree_can_expand_a_lazy_node_and_receive_children_later() {
    let mut backend = backend();
    backend.state_mut().lazy = true;
    backend.render();
    click(&mut backend, 0, 0);
    assert!(backend.state().expanded);
    assert!(
        !backend
            .capture_frame()
            .to_fixed_grid_lines()
            .join("\n")
            .contains("child")
    );
    backend.dispatch(Msg::Load).unwrap();
    backend.render();
    assert!(backend.capture_frame().to_fixed_grid_lines()[1].contains("child"));
}

#[test]
fn hovering_an_embedded_action_keeps_its_parent_row_hovered() {
    let mut backend = backend();
    let rect = backend.rect_of_key(&"close".into()).unwrap();
    backend
        .send_mouse(MouseEvent {
            x: rect.x as u16,
            y: rect.y as u16,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(backend.state().hovered.contains(&(0, true)));
    backend
        .send_mouse(MouseEvent {
            x: 0,
            y: 6,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(backend.state().hovered.contains(&(0, false)));
}

struct ChromeTree {
    composed: bool,
    foreground: bool,
}
impl Component for ChromeTree {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let mut child = TreeNode::new("child").indent(2);
        if self.composed {
            child = child.content(Text::new("child"));
        }
        Tree::new(TreeNode::new("root").expanded(true).children([child]))
            .show_icons(false)
            .indent_style(IndentStyle::Branch)
            .indent_guide_style(Style::new().fg(Color::rgb(30, 30, 30)))
            .item_hover_style(if self.foreground {
                Style::new()
                    .fg(Color::Yellow)
                    .bg(Color::rgb(70, 70, 70))
                    .contrast_policy(tui_lipan::ContrastPolicy::Wcag)
            } else {
                Style::new()
                    .bg(Color::rgb(70, 70, 70))
                    .contrast_policy(tui_lipan::ContrastPolicy::Wcag)
            })
            .width(Length::Px(20))
            .height(Length::Px(5))
            .into()
    }
}

#[test]
fn composed_guides_match_text_tree_hover_contrast_and_foreground() {
    for foreground in [false, true] {
        let mut results = Vec::new();
        for composed in [false, true] {
            let mut backend = TestBackend::new(ChromeTree {
                composed,
                foreground,
            });
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 5,
            });
            backend.render();
            let before = backend.capture_frame().cell(0, 1).clone();
            backend
                .send_mouse(MouseEvent {
                    x: 4,
                    y: 1,
                    kind: MouseKind::Moved,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
            let after = backend.capture_frame().cell(0, 1).clone();
            assert_ne!(
                after.fg,
                before.fg,
                "hover keeps guides readable, composed={composed}: {:?}",
                backend.capture_frame().to_fixed_grid_lines()
            );
            results.push((after.symbol.clone(), after.fg, after.bg));
            backend
                .send_mouse(MouseEvent {
                    x: 19,
                    y: 4,
                    kind: MouseKind::Moved,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
            assert_eq!(
                backend.capture_frame().cell(0, 1).fg,
                before.fg,
                "leaving restores guide color"
            );
        }
        assert_eq!(results[0], results[1], "both Tree render paths agree");
    }
}
