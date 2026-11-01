use std::{cell::RefCell, rc::Rc};

use tui_lipan::{
    TestBackend,
    core::event::{MouseButton, MouseKind},
    prelude::*,
};

struct Regions {
    events: Rc<RefCell<Vec<(&'static str, MouseRegionEvent)>>>,
    inner_enabled: bool,
}
impl Component for Regions {
    type Properties = ();
    type Message = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let outer = self.events.clone();
        let inner = self.events.clone();
        MouseRegion::new()
            .on_middle_click(Callback::new(move |e| {
                outer.borrow_mut().push(("outer", e))
            }))
            .child(
                Frame::new().border(false).padding((1, 2)).child(
                    MouseRegion::new()
                        .enabled(self.inner_enabled)
                        .hit_test(|x, _| x < 2)
                        .on_middle_click(Callback::new(move |e| {
                            inner.borrow_mut().push(("inner", e))
                        }))
                        .child(
                            Button::new("row")
                                .width(Length::Px(10))
                                .on_click(Callback::new(|_| {
                                    panic!("middle click activated left callback")
                                })),
                        ),
                ),
            )
            .into()
    }
}

#[test]
fn nearest_accepting_region_owns_middle_click_and_reports_local_coordinates() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Regions {
        events: events.clone(),
        inner_enabled: true,
    });
    backend.render();
    let focused = backend.focused();
    for (x, y) in [(3, 1), (7, 1)] {
        for kind in [
            MouseKind::Down(MouseButton::Middle),
            MouseKind::Drag(MouseButton::Middle),
            MouseKind::Up(MouseButton::Middle),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x,
                    y,
                    kind,
                    mods: KeyMods::ALT,
                })
                .unwrap();
        }
    }
    assert_eq!(backend.focused(), focused);
    let events = events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "inner");
    assert_eq!((events[0].1.local_x, events[0].1.local_y), (1, 0));
    assert_eq!(events[0].1.mods, KeyMods::ALT);
    assert_eq!(events[0].1.kind, MouseKind::Down(MouseButton::Middle));
    assert_eq!(events[1].0, "outer");
}

#[test]
fn disabled_inner_handler_allows_enabled_ancestor_to_handle_middle_click() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Regions {
        events: events.clone(),
        inner_enabled: false,
    });
    backend.render();
    backend
        .send_mouse(MouseEvent {
            x: 3,
            y: 1,
            kind: MouseKind::Down(MouseButton::Middle),
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].0, "outer");
}
