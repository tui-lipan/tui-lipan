use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

const VIEWPORT: Rect = Rect {
    x: 0,
    y: 0,
    w: 20,
    h: 3,
};

fn bottom_indicator_click() -> MouseEvent {
    MouseEvent {
        x: 0,
        y: VIEWPORT.h - 1,
        kind: MouseKind::Down(MouseButton::Left),
        mods: KeyMods::NONE,
    }
}

struct IndicatorList;

impl Component for IndicatorList {
    type Message = usize;
    type Properties = ();
    type State = Vec<usize>;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Vec::new()
    }

    fn update(&mut self, offset: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(offset);
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        List::new()
            .items((0..8).map(|index| ListItem::new(format!("row {index}"))))
            .show_scroll_indicators(true)
            .on_scroll_to(ctx.link().callback(|offset| offset))
            .focusable(false)
            .width(Length::Px(VIEWPORT.w))
            .height(Length::Px(VIEWPORT.h))
            .into()
    }
}

#[test]
fn list_overflow_indicator_emits_scroll_without_on_select() {
    let mut backend = TestBackend::new(IndicatorList);
    backend.set_viewport(VIEWPORT);
    backend.render();

    assert!(
        backend
            .send_mouse(bottom_indicator_click())
            .expect("click bottom list indicator")
    );
    assert_eq!(
        backend.state(),
        &[2],
        "on_scroll_to should receive the new list offset"
    );
}

struct IndicatorTable;

impl Component for IndicatorTable {
    type Message = usize;
    type Properties = ();
    type State = Vec<usize>;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Vec::new()
    }

    fn update(&mut self, offset: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(offset);
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Table::new()
            .rows((0..8).map(|index| TableRow::new([format!("row {index}")])))
            .show_scroll_indicators(true)
            .on_scroll_to(ctx.link().callback(|offset| offset))
            .focusable(false)
            .width(Length::Px(VIEWPORT.w))
            .height(Length::Px(VIEWPORT.h))
            .into()
    }
}

#[test]
fn table_overflow_indicator_emits_scroll_without_on_select() {
    let mut backend = TestBackend::new(IndicatorTable);
    backend.set_viewport(VIEWPORT);
    backend.render();

    assert!(
        backend
            .send_mouse(bottom_indicator_click())
            .expect("click bottom table indicator")
    );
    assert_eq!(
        backend.state(),
        &[1],
        "on_scroll_to should receive the new table offset"
    );
}
