//! `Modal::reserve_height` positions; `Modal::max_height` bounds. They are independent,
//! so a modal can be taller than the band it is centered by.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

/// A modal whose content is `rows` tall, with an optional band and cap.
struct Host {
    rows: u16,
    reserve: Option<Length>,
    max: Option<Length>,
}

impl Component for Host {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &Self::Properties) -> Self::State {}
    fn update(&mut self, _: Self::Message, _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut body = VStack::new();
        for i in 0..self.rows {
            body = body.child(Text::new(format!("row{i}")));
        }
        let mut modal = Modal::new()
            .border(false)
            .padding(0)
            .width(Length::Px(20))
            .height(Length::Auto)
            .frame_style(Style::new().bg(Color::Blue))
            .child(body);
        if let Some(reserve) = self.reserve {
            modal = modal.reserve_height(reserve);
        }
        if let Some(max) = self.max {
            modal = modal.max_height(max);
        }
        ui! {
            ZStack::new() => {
                Spacer::new(),
                modal,
            }
        }
    }
}

/// (top_gap, height) of the modal within a `h`-row viewport.
fn measure(rows: u16, reserve: Option<Length>, max: Option<Length>, h: u16) -> (u16, u16) {
    let mut backend = TestBackend::new(Host { rows, reserve, max });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h,
    });
    backend.render();
    let frame = backend.capture_frame();
    let backdrop = frame.cell(0, 0).bg;
    let painted: Vec<u16> = (0..h)
        .filter(|&y| frame.cell(20, y).bg != backdrop)
        .collect();
    let top = *painted.first().expect("modal not rendered");
    let bottom = *painted.last().expect("modal not rendered");
    (top, bottom - top + 1)
}

#[test]
fn without_reserve_height_the_modal_centers_by_its_own_height() {
    assert_eq!(measure(10, None, None, 40), (15, 10));
    assert_eq!(measure(4, None, None, 40), (18, 4));
}

#[test]
fn reserve_height_fixes_the_top_edge_as_content_shrinks() {
    let band = Some(Length::Percent(50));
    // Band is 20 rows, centered in 40 => top edge at 10, regardless of content height.
    assert_eq!(measure(18, band, None, 40), (10, 18));
    assert_eq!(measure(10, band, None, 40), (10, 10));
    assert_eq!(measure(1, band, None, 40), (10, 1));
}

#[test]
fn content_taller_than_the_band_keeps_the_top_edge_and_extends_past_it() {
    let band = Some(Length::Percent(50));
    // 24 rows of content in a 20-row band: top stays at 10, the modal runs 4 rows past
    // the band's bottom rather than re-centering.
    assert_eq!(measure(24, band, None, 40), (10, 24));
}

#[test]
fn max_height_bounds_growth_independently_of_the_band() {
    let band = Some(Length::Percent(50));
    let cap = Some(Length::Px(22));
    // Content wants 30 rows; the cap clamps it to 22, while the band still puts it at 10.
    assert_eq!(measure(30, band, cap, 40), (10, 22));
    // Under the cap, the modal hugs its content.
    assert_eq!(measure(12, band, cap, 40), (10, 12));
}

#[test]
fn reserve_height_larger_than_the_viewport_clamps_to_the_top() {
    assert_eq!(measure(5, Some(Length::Percent(100)), None, 20), (0, 5));
}
