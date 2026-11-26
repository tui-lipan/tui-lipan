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

#[derive(Clone)]
struct DelayedSubmenu {
    delay: std::time::Duration,
}

impl Component for DelayedSubmenu {
    type Properties = ();
    type State = std::time::Duration;
    type Message = std::time::Duration;

    fn create_state(&self, _: &()) -> Self::State {
        self.delay
    }

    fn update(&mut self, delay: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = delay;
        Update::render()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        ContextMenu::new(Text::new("Host"))
            .open(true)
            .anchor(Some((2, 2)))
            .submenu_delay(ctx.state)
            .entries([
                ContextMenuEntry::action("pin", "Pin thread"),
                ContextMenuEntry::submenu(
                    "copy",
                    "Copy",
                    [ContextMenuEntry::action("copy-id", "Thread ID")],
                ),
                ContextMenuEntry::action("delete", "Delete"),
            ])
            .into()
    }
}

#[test]
fn submenu_delay_waits_for_hover_cancels_on_sweep_and_retimes() {
    use std::time::Duration;

    let mut backend = TestBackend::new(DelayedSubmenu {
        delay: Duration::from_millis(200),
    });
    backend.render();

    let text = backend.capture_frame().plain_text();
    let (copy_x, copy_y) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Copy").map(|x| (x as u16 + 1, y as u16)))
        .expect("Copy row should be visible");
    let (pin_x, pin_y) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Pin thread").map(|x| (x as u16 + 1, y as u16)))
        .expect("Pin thread row should be visible");

    // Hover Copy, advance less than delay -> submenu must stay closed.
    backend
        .send_mouse(MouseEvent {
            x: copy_x,
            y: copy_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    backend.advance(Duration::from_millis(100));
    assert!(
        !backend.capture_frame().plain_text().contains("Thread ID"),
        "submenu should stay closed before submenu_delay elapses"
    );

    // Sweep back to Pin thread before delay completes -> cancels pending open.
    backend
        .send_mouse(MouseEvent {
            x: pin_x,
            y: pin_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    backend.advance(Duration::from_millis(200));
    assert!(
        !backend.capture_frame().plain_text().contains("Thread ID"),
        "sweeping off submenu row before delay must cancel opening"
    );

    // Hover Copy again and wait the full delay -> submenu opens.
    backend
        .send_mouse(MouseEvent {
            x: copy_x,
            y: copy_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    backend.advance(Duration::from_millis(120));
    assert!(!backend.capture_frame().plain_text().contains("Thread ID"));

    // Shortening submenu_delay mid-hover retimes from the original hover entry.
    backend.dispatch(Duration::from_millis(100)).unwrap();
    backend.render();
    assert!(
        backend.capture_frame().plain_text().contains("Thread ID"),
        "shortening submenu_delay below elapsed hover time should open immediately"
    );
}

#[test]
fn escape_closes_hover_opened_submenu_before_root_menu() {
    let mut backend = TestBackend::new(CascadingMenu::default());
    backend.render();
    assert!(*backend.state());

    let text = backend.capture_frame().plain_text();
    let (copy_x, copy_y) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Copy").map(|x| (x as u16 + 1, y as u16)))
        .expect("Copy row should be visible");

    // Hover Copy to open the submenu without transferring keyboard focus.
    backend
        .send_mouse(MouseEvent {
            x: copy_x,
            y: copy_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        backend.capture_frame().plain_text().contains("Thread ID"),
        "hovering Copy should open submenu"
    );

    // First Esc closes only the hover-opened submenu, keeping the root menu open.
    key(&mut backend, KeyCode::Esc);
    assert!(
        *backend.state(),
        "root menu must stay open after Esc closes a hover-opened submenu"
    );
    assert!(
        !backend.capture_frame().plain_text().contains("Thread ID"),
        "submenu must close on first Esc"
    );

    // Second Esc closes the root menu.
    key(&mut backend, KeyCode::Esc);
    assert!(!*backend.state(), "second Esc must close root menu");
}

struct SizedMenu {
    width: Option<Length>,
    height: Length,
    entries: Vec<ContextMenuEntry>,
}

impl Component for SizedMenu {
    type Properties = ();
    type State = ();
    type Message = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        let mut menu = ContextMenu::new(Text::new("Host"))
            .open(true)
            .anchor(Some((2, 2)))
            .height(self.height)
            .entries(self.entries.clone());
        if let Some(width) = self.width {
            menu = menu.width(width);
        }
        menu.into()
    }
}

#[test]
fn submenu_anchors_against_explicit_and_constrained_parent_width() {
    let entries = vec![
        ContextMenuEntry::action("a", "Alpha"),
        ContextMenuEntry::submenu(
            "sub",
            "More",
            [ContextMenuEntry::action("child", "SubItem")],
        ),
    ];

    // 1. Explicit parent width (40 cols): submenu must start at x = 2 + 40 = 42, not overlap!
    let mut backend = TestBackend::new(SizedMenu {
        width: Some(Length::Px(40)),
        height: Length::Auto,
        entries: entries.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();
    backend
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_key(KeyEvent {
            code: KeyCode::Right,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    let frame = backend.capture_frame();
    let lines = frame.to_fixed_grid_lines();
    let sub_col = lines
        .iter()
        .find_map(|line| line.find("SubItem"))
        .expect("SubItem should be rendered");
    assert!(
        sub_col >= 42,
        "submenu should open at or right of explicit 40-col parent (x=2..42), got col {sub_col}:\n{}",
        frame.plain_text()
    );

    // 2. Narrow terminal constraining a 40-col menu: submenu flips/clamps cleanly without panic.
    let mut narrow = TestBackend::new(SizedMenu {
        width: Some(Length::Px(40)),
        height: Length::Auto,
        entries,
    });
    narrow.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 12,
    });
    narrow.render();
    narrow
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    narrow
        .send_key(KeyEvent {
            code: KeyCode::Right,
            mods: KeyMods::NONE,
        })
        .unwrap();
    narrow.render();
    assert!(
        narrow.capture_frame().plain_text().contains("SubItem"),
        "submenu should remain visible in a terminal-constrained layout"
    );
}

#[test]
fn submenu_tracks_scrolled_parent_row_and_closes_when_scrolled_out_of_view() {
    let entries = vec![
        ContextMenuEntry::action("0", "Row 0"),
        ContextMenuEntry::action("1", "Row 1"),
        ContextMenuEntry::action("2", "Row 2"),
        ContextMenuEntry::action("3", "Row 3"),
        ContextMenuEntry::action("4", "Row 4"),
        ContextMenuEntry::submenu(
            "sub",
            "Row 5 Sub",
            [ContextMenuEntry::action("child", "ScrolledChild")],
        ),
        ContextMenuEntry::action("6", "Row 6"),
        ContextMenuEntry::action("7", "Row 7"),
        ContextMenuEntry::action("8", "Row 8"),
    ];

    // Menu height = 5 (2 border rows + 3 visible item rows).
    let mut backend = TestBackend::new(SizedMenu {
        width: None,
        height: Length::Px(5),
        entries,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 24,
    });
    backend.render();

    // Navigate down 5 times so "Row 5 Sub" is scrolled into the 3-row viewport, then open submenu.
    for _ in 0..5 {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Down,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    backend
        .send_key(KeyEvent {
            code: KeyCode::Right,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    let open_frame = backend.capture_frame().plain_text();
    let trigger_y = open_frame
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.contains("Row 5 Sub").then_some(y))
        .expect("Row 5 Sub should be visible");
    let child_y = open_frame
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.contains("ScrolledChild").then_some(y))
        .expect("ScrolledChild should be visible");
    assert_eq!(
        child_y, trigger_y,
        "submenu's first item should align vertically with scrolled trigger row:\n{open_frame}"
    );

    // Wheel-scroll the parent menu back up until "Row 5 Sub" scrolls out of view.
    for _ in 0..5 {
        backend
            .send_mouse(MouseEvent {
                x: 6,
                y: 4,
                kind: MouseKind::ScrollUp,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    let scrolled_out = backend.capture_frame().plain_text();
    assert!(
        !scrolled_out.contains("Row 5 Sub") && !scrolled_out.contains("ScrolledChild"),
        "submenu must close once its trigger row scrolls out of view:\n{scrolled_out}"
    );
}

struct FlatGutterMenu;

impl Component for FlatGutterMenu {
    type Properties = ();
    type State = ();
    type Message = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        ContextMenu::new(Text::new("Host"))
            .open(true)
            .anchor(Some((1, 1)))
            .items([ListItem::new("Open")
                .gutter(tui_lipan::ListItemGutter::from_spans([Span::new("*")]))])
            .into()
    }
}

#[test]
fn flat_context_menu_preserves_zero_default_gutter_gap() {
    let mut backend = TestBackend::new(FlatGutterMenu);
    backend.render();
    let frame = backend.capture_frame().plain_text();
    assert!(
        frame.contains("*Open"),
        "flat context menu should default gutter_gap to 0 (matching List):\n{frame}"
    );
}
