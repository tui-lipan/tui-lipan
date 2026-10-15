use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

#[derive(Clone, Copy)]
enum Kind {
    List,
    Tree,
    ComposedTree,
    FileTree,
}

struct Entries {
    kind: Kind,
    ctrl: bool,
    activate_on_click: bool,
}

#[derive(Default)]
struct State {
    activated: Vec<String>,
    ctrl_activated: Vec<String>,
    selected: usize,
    toggled: usize,
}

enum Msg {
    Activate(String),
    CtrlActivate(String),
    Select,
    Toggle,
}

impl Component for Entries {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Activate(path) => ctx.state.activated.push(path),
            Msg::CtrlActivate(path) => ctx.state.ctrl_activated.push(path),
            Msg::Select => ctx.state.selected += 1,
            Msg::Toggle => ctx.state.toggled += 1,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        match self.kind {
            Kind::List => {
                let mut list = List::new()
                    .items([ListItem::new("directory"), ListItem::new("file.txt")])
                    .activate_on_click(self.activate_on_click)
                    .on_select(ctx.link().callback(|_| Msg::Select))
                    .on_activate(
                        ctx.link()
                            .callback(|event: ListEvent| Msg::Activate(event.index.to_string())),
                    );
                if self.ctrl {
                    list =
                        list.on_ctrl_activate(ctx.link().callback(|event: ListEvent| {
                            Msg::CtrlActivate(event.index.to_string())
                        }));
                }
                list.key("entries")
            }
            Kind::Tree | Kind::ComposedTree => {
                let node = |label: &'static str| {
                    let node = TreeNode::new(label);
                    if matches!(self.kind, Kind::ComposedTree) {
                        node.content(Text::new(label))
                    } else {
                        node
                    }
                };
                let mut children = vec![node("directory").child(node("nested")), node("file.txt")];
                if matches!(self.kind, Kind::ComposedTree) {
                    children.insert(
                        0,
                        TreeNode::new(ListItem::header("heading"))
                            .content(Text::new("heading"))
                            .child(node("header child")),
                    );
                }
                let root = node("root").expanded(true).children(children);
                let mut tree = Tree::new(root)
                    .focus_key("entries")
                    .activate_on_click(self.activate_on_click)
                    .on_select(ctx.link().callback(|_| Msg::Select))
                    .on_toggle(ctx.link().callback(|_| Msg::Toggle))
                    .on_activate(
                        ctx.link()
                            .callback(|event: TreeEvent| Msg::Activate(event.index.to_string())),
                    );
                if self.ctrl {
                    tree =
                        tree.on_ctrl_activate(ctx.link().callback(|event: TreeEvent| {
                            Msg::CtrlActivate(event.index.to_string())
                        }));
                }
                tree.into()
            }
            Kind::FileTree => {
                let mut tree = FileTree::new("/repo")
                    .entry_source(FileTreeEntrySource::provided([
                        FileTreeDirectoryListing::new(
                            ".",
                            [
                                FileTreeEntry::directory("directory"),
                                FileTreeEntry::file("file.txt"),
                            ],
                        ),
                        FileTreeDirectoryListing::new("directory", [FileTreeEntry::file("nested")]),
                    ]))
                    .change_source(FileTreeChangeSource::Provided(Vec::new()))
                    .tree_focus_key("entries")
                    .activate_on_click(self.activate_on_click)
                    .on_select(ctx.link().callback(|_| Msg::Select))
                    .on_toggle(ctx.link().callback(|_| Msg::Toggle))
                    .on_activate(
                        ctx.link()
                            .callback(|event: FileTreeEvent| Msg::Activate(event.path.to_string())),
                    );
                if self.ctrl {
                    tree = tree.on_ctrl_activate(ctx.link().callback(|event: FileTreeEvent| {
                        Msg::CtrlActivate(event.path.to_string())
                    }));
                }
                tree.into()
            }
        }
    }
}

fn backend(kind: Kind, ctrl: bool, activate_on_click: bool) -> TestBackend<Entries> {
    let mut backend = TestBackend::new(Entries {
        kind,
        ctrl,
        activate_on_click,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    backend
}

fn click<C: Component>(backend: &mut TestBackend<C>, label: &str, mods: KeyMods) {
    let lines = backend.capture_frame().to_fixed_grid_lines();
    let (y, row) = lines
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains(label))
        .unwrap();
    let x = row[..row.find(label).unwrap()].chars().count() as u16;
    for kind in [
        MouseKind::Moved,
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y: y as u16,
                kind,
                mods,
            })
            .unwrap();
        backend.render();
    }
}

