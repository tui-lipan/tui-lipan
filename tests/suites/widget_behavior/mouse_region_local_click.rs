//! `MouseRegion` press, release, and click events carry coordinates relative to the region,
//! including presses bubbled up from an interactive descendant.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::style::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Down,
    Up,
    Click,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Hit {
    phase: Phase,
    local: (u16, u16),
    size: (u16, u16),
}

fn hit(phase: Phase) -> impl Fn(MouseRegionEvent) -> Hit {
    move |event| Hit {
        phase,
        local: (event.local_x, event.local_y),
        size: (event.target_w, event.target_h),
    }
}

struct LocalApp {
    bubbled: bool,
}

impl Component for LocalApp {
    type Message = Hit;
    type Properties = ();
    type State = Vec<Hit>;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Vec::new()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let content: Element = if self.bubbled {
            Button::new("button").width(Length::Px(6)).into()
        } else {
            Text::new("region")
                .width(Length::Px(6))
                .height(Length::Px(2))
                .into()
        };
        let region = MouseRegion::new()
            .bubble_mouse_down(self.bubbled)
            .on_mouse_down(ctx.link().callback(hit(Phase::Down)))
            .on_mouse_up(ctx.link().callback(hit(Phase::Up)))
            .on_click(ctx.link().callback(hit(Phase::Click)))
            .child(content);
        // The region sits at column 4, row 2.
        VStack::new()
            .child(Text::new("").height(Length::Px(2)))
            .child(
                HStack::new()
                    .align(Align::Start)
                    .child(Text::new("").width(Length::Px(4)))
                    .child(region),
            )
            .into()
    }
}

fn press_and_release(bubbled: bool, x: u16, y: u16) -> Vec<Hit> {
    let mut backend = TestBackend::new(LocalApp { bubbled });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    backend.state().clone()
}

#[test]
fn press_release_and_click_report_region_local_coordinates() {
    let at = |phase| Hit {
        phase,
        local: (3, 1),
        size: (6, 2),
    };
    assert_eq!(
        press_and_release(false, 7, 3),
        vec![at(Phase::Down), at(Phase::Up), at(Phase::Click)]
    );
}

#[test]
fn bubbled_press_reports_coordinates_relative_to_the_region() {
    let hits = press_and_release(true, 6, 2);
    assert_eq!(
        hits.first(),
        Some(&Hit {
            phase: Phase::Down,
            local: (2, 0),
            size: (6, 1),
        })
    );
}
