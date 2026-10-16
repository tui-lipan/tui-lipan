use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct ExpansionTree {
    composed: bool,
    controlled: bool,
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
                } else {
                    ctx.state.group_open = event.expanded;
                }
            }
            Msg::Activate => ctx.state.activations += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut group = TreeNode::new("group")
            .expansion_mode(ctx.state.mode)
            .expanded(ctx.state.group_open);
        if self.composed {
            group = group.content(Text::new("group"));
        }
        if ctx.state.loaded {
            group = group.child(TreeNode::new("leaf"));
        }
        Tree::new(
            TreeNode::new("root")
                .expanded(ctx.state.root_open)
                .children([group, TreeNode::new("sibling")]),
        )
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
            assert!(rows[1].starts_with("├─ group"), "{rows:?}");
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
            assert!(lines(&backend)[1].starts_with("├─ group"));
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
            assert!(lines(&backend)[1].starts_with("├─ group"));
        }
    }
}