#[test]
fn ctrl_click_and_ctrl_enter_are_distinct_and_do_not_toggle_directories() {
    for kind in [Kind::List, Kind::Tree, Kind::ComposedTree, Kind::FileTree] {
        let mut backend = backend(kind, true, true);
        click(&mut backend, "directory", KeyMods::CTRL);
        assert_eq!(backend.state().ctrl_activated.len(), 1);
        assert!(backend.state().selected > 0);
        assert!(backend.state().activated.is_empty());
        assert_eq!(backend.state().toggled, 0);
        assert!(!backend.capture_frame().plain_text().contains("nested"));
        assert!(backend.focus_key(&"entries".into()));
        backend
            .send_key(KeyEvent {
                code: KeyCode::Enter,
                mods: KeyMods::CTRL,
            })
            .unwrap();
        backend.render();
        assert_eq!(backend.state().ctrl_activated.len(), 2);
        assert_eq!(backend.state().toggled, 0);
        click(&mut backend, "directory", KeyMods::NONE);
        assert_eq!(backend.state().activated.len(), 1);
        if !matches!(kind, Kind::List) {
            assert_eq!(backend.state().toggled, 1);
            assert!(backend.capture_frame().plain_text().contains("nested"));
            click(&mut backend, "directory", KeyMods::CTRL);
            assert_eq!(
                backend.state().toggled,
                1,
                "Ctrl also leaves an expanded directory open"
            );
        }
        click(&mut backend, "file.txt", KeyMods::CTRL);
        assert_eq!(
            backend.state().activated.len(),
            1,
            "Ctrl never also invokes normal activation"
        );
        if matches!(kind, Kind::FileTree) {
            assert_eq!(
                backend.state().ctrl_activated.first().unwrap(),
                "/repo/directory"
            );
            assert_eq!(
                backend.state().ctrl_activated.last().unwrap(),
                "/repo/file.txt"
            );
        }
    }
}

#[test]
fn ctrl_click_works_when_normal_click_activation_is_disabled() {
    for kind in [Kind::List, Kind::Tree, Kind::ComposedTree, Kind::FileTree] {
        let mut backend = backend(kind, true, false);
        click(&mut backend, "directory", KeyMods::CTRL);
        assert_eq!(backend.state().ctrl_activated.len(), 1);
        assert!(backend.state().activated.is_empty());
        assert_eq!(backend.state().toggled, 0);
    }
}

#[test]
fn ctrl_click_without_a_callback_keeps_existing_activation_behavior() {
    for kind in [Kind::List, Kind::Tree, Kind::ComposedTree, Kind::FileTree] {
        let mut backend = backend(kind, false, true);
        click(&mut backend, "directory", KeyMods::CTRL);
        assert_eq!(backend.state().activated.len(), 1);
        assert!(backend.state().ctrl_activated.is_empty());
    }
}

struct CtrlOnlyList(bool);

impl Component for CtrlOnlyList {
    type Message = ();
    type Properties = ();
    type State = usize;

    fn create_state(&self, _: &()) -> usize {
        0
    }
    fn update(&mut self, _: (), ctx: &mut Context<Self>) -> Update {
        ctx.state += 1;
        Update::none()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        List::new()
            .items([ListItem::header("heading"), ListItem::new("entry")])
            .selected(Some(1))
            .disabled(self.0)
            .scroll_wheel(false)
            .scrollbar(false)
            .show_scroll_indicators(false)
            .on_ctrl_activate(ctx.link().callback(|_| ()))
            .key("entries")
    }
}

#[test]
fn ctrl_only_lists_respect_disabled_rows_and_structural_headers() {
    for disabled in [false, true] {
        let mut backend = TestBackend::new(CtrlOnlyList(disabled));
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        });
        backend.render();
        click(&mut backend, "heading", KeyMods::CTRL);
        assert_eq!(*backend.state(), 0);
        click(&mut backend, "entry", KeyMods::CTRL);
        if disabled {
            assert_eq!(*backend.state(), 0);
            assert!(!backend.focus_key(&"entries".into()));
        } else {
            assert_eq!(*backend.state(), 1);
            assert!(backend.focus_key(&"entries".into()));
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Enter,
                    mods: KeyMods::CTRL,
                })
                .unwrap();
            assert_eq!(*backend.state(), 2);
        }
    }
}

#[test]
fn ctrl_click_on_a_structural_headers_arrow_does_not_select_or_activate_it() {
    let mut backend = backend(Kind::ComposedTree, true, true);
    let lines = backend.capture_frame().to_fixed_grid_lines();
    let (y, row) = lines
        .iter()
        .enumerate()
        .find(|(_, row)| row.contains("heading"))
        .unwrap();
    let x = row.chars().position(|ch| ch == '▶').unwrap() as u16;
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y: y as u16,
                kind,
                mods: KeyMods::CTRL,
            })
            .unwrap();
        backend.render();
    }
    assert_eq!(backend.state().selected, 0);
    assert!(backend.state().ctrl_activated.is_empty());
    assert!(backend.state().activated.is_empty());
    assert_eq!(backend.state().toggled, 0);
}
