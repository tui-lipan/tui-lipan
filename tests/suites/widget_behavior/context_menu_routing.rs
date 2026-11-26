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

#[test]
fn left_closes_hover_opened_submenu_without_closing_root_menu() {
    let mut backend = TestBackend::new(CascadingMenu::default());
    backend.render();
    assert!(*backend.state());

    let text = backend.capture_frame().plain_text();
    let (copy_x, copy_y) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Copy").map(|x| (x as u16 + 1, y as u16)))
        .expect("Copy row should be visible");

    // Hover Copy to open the submenu while root keeps keyboard focus.
    backend
        .send_mouse(MouseEvent {
            x: copy_x,
            y: copy_y,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(backend.capture_frame().plain_text().contains("Thread ID"));

    // First Left closes only the hover-opened submenu.
    key(&mut backend, KeyCode::Left);
    assert!(*backend.state(), "root menu must stay open on Left");
    assert!(
        !backend.capture_frame().plain_text().contains("Thread ID"),
        "Left must close hover-opened submenu even when root holds focus"
    );

    // Second Left on the root menu with no open submenu must keep the root menu open.
    key(&mut backend, KeyCode::Left);
    assert!(
        *backend.state(),
        "Left must never dismiss the root context menu"
    );
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

fn press<C: Component>(
    backend: &mut TestBackend<C>,
    kind: MouseKind,
    x: u16,
    y: u16,
    mods: KeyMods,
) {
    backend.send_mouse(MouseEvent { kind, x, y, mods }).unwrap();
}

#[test]
fn search_palette_context_menu_maps_filtered_rows_without_selecting() {
    type PickerHit = (SearchEvent<u8>, MouseEvent);
    struct Picker(Rc<RefCell<Vec<PickerHit>>>);

    impl Component for Picker {
        type Properties = ();
        type State = ();
        type Message = ();

        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, _: &Context<Self>) -> Element {
            let hits = self.0.clone();
            SearchPalette::new()
                .entries([
                    tui_lipan::SearchEntry::header("Group"),
                    tui_lipan::SearchEntry::item("Skip", 1),
                    tui_lipan::SearchEntry::item("Match first", 2),
                    tui_lipan::SearchEntry::item("Match target", 3),
                    tui_lipan::SearchEntry::item("Match disabled", 4).disabled(true),
                ])
                .initial_query("Match")
                .preserve_groups(true)
                .preserve_item_order(true)
                .initial_selected_item_index(Some(1))
                .on_context_menu(Callback::new(move |hit| hits.borrow_mut().push(hit)))
                .into()
        }
    }

    let hits = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Picker(hits.clone()));
    backend.render();
    let before = backend.capture_frame().plain_text();
    let find = |label| {
        before
            .lines()
            .enumerate()
            .find_map(|(y, line)| line.find(label).map(|x| (x as u16, y as u16)))
            .unwrap_or_else(|| panic!("{label} missing from palette:\n{before}"))
    };
    for label in ["Group", "Match disabled", "Match target"] {
        let (x, y) = find(label);
        press(
            &mut backend,
            MouseKind::Down(MouseButton::Right),
            x,
            y,
            KeyMods::NONE,
        );
        press(
            &mut backend,
            MouseKind::Up(MouseButton::Right),
            x,
            y,
            KeyMods::NONE,
        );
    }
    backend.render();
    assert_eq!(backend.capture_frame().plain_text(), before);
    let hits = hits.borrow();
    assert_eq!(hits.len(), 1);
    let (event, mouse) = &hits[0];
    assert_eq!(
        (event.match_index, event.item_index, event.item.value),
        (1, 2, 3)
    );
    assert_eq!((mouse.x, mouse.y), find("Match target"));
}

#[cfg(feature = "terminal")]
mod terminal_routing {
    use super::*;

    use tui_lipan::ForwardedMouseEvent;

    const RIGHT_DOWN: MouseKind = MouseKind::Down(MouseButton::Right);
    const RIGHT_DRAG: MouseKind = MouseKind::Drag(MouseButton::Right);
    const RIGHT_UP: MouseKind = MouseKind::Up(MouseButton::Right);

    #[derive(Default)]
    struct Events {
        menus: Vec<&'static str>,
        reports: Vec<(&'static str, ForwardedMouseEvent)>,
    }

    struct Panes {
        first: Rc<RefCell<TerminalScreen>>,
        second: Rc<RefCell<TerminalScreen>>,
        events: Rc<RefCell<Events>>,
        open: bool,
        open_when_untracked: bool,
    }

    impl Panes {
        fn new(open: bool, open_when_untracked: bool) -> Self {
            let second = Rc::new(RefCell::new(TerminalScreen::new(5, 10, 0)));
            second.borrow_mut().process_bytes(b"\x1b[?1002;1006h");
            Self {
                first: Rc::new(RefCell::new(TerminalScreen::new(5, 10, 0))),
                second,
                events: Rc::new(RefCell::new(Events::default())),
                open,
                open_when_untracked,
            }
        }

        fn terminal(&self, name: &'static str, screen: Rc<RefCell<TerminalScreen>>) -> Terminal {
            let events = self.events.clone();
            Terminal::new()
                .screen(screen)
                .width(Length::Px(10))
                .scrollbar(false)
                .on_mouse_forward_event(Callback::new(move |event| {
                    events.borrow_mut().reports.push((name, event));
                }))
        }
    }

    impl Component for Panes {
        type Properties = ();
        type State = bool;
        type Message = ();

        fn create_state(&self, _: &()) -> bool {
            self.open
        }

        fn update(&mut self, _: (), ctx: &mut Context<Self>) -> Update {
            ctx.state = false;
            Update::render()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let menus = self.events.clone();
            let ancestor = self.events.clone();
            ContextMenu::new(
                HStack::new()
                    .gap(0)
                    .child(
                        MouseRegion::new()
                            .on_context_menu(Callback::new(move |_| {
                                ancestor.borrow_mut().menus.push("ancestor");
                            }))
                            .child(
                                self.terminal("first", self.first.clone())
                                    .context_menu_policy(
                                        TerminalContextMenuPolicy::ApplicationFirst {
                                            override_mods: Some(KeyMods::CTRL),
                                            open_when_untracked: self.open_when_untracked,
                                        },
                                    )
                                    .on_context_menu(Callback::new(move |_| {
                                        menus.borrow_mut().menus.push("first");
                                    })),
                            ),
                    )
                    .child(self.terminal("second", self.second.clone())),
            )
            .open(ctx.state)
            .anchor(Some((15, 4)))
            .items(["Do something"])
            .on_close(ctx.link().callback(|_| ()))
            .into()
        }
    }

    #[test]
    fn terminal_policy_reads_live_modes_and_keeps_right_gesture_with_press_owner() {
        let panes = Panes::new(false, true);
        let first = panes.first.clone();
        let events = panes.events.clone();
        let mut backend = TestBackend::new(panes);
        backend.render();

        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::NONE);
        press(&mut backend, RIGHT_DRAG, 11, 1, KeyMods::NONE);
        press(&mut backend, RIGHT_UP, 11, 1, KeyMods::NONE);

        // Change the PTY mode through the live screen without rebuilding the component.
        first.borrow_mut().process_bytes(b"\x1b[?1002;1006h");
        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::NONE);
        press(&mut backend, RIGHT_DRAG, 11, 1, KeyMods::CTRL);
        press(&mut backend, RIGHT_UP, 11, 1, KeyMods::SHIFT);

        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::CTRL);
        press(&mut backend, RIGHT_UP, 11, 1, KeyMods::NONE);

        assert_eq!(events.borrow().menus, ["first", "first"]);
        let reports: Vec<_> = events
            .borrow()
            .reports
            .iter()
            .map(|(name, event)| (*name, event.kind, event.cell.0))
            .collect();
        assert_eq!(
            reports,
            [
                ("first", RIGHT_DOWN, 1),
                ("first", RIGHT_DRAG, 9),
                ("first", RIGHT_UP, 9)
            ]
        );
    }

    #[test]
    fn shift_keeps_right_press_local_unless_the_menu_override_matches() {
        let panes = Panes::new(false, true);
        panes.first.borrow_mut().process_bytes(b"\x1b[?1002;1006h");
        let events = panes.events.clone();
        let mut backend = TestBackend::new(panes);
        backend.render();

        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::SHIFT);
        press(&mut backend, RIGHT_UP, 1, 1, KeyMods::NONE);
        assert!(events.borrow().menus.is_empty());
        assert!(events.borrow().reports.is_empty());

        let ctrl_shift = KeyMods {
            shift: true,
            ..KeyMods::CTRL
        };
        press(&mut backend, RIGHT_DOWN, 1, 1, ctrl_shift);
        press(&mut backend, RIGHT_UP, 1, 1, KeyMods::NONE);
        assert_eq!(events.borrow().menus, ["first"]);
        assert!(events.borrow().reports.is_empty());
    }

    #[test]
    fn disabling_tracking_mid_gesture_and_x10_do_not_retarget_release() {
        let panes = Panes::new(false, false);
        let first = panes.first.clone();
        let events = panes.events.clone();
        let mut backend = TestBackend::new(panes);
        backend.render();

        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::NONE);
        first.borrow_mut().process_bytes(b"\x1b[?1000;1006h");
        press(&mut backend, RIGHT_UP, 1, 1, KeyMods::NONE);

        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::NONE);
        first.borrow_mut().process_bytes(b"\x1b[?1000l");
        press(&mut backend, RIGHT_UP, 11, 1, KeyMods::NONE);

        first.borrow_mut().process_bytes(b"\x1b[?9h");
        press(&mut backend, RIGHT_DOWN, 1, 1, KeyMods::NONE);
        first.borrow_mut().process_bytes(b"\x1b[?1003h");
        press(&mut backend, RIGHT_DRAG, 11, 1, KeyMods::NONE);
        press(&mut backend, RIGHT_UP, 11, 1, KeyMods::NONE);

        assert!(events.borrow().menus.is_empty());
        let reports: Vec<_> = events
            .borrow()
            .reports
            .iter()
            .map(|(name, event)| (*name, event.kind))
            .collect();
        assert_eq!(reports, [("first", RIGHT_DOWN), ("first", RIGHT_DOWN)]);
    }

    #[test]
    fn outside_dismissal_does_not_send_a_left_release_to_the_terminal() {
        let panes = Panes::new(true, true);
        let events = panes.events.clone();
        let mut backend = TestBackend::new(panes);
        backend.render();

        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Drag(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            press(&mut backend, kind, 11, 1, KeyMods::NONE);
        }

        assert!(!*backend.state());
        assert!(events.borrow().reports.is_empty());
    }
}

