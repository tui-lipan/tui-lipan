use std::{cell::RefCell, rc::Rc};
use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::{TestBackend, prelude::*};

#[derive(Clone, Default)]
struct Menus(Rc<RefCell<Vec<String>>>);
impl Component for Menus {
    type Properties = ();
    type State = bool;
    type Message = bool;
    fn create_state(&self, _: &()) -> bool {
        false
    }
    fn update(&mut self, open: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state = open;
        Update::render()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let selected = self.0.clone();
        let activated = self.0.clone();
        let region = self.0.clone();
        let link = ctx.link().clone();
        VStack::new()
            .gap(0)
            .child(
                DraggableTabBar::new()
                    .tabs([DraggableTab::new("First"), DraggableTab::new("Second")])
                    .selected(0)
                    .focusable(false)
                    .on_select(Callback::new(move |event: TabsEvent| {
                        selected
                            .borrow_mut()
                            .push(format!("selected {}", event.index))
                    }))
                    .on_context_menu(Callback::new(
                        move |(index, _): (Option<usize>, MouseEvent)| {
                            region.borrow_mut().push(format!("menu {index:?}"));
                            link.send(true);
                        },
                    ))
                    .key("tabs"),
            )
            .child(
                ContextMenu::new(
                    MouseRegion::new()
                        .on_context_menu(ctx.link().callback(|_| true))
                        .child(Text::new("Region")),
                )
                .open(ctx.state)
                .anchor(Some((0, 3)))
                .border(false)
                .items(["One", "Two"])
                .on_select(Callback::new(move |index| {
                    activated.borrow_mut().push(format!("activated {index}"))
                }))
                .on_close(ctx.link().callback(|_| false)),
            )
            .into()
    }
}

fn right(backend: &mut TestBackend<Menus>, x: u16, y: u16) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Up(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
}

#[test]
fn context_menu_does_not_activate_on_keyboard_navigation() {
    let menus = Menus::default();
    let events = menus.0.clone();
    let mut backend = TestBackend::new(menus);
    backend.render();
    right(&mut backend, 1, 1);
    assert!(*backend.state(), "MouseRegion opens the menu");
    backend
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        events.borrow().is_empty(),
        "moving the highlight must not run an action"
    );
    backend
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(events.borrow().is_empty());
    backend
        .send_key(KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(&*events.borrow(), &["activated 1"]);
}

#[test]
fn right_clicking_tab_reports_hit_without_switching_tabs() {
    let menus = Menus::default();
    let events = menus.0.clone();
    let mut backend = TestBackend::new(menus);
    backend.render();
    let frame = backend.capture_frame();
    let text: String = (0..80).map(|x| frame.cell(x, 0).symbol.clone()).collect();
    let x = text.find("Second").unwrap() as u16;
    right(&mut backend, x, 0);
    assert_eq!(&*events.borrow(), &["menu Some(1)"]);
}

#[derive(Clone, Default)]
struct CascadingMenu(Rc<RefCell<Vec<String>>>);

impl Component for CascadingMenu {
    type Properties = ();
    type State = bool;
    type Message = bool;

    fn create_state(&self, _: &()) -> bool {
        true
    }

    fn update(&mut self, open: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state = open;
        Update::render()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let log = self.0.clone();
        ContextMenu::new(Text::new("Host"))
            .open(ctx.state)
            .anchor(Some((2, 2)))
            .entries([
                ContextMenuEntry::action("pin", "Pin thread"),
                ContextMenuEntry::action("disabled", "Busy action").disabled(true),
                ContextMenuEntry::separator(),
                ContextMenuEntry::submenu(
                    "copy",
                    "Copy",
                    [
                        ContextMenuEntry::action("copy-path", "Path").detail("/repo"),
                        ContextMenuEntry::action("copy-id", "Thread ID"),
                    ],
                ),
                ContextMenuEntry::action("delete", "Delete").destructive(true),
            ])
            .on_action(Callback::new(move |id: std::sync::Arc<str>| {
                log.borrow_mut().push(id.to_string());
            }))
            .on_close(ctx.link().callback(|_| false))
            .into()
    }
}

