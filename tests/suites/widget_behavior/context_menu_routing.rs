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
        Update::layout()
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
                    .active(0)
                    .focusable(false)
                    .on_change(Callback::new(move |event: TabsEvent| {
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