#[derive(Clone, Default)]
struct DetailMenu {
    extend: bool,
}

impl Component for DetailMenu {
    type Properties = ();
    type State = ();
    type Message = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        let menu = ContextMenu::new(Text::new("Host"))
            .open(true)
            .anchor(Some((0, 0)))
            .entries([
                ContextMenuEntry::action("one", "One").detail("K1"),
                ContextMenuEntry::action("two", "Two").detail("K2"),
                ContextMenuEntry::submenu(
                    "more",
                    "More",
                    [ContextMenuEntry::action("child", "Child")],
                ),
            ])
            .selection_style(Style::new().fg(Color::Black).bg(Color::Magenta))
            .detail_style(Style::new().fg(Color::Blue))
            .submenu_indicator_style(Style::new().fg(Color::Green));
        if self.extend {
            menu.extend_active_detail_style(Style::new().bold()).into()
        } else {
            menu.active_detail_style(Style::new().fg(Color::Red)).into()
        }
    }
}

#[test]
fn highlighted_entry_detail_uses_the_active_detail_style() {
    let mut backend = TestBackend::new(DetailMenu::default());
    backend.render();
    // The foreground of the first cell of `detail`, a two-letter label.
    let detail_fg = |backend: &TestBackend<DetailMenu>, detail: &str| {
        let frame = backend.capture_frame();
        let (first, second) = (&detail[..1], &detail[1..]);
        (0..frame.height)
            .flat_map(|y| (0..frame.width.saturating_sub(1)).map(move |x| (x, y)))
            .find(|&(x, y)| {
                frame.cell(x, y).symbol == first && frame.cell(x + 1, y).symbol == second
            })
            .map(|(x, y)| frame.cell(x, y).fg)
            .expect("detail on screen")
    };
    assert_eq!(detail_fg(&backend, "K1"), Color::Red);
    assert_eq!(detail_fg(&backend, "K2"), Color::Blue);
    backend
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert_eq!(detail_fg(&backend, "K1"), Color::Blue);
    assert_eq!(detail_fg(&backend, "K2"), Color::Red);
}

