//! Roving focus for DatePicker and Radio (one tab stop + arrows).

use std::cell::Cell;
use std::rc::Rc;

use tui_lipan::Key;
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct DatePickerHarness {
    selects: Rc<Cell<u32>>,
    last: Rc<Cell<(i32, u32, u32)>>,
}

#[derive(Clone)]
enum DateMsg {
    Select(DateEvent),
    PrevMonth,
    NextMonth,
}

struct DateState {
    year: i32,
    month: u32,
    day: u32,
}

impl Component for DatePickerHarness {
    type Message = DateMsg;
    type Properties = ();
    type State = DateState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        DateState {
            year: 2024,
            month: 1,
            day: 31,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            DateMsg::Select(ev) => {
                self.selects.set(self.selects.get() + 1);
                self.last.set((ev.year, ev.month, ev.day));
                ctx.state.year = ev.year;
                ctx.state.month = ev.month;
                ctx.state.day = ev.day;
            }
            DateMsg::PrevMonth => {
                if ctx.state.month == 1 {
                    ctx.state.year -= 1;
                    ctx.state.month = 12;
                } else {
                    ctx.state.month -= 1;
                }
            }
            DateMsg::NextMonth => {
                if ctx.state.month == 12 {
                    ctx.state.year += 1;
                    ctx.state.month = 1;
                } else {
                    ctx.state.month += 1;
                }
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Input::new("before").key("before"))
            .child(
                DatePicker::new()
                    .year(ctx.state.year)
                    .month(ctx.state.month)
                    .day(ctx.state.day)
                    .title(Some("Due"))
                    .border(false)
                    .on_select(ctx.link().callback(DateMsg::Select))
                    .on_prev_month(ctx.link().callback(|_| DateMsg::PrevMonth))
                    .on_next_month(ctx.link().callback(|_| DateMsg::NextMonth)),
            )
            .child(Input::new("after").key("after"))
            .into()
    }
}

struct RadioHarness {
    changes: Rc<Cell<usize>>,
}

#[derive(Clone)]
enum RadioMsg {
    Change(usize),
}

struct RadioState {
    selected: usize,
}

impl Component for RadioHarness {
    type Message = RadioMsg;
    type Properties = ();
    type State = RadioState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        RadioState { selected: 1 }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            RadioMsg::Change(i) => {
                self.changes.set(i);
                ctx.state.selected = i;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Input::new("before").key("before"))
            .child(
                Radio::new(["a", "b", "c"])
                    .selected(Some(ctx.state.selected))
                    .on_change(ctx.link().callback(RadioMsg::Change)),
            )
            .child(
                Radio::new(["x", "y", "z"])
                    .selected(Some(0))
                    .on_change(Callback::new(|_| {})),
            )
            .child(Input::new("after").key("after"))
            .into()
    }
}

#[test]
fn date_picker_is_one_tab_stop_and_arrows_move_day() {
    let selects = Rc::new(Cell::new(0));
    let last = Rc::new(Cell::new((0, 0, 0)));
    let mut backend = TestBackend::new(DatePickerHarness {
        selects: selects.clone(),
        last: last.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 20,
    });
    backend.render();

    backend.focus_next();
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));

    backend.focus_next();
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("__tui_lipan_datepicker:Due"))
    );

    backend.focus_next();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    backend.focus_prev();
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("__tui_lipan_datepicker:Due"))
    );

    backend
        .send_key(KeyEvent {
            code: KeyCode::Left,
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(selects.get(), 1);
    assert_eq!(last.get(), (2024, 1, 30));
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("__tui_lipan_datepicker:Due"))
    );

    backend
        .send_key(KeyEvent {
            code: KeyCode::PageDown,
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(last.get(), (2024, 2, 29));
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("__tui_lipan_datepicker:Due"))
    );
}

#[test]
fn radio_is_one_tab_stop_and_arrows_change_selection() {
    let changes = Rc::new(Cell::new(99));
    let mut backend = TestBackend::new(RadioHarness {
        changes: changes.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();

    backend.focus_next();
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));
    backend.focus_next();
    let first_key = backend.focused_key().cloned();
    assert!(first_key.is_some());
    backend.focus_next();
    let second_key = backend.focused_key().cloned();
    assert_ne!(first_key, second_key);
    backend.focus_next();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    backend.focus_prev();
    backend.focus_prev();
    assert_eq!(backend.focused_key(), first_key.as_ref());

    backend
        .send_key(KeyEvent {
            code: KeyCode::Down,
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(changes.get(), 2);
    assert_eq!(backend.focused_key(), first_key.as_ref());

    backend
        .send_key(KeyEvent {
            code: KeyCode::Up,
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert_eq!(changes.get(), 1);
}

#[test]
#[should_panic(expected = "duplicate focusable element key")]
fn duplicate_focusable_keys_panic_in_debug() {
    struct DupKeys;
    impl Component for DupKeys {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(Button::new("one").key("shared"))
                .child(Button::new("two").key("shared"))
                .into()
        }
    }

    let mut backend = TestBackend::new(DupKeys);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    });
    backend.render();
}