fn key(backend: &mut TestBackend<CascadingMenu>, code: KeyCode) {
    backend
        .send_key(KeyEvent {
            code,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
}

#[test]
fn cascading_context_menu_navigates_submenus_skips_disabled_and_closes_in_layers() {
    let app = CascadingMenu::default();
    let log = app.0.clone();
    let mut backend = TestBackend::new(app);
    backend.render();

    // Down from "Pin thread" skips disabled "Busy action" and separator to land on "Copy".
    key(&mut backend, KeyCode::Down);
    key(&mut backend, KeyCode::Right);
    let open_text = backend.capture_frame().plain_text();
    assert!(
        open_text.contains("Path") && open_text.contains("Thread ID"),
        "submenu should open on Right:\n{open_text}"
    );

    // Esc inside a submenu closes only the submenu, keeping the root menu open.
    key(&mut backend, KeyCode::Esc);
    assert!(*backend.state(), "root menu stays open when submenu closes");
    let closed_sub_text = backend.capture_frame().plain_text();
    assert!(
        !closed_sub_text.contains("Thread ID"),
        "submenu should close on Esc:\n{closed_sub_text}"
    );

    // Reopen submenu with Enter, move to second item ("Thread ID"), and activate it.
    key(&mut backend, KeyCode::Enter);
    key(&mut backend, KeyCode::Down);
    key(&mut backend, KeyCode::Enter);
    assert_eq!(&*log.borrow(), &["copy-id"]);
    assert!(!*backend.state(), "activating leaf action closes menu");
}

#[test]
fn hovering_submenu_opens_and_clicking_outside_dismisses_entire_stack() {
    let app = CascadingMenu::default();
    let mut backend = TestBackend::new(app);
    backend.render();

    let text = backend.capture_frame().plain_text();
    let (copy_x, copy_y) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Copy").map(|x| (x as u16 + 1, y as u16)))
        .expect("Copy row should be visible");

    backend
        .send_mouse(MouseEvent {
            x: copy_x,
            y: copy_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    let after_hover = backend.capture_frame().plain_text();
    assert!(
        after_hover.contains("Thread ID"),
        "hovering submenu row should open submenu:\n{after_hover}"
    );

    // Clicking outside both menus closes the entire menu stack in a single click.
    backend
        .send_mouse(MouseEvent {
            x: 75,
            y: 20,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(!*backend.state(), "outside click should close menu stack");
}

#[test]
fn right_click_outside_closes_and_inside_activates_item() {
    let mut backend = TestBackend::new(CascadingMenu::default());
    backend.render();
    assert!(*backend.state());

    // Right-clicking outside the menu closes it without leaking.
    backend
        .send_mouse(MouseEvent {
            x: 75,
            y: 20,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 75,
            y: 20,
            kind: MouseKind::Up(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        !*backend.state(),
        "right-clicking outside an open context menu should close it"
    );

    // Re-open and verify right-clicking an enabled action activates it and closes the menu.
    let app = CascadingMenu::default();
    let log = Rc::clone(&app.0);
    let mut backend = TestBackend::new(app);
    backend.render();
    assert!(*backend.state());

    let frame = backend.capture_frame().plain_text();
    let (item_x, item_y) = frame
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            line.find("Pin thread")
                .map(|col| (col as u16 + 1, row as u16))
        })
        .expect("Pin thread should be rendered inside the menu");

    backend
        .send_mouse(MouseEvent {
            x: item_x,
            y: item_y,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: item_x,
            y: item_y,
            kind: MouseKind::Up(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert_eq!(&*log.borrow(), &["pin"]);
    assert!(
        !*backend.state(),
        "right-clicking an action inside the context menu should activate and close it"
    );

    // Re-open and verify right-clicking on the anchor border corner closes the menu cleanly.
    let mut backend = TestBackend::new(CascadingMenu::default());
    backend.render();
    assert!(*backend.state());
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 2,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 2,
            kind: MouseKind::Up(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        !*backend.state(),
        "right-clicking the menu's border corner (same spot as anchor) should close it"
    );
}