#[test]
fn hovering_submenu_applies_the_extended_active_detail_style_to_its_indicator() {
    let mut backend = TestBackend::new(DetailMenu { extend: true });
    backend.render();

    let frame = backend.capture_frame();
    let detail = frame.cells.iter().find(|cell| cell.symbol == "K").unwrap();
    assert_eq!(detail.fg, Color::Blue);
    assert!(detail.modifiers.bold);
    let indicator = frame
        .cells
        .iter()
        .position(|cell| cell.symbol == "›")
        .unwrap();
    assert_eq!(frame.cells[indicator].fg, Color::Green);
    assert!(!frame.cells[indicator].modifiers.bold);

    let width = usize::from(frame.width);
    backend
        .send_mouse(MouseEvent {
            x: (indicator % width) as u16,
            y: (indicator / width) as u16,
            kind: MouseKind::Moved,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    let frame = backend.capture_frame();
    let detail = frame.cells.iter().find(|cell| cell.symbol == "K").unwrap();
    assert_eq!(detail.fg, Color::Blue);
    assert!(!detail.modifiers.bold);
    let indicator = frame.cells.iter().find(|cell| cell.symbol == "›").unwrap();
    assert_eq!(indicator.fg, Color::Blue);
    assert!(indicator.modifiers.bold);
}

struct TextAreaContextMenuHarness;

struct TextAreaMenuState {
    editor: tui_lipan::TextEditor,
    anchor: Option<(u16, u16)>,
    copied_on_action: Option<String>,
}

#[derive(Clone, Debug)]
enum TextAreaMenuMsg {
    Edited(tui_lipan::TextAreaEvent),
    Open(MouseEvent),
    Action(std::sync::Arc<str>),
    Close,
}

impl Component for TextAreaContextMenuHarness {
    type Properties = ();
    type State = TextAreaMenuState;
    type Message = TextAreaMenuMsg;

    fn create_state(&self, _: &()) -> Self::State {
        let mut editor = tui_lipan::TextEditor::new("hello world");
        editor.set_cursor(11);
        editor.set_anchor(Some(6));
        TextAreaMenuState {
            editor,
            anchor: None,
            copied_on_action: None,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TextAreaMenuMsg::Edited(ev) => {
                ev.apply_to(&mut ctx.state.editor);
                Update::full()
            }
            TextAreaMenuMsg::Open(mouse) => {
                ctx.state.anchor = Some((mouse.x, mouse.y));
                Update::full()
            }
            TextAreaMenuMsg::Action(id) => {
                if id.as_ref() == "copy" {
                    ctx.state.copied_on_action =
                        ctx.state.editor.selected_text().map(ToOwned::to_owned);
                }
                ctx.state.anchor = None;
                Update::full()
            }
            TextAreaMenuMsg::Close => {
                ctx.state.anchor = None;
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let trigger = VStack::new()
            .child(Text::new("Header").height(Length::Px(1)))
            .child(
                TextArea::bound(&ctx.state.editor)
                    .height(Length::Px(4))
                    .on_change(ctx.link().callback(TextAreaMenuMsg::Edited))
                    .on_context_menu(ctx.link().callback(TextAreaMenuMsg::Open)),
            );
        ContextMenu::new(trigger)
            .open(ctx.state.anchor.is_some())
            .anchor(ctx.state.anchor)
            .entries([
                ContextMenuEntry::action("copy", "Copy"),
                ContextMenuEntry::action("paste", "Paste"),
            ])
            .on_action(ctx.link().callback(TextAreaMenuMsg::Action))
            .on_close(ctx.link().callback(|()| TextAreaMenuMsg::Close))
            .into()
    }
}

#[test]
fn clicking_context_menu_item_preserves_underlying_textarea_selection() {
    let mut backend = TestBackend::new(TextAreaContextMenuHarness);
    backend.render();

    // Right-click inside the TextArea (row 2) to open the context menu.
    backend
        .send_mouse(MouseEvent {
            x: 4,
            y: 2,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: 4,
            y: 2,
            kind: MouseKind::Up(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    let frame = backend.capture_frame().plain_text();
    let (copy_x, copy_y) = frame
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("Copy")
                .map(|x| (u16::try_from(x).unwrap(), u16::try_from(y).unwrap()))
        })
        .unwrap_or_else(|| panic!("expected Copy row in open context menu:\n{frame}"));

    // Left-click "Copy" inside the context menu: selection must remain intact when on_action runs.
    backend
        .send_mouse(MouseEvent {
            x: copy_x + 1,
            y: copy_y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: copy_x + 1,
            y: copy_y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();

    assert_eq!(
        backend.state().copied_on_action.as_deref(),
        Some("world"),
        "clicking a context menu item must not clear TextArea selection before on_action runs"
    );
    assert_eq!(
        backend.state().editor.selected_text(),
        Some("world"),
        "Copy should leave the TextArea selection intact"
    );

    // Clicking outside the TextArea (on the header at row 0) while the menu is closed still
    // clears the selection normally.
    backend
        .send_mouse(MouseEvent {
            x: 2,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        backend.state().editor.selected_text().is_none(),
        "clicking outside the TextArea when the context menu is closed should still clear selection"
    );
}

#[derive(Clone, Debug)]
enum UndoMenuMsg {
    Edited(tui_lipan::TextAreaEvent),
    Paste(&'static str),
    Reset(&'static str),
}

struct BoundUndoHarness;

impl Component for BoundUndoHarness {
    type Properties = ();
    type State = tui_lipan::TextEditor;
    type Message = UndoMenuMsg;

    fn create_state(&self, _: &()) -> Self::State {
        tui_lipan::TextEditor::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            UndoMenuMsg::Edited(ev) => {
                ev.apply_to(&mut ctx.state);
                Update::full()
            }
            UndoMenuMsg::Paste(text) => {
                ctx.state.insert_str(text);
                Update::full()
            }
            UndoMenuMsg::Reset(text) => {
                ctx.state = tui_lipan::TextEditor::new(text);
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::bound(&ctx.state)
            .height(Length::Px(4))
            .on_change(ctx.link().callback(UndoMenuMsg::Edited))
            .key("bound-editor")
    }
}

#[test]
fn programmatic_bound_editor_edits_preserve_and_extend_undo_history() {
    let mut backend = TestBackend::new(BoundUndoHarness);
    backend.render();

    // Focus the TextArea and type "hi " so undo history has a typed step.
    backend.focus_key(&tui_lipan::Key::from("bound-editor"));
    for ch in ['h', 'i', ' '] {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(ch),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    assert_eq!(backend.state().text(), "hi ");

    // Programmatically insert via TextEditor::insert_str (e.g. context menu Paste).
    backend.dispatch(UndoMenuMsg::Paste("there")).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi there");

    // Second multi-char programmatic edit without an intermediate keystroke gets its own undo step.
    backend.dispatch(UndoMenuMsg::Paste(" again")).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi there again");

    let undo = KeyEvent {
        code: KeyCode::Char('z'),
        mods: KeyMods::CTRL,
    };
    let redo = KeyEvent {
        code: KeyCode::Char('y'),
        mods: KeyMods::CTRL,
    };

    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi there");

    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi ");

    // Prior typed undo history is still intact!
    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");

    // Redo walks back forward through typed and programmatic edits.
    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi ");
    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hi there");

    // Replacing TextEditor with a fresh instance (e.g. after submit) resets undo history.
    backend.dispatch(UndoMenuMsg::Reset("")).unwrap();
    backend.render();
    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");
}

#[test]
fn context_menu_keeps_static_caret_without_selection_and_hides_caret_with_selection() {
    struct CaretMenuHarness;

    impl Component for CaretMenuHarness {
        type Properties = ();
        type State = TextAreaMenuState;
        type Message = TextAreaMenuMsg;

        fn create_state(&self, _: &()) -> Self::State {
            let mut editor = tui_lipan::TextEditor::new("hello world");
            editor.set_cursor(5);
            TextAreaMenuState {
                editor,
                anchor: None,
                copied_on_action: None,
            }
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                TextAreaMenuMsg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state.editor);
                    Update::full()
                }
                TextAreaMenuMsg::Open(mouse) => {
                    ctx.state.anchor = Some((mouse.x, mouse.y));
                    Update::full()
                }
                TextAreaMenuMsg::Action(_) | TextAreaMenuMsg::Close => {
                    ctx.state.anchor = None;
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let area = TextArea::bound(&ctx.state.editor)
                .caret_shape(tui_lipan::style::CaretShape::Bar)
                .caret_blinking(true)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(TextAreaMenuMsg::Edited))
                .on_context_menu(ctx.link().callback(TextAreaMenuMsg::Open))
                .key("caret-editor");
            ContextMenu::new(area)
                .open(ctx.state.anchor.is_some())
                .anchor(ctx.state.anchor)
                .entries([ContextMenuEntry::action("paste", "Paste")])
                .on_action(ctx.link().callback(TextAreaMenuMsg::Action))
                .on_close(ctx.link().callback(|()| TextAreaMenuMsg::Close))
                .into()
        }
    }

    let mut backend = TestBackend::new(CaretMenuHarness);
    backend.focus_key(&tui_lipan::Key::from("caret-editor"));
    backend.render();

    // Focused editor with no selection has a blinking bar caret.
    let before = backend.capture_frame();
    let before_cursor = before.cursor.expect("focused caret visible");
    assert!(before_cursor.blinking);

    // Right-click away from the caret cell so the menu doesn't occlude it.
    backend
        .send_mouse(MouseEvent {
            x: 15,
            y: 2,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(backend.state().anchor.is_some());

    // With no selection, the insertion caret stays visible at the same cell and turns static.
    let while_open = backend.capture_frame();
    let static_cursor = while_open
        .cursor
        .expect("static insertion caret should remain visible while context menu is open");
    assert_eq!(
        (static_cursor.x, static_cursor.y),
        (before_cursor.x, before_cursor.y)
    );
    assert!(
        !static_cursor.blinking,
        "preserved caret must be static (non-blinking) while context menu owns focus"
    );

    // Dismissing restores focus and blinking caret.
    backend
        .send_key(KeyEvent {
            code: KeyCode::Esc,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    let after_close = backend.capture_frame();
    assert!(
        after_close.cursor.expect("caret after close").blinking,
        "blinking caret should resume once context menu closes"
    );

    // When text IS selected, opening the context menu keeps the caret hidden.
    backend.state_mut().editor.set_cursor(11);
    backend.state_mut().editor.set_anchor(Some(6));
    backend.render();
    backend
        .send_mouse(MouseEvent {
            x: 15,
            y: 2,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(
        backend.capture_frame().cursor.is_none(),
        "caret must stay hidden when a selection is active"
    );
}

#[test]
fn intercepted_paste_replacing_bound_editor_resets_undo_history() {
    #[derive(Clone, Debug)]
    enum Msg {
        Edited(tui_lipan::TextAreaEvent),
        ReplaceOnPaste,
    }

    struct PasteResetHarness;

    impl Component for PasteResetHarness {
        type Properties = ();
        type State = tui_lipan::TextEditor;
        type Message = Msg;

        fn create_state(&self, _: &()) -> Self::State {
            tui_lipan::TextEditor::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state);
                    Update::full()
                }
                Msg::ReplaceOnPaste => {
                    ctx.state = tui_lipan::TextEditor::new("fresh");
                    let end = ctx.state.text().len();
                    ctx.state.set_cursor(end);
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(Msg::Edited))
                .on_text_paste(ctx.link().callback(|_| Msg::ReplaceOnPaste))
                .key("paste-reset-editor")
        }
    }

    let mut backend = TestBackend::new(PasteResetHarness);
    backend.focus_key(&tui_lipan::Key::from("paste-reset-editor"));
    backend.render();

    for ch in ['o', 'l', 'd'] {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(ch),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    assert_eq!(backend.state().text(), "old");

    // Paste triggers on_text_paste, which replaces the bound TextEditor with a fresh instance.
    backend.send_paste("ignored").unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "fresh");

    // Pressing Ctrl+Z must NOT restore content from the previous editor instance.
    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('z'),
            mods: KeyMods::CTRL,
        })
        .unwrap();
    backend.render();
    assert_eq!(
        backend.state().text(),
        "fresh",
        "fresh TextEditor history epoch must override pending external paste and reset history"
    );
}

#[test]
fn programmatic_undo_and_redo_preserve_runtime_history() {
    #[derive(Clone, Debug)]
    enum Msg {
        Edited(tui_lipan::TextAreaEvent),
        Insert(&'static str),
        Undo,
        Redo,
    }

    struct ProgrammaticUndoHarness;

    impl Component for ProgrammaticUndoHarness {
        type Properties = ();
        type State = tui_lipan::TextEditor;
        type Message = Msg;

        fn create_state(&self, _: &()) -> Self::State {
            tui_lipan::TextEditor::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state);
                    Update::full()
                }
                Msg::Insert(s) => {
                    ctx.state.insert_str(s);
                    Update::full()
                }
                Msg::Undo => {
                    ctx.state.undo();
                    Update::full()
                }
                Msg::Redo => {
                    ctx.state.redo();
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(Msg::Edited))
                .key("prog-undo-editor")
        }
    }

    let mut backend = TestBackend::new(ProgrammaticUndoHarness);
    backend.focus_key(&tui_lipan::Key::from("prog-undo-editor"));
    backend.render();

    for ch in ['h', 'e', 'l', 'l', 'o'] {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(ch),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    assert_eq!(backend.state().text(), "hello");

    backend.dispatch(Msg::Insert(" world")).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello world");

    let undo = KeyEvent {
        code: KeyCode::Char('z'),
        mods: KeyMods::CTRL,
    };
    let redo = KeyEvent {
        code: KeyCode::Char('y'),
        mods: KeyMods::CTRL,
    };

    // Flow A: Programmatic undo followed by programmatic redo preserves full keyboard undo stack.
    backend.dispatch(Msg::Undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello");

    backend.dispatch(Msg::Redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello world");

    // Flow B: Programmatic undo leaves both earlier typed undo history and redo stack intact.
    backend.dispatch(Msg::Undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello");

    // Keyboard undo steps back through the original typed text.
    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");

    // Keyboard redo walks forward through both typed text and the programmatically undone edit.
    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello");

    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello world");
}

#[test]
fn captured_static_caret_respects_suspended_vim_insert_mode() {
    struct VimCaretMenuHarness;

    impl Component for VimCaretMenuHarness {
        type Properties = ();
        type State = TextAreaMenuState;
        type Message = TextAreaMenuMsg;

        fn create_state(&self, _: &()) -> Self::State {
            let mut editor = tui_lipan::TextEditor::new("hello world");
            editor.set_cursor(5);
            TextAreaMenuState {
                editor,
                anchor: None,
                copied_on_action: None,
            }
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                TextAreaMenuMsg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state.editor);
                    Update::full()
                }
                TextAreaMenuMsg::Open(mouse) => {
                    ctx.state.anchor = Some((mouse.x, mouse.y));
                    Update::full()
                }
                TextAreaMenuMsg::Action(_) | TextAreaMenuMsg::Close => {
                    ctx.state.anchor = None;
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let area = TextArea::bound(&ctx.state.editor)
                .vim_motions(true)
                .caret_shape(tui_lipan::style::CaretShape::Block)
                .caret_blinking(true)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(TextAreaMenuMsg::Edited))
                .on_context_menu(ctx.link().callback(TextAreaMenuMsg::Open))
                .key("vim-caret-editor");
            ContextMenu::new(area)
                .open(ctx.state.anchor.is_some())
                .anchor(ctx.state.anchor)
                .entries([ContextMenuEntry::action("paste", "Paste")])
                .on_action(ctx.link().callback(TextAreaMenuMsg::Action))
                .on_close(ctx.link().callback(|()| TextAreaMenuMsg::Close))
                .into()
        }
    }

    let mut backend = TestBackend::new(VimCaretMenuHarness);
    backend.focus_key(&tui_lipan::Key::from("vim-caret-editor"));
    backend.render();

    // Enter Vim Insert mode -> focused block caret becomes a blinking bar.
    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('i'),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    let focused_cursor = backend
        .capture_frame()
        .cursor
        .expect("focused vim insert caret");
    assert_eq!(focused_cursor.shape, tui_lipan::capture::CursorShape::Bar);
    assert!(focused_cursor.blinking);

    // Open the context menu (focus moves to menu List): captured static caret must still
    // resolve the suspended editor's Insert mode as a steady Bar, not Normal-mode Block.
    backend
        .send_mouse(MouseEvent {
            x: 15,
            y: 2,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.render();
    assert!(backend.state().anchor.is_some());

    let static_cursor = backend
        .capture_frame()
        .cursor
        .expect("static vim insert caret while context menu is open");
    assert_eq!(
        static_cursor.shape,
        tui_lipan::capture::CursorShape::Bar,
        "captured static caret should reflect suspended Vim Insert mode"
    );
    assert!(!static_cursor.blinking);
}

#[test]
fn mixing_keyboard_undo_and_programmatic_undo_never_panics_and_shares_history() {
    #[derive(Clone, Debug)]
    enum Msg {
        Edited(tui_lipan::TextAreaEvent),
        Insert(&'static str),
        Undo,
        Redo,
    }

    struct SharedHistoryHarness;

    impl Component for SharedHistoryHarness {
        type Properties = ();
        type State = tui_lipan::TextEditor;
        type Message = Msg;

        fn create_state(&self, _: &()) -> Self::State {
            tui_lipan::TextEditor::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state);
                    Update::full()
                }
                Msg::Insert(s) => {
                    ctx.state.insert_str(s);
                    Update::full()
                }
                Msg::Undo => {
                    ctx.state.undo();
                    Update::full()
                }
                Msg::Redo => {
                    ctx.state.redo();
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(Msg::Edited))
                .key("shared-history-editor")
        }
    }

    let mut backend = TestBackend::new(SharedHistoryHarness);
    backend.focus_key(&tui_lipan::Key::from("shared-history-editor"));
    backend.render();

    // 1. Type "hello" via keyboard.
    for ch in ['h', 'e', 'l', 'l', 'o'] {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(ch),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    }
    assert_eq!(backend.state().text(), "hello");
    assert!(
        backend.state().can_undo(),
        "keyboard edits on a bound TextArea should populate the bound TextEditor's undo stack"
    );

    // 2. Programmatically insert " world".
    backend.dispatch(Msg::Insert(" world")).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello world");

    let undo = KeyEvent {
        code: KeyCode::Char('z'),
        mods: KeyMods::CTRL,
    };
    let redo = KeyEvent {
        code: KeyCode::Char('y'),
        mods: KeyMods::CTRL,
    };

    // 3. Keyboard Ctrl+Z reverts the programmatic insertion back to "hello".
    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello");

    // 4. Programmatic undo reverts the earlier keyboard typing back to "" without panicking.
    backend.dispatch(Msg::Undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");
    assert!(!backend.state().can_undo());
    assert!(backend.state().can_redo());

    // 5. Keyboard Ctrl+Y redoes the keyboard typing, then programmatic redo redoes " world".
    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello");

    backend.dispatch(Msg::Redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "hello world");
    assert!(!backend.state().can_redo());
}

#[test]
fn coalesced_programmatic_edits_preserve_exact_undo_and_redo_semantics() {
    #[derive(Clone, Debug)]
    enum Msg {
        Edited(tui_lipan::TextAreaEvent),
        Insert(&'static str),
        Undo,
    }

    struct CoalescedHarness;

    impl Component for CoalescedHarness {
        type Properties = ();
        type State = tui_lipan::TextEditor;
        type Message = Msg;

        fn create_state(&self, _: &()) -> Self::State {
            tui_lipan::TextEditor::default()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(ev) => {
                    ev.apply_to(&mut ctx.state);
                    Update::full()
                }
                Msg::Insert(s) => {
                    ctx.state.insert_str(s);
                    Update::full()
                }
                Msg::Undo => {
                    ctx.state.undo();
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .height(Length::Px(4))
                .on_change(ctx.link().callback(Msg::Edited))
                .key("coalesced-editor")
        }
    }

    let mut backend = TestBackend::new(CoalescedHarness);
    backend.focus_key(&tui_lipan::Key::from("coalesced-editor"));
    backend.render();

    // Two consecutive programmatic insertions coalesce into a single edit in TextEditor.
    backend.dispatch(Msg::Insert("a")).unwrap();
    backend.render();
    backend.dispatch(Msg::Insert("b")).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "ab");

    // Programmatic undo reverts the coalesced edit back to "".
    backend.dispatch(Msg::Undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");

    let undo = KeyEvent {
        code: KeyCode::Char('z'),
        mods: KeyMods::CTRL,
    };
    let redo = KeyEvent {
        code: KeyCode::Char('y'),
        mods: KeyMods::CTRL,
    };

    // Keyboard Ctrl+Y redoes the exact coalesced edit ("ab"), and Ctrl+Z undoes back to "".
    backend.send_key(redo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "ab");

    backend.send_key(undo).unwrap();
    backend.render();
    assert_eq!(backend.state().text(), "");
}
