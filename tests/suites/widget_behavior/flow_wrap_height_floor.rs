//! A wrapping Auto `Flow` in a bounded `VStack` keeps its wrap height. Flex and
//! Px siblings yield first; `Flow::shrinkable` still truncates on purpose.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Host {
    list: Length,
    shrinkable_flow: bool,
}

fn wrap_flow(shrinkable: bool) -> Element {
    Flow::new()
        .gap(1)
        .row_gap(0)
        .width(Length::Flex(1))
        .height(Length::Auto)
        .shrinkable(shrinkable)
        .child(Text::new("AAAAAAAAAA"))
        .child(Text::new("BBBBBBBBBB"))
        .child(Text::new("CCCCCCCCCC"))
        .into()
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
        ui! {
            VStack::new().width(Length::Px(12)).height(Length::Px(5)) => {
                Text::new("LIST").height(self.list),
                wrap_flow(self.shrinkable_flow),
            }
        }
    }
}

fn render(list: Length, shrinkable_flow: bool) -> String {
    let mut backend = TestBackend::new(Host {
        list,
        shrinkable_flow,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 5,
    });
    backend.render();
    backend
        .capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_whole_wrap(joined: &str) {
    for whole in ["AAAAAAAAAA", "BBBBBBBBBB", "CCCCCCCCCC"] {
        assert!(
            joined.contains(whole),
            "expected whole wrap item '{whole}'; got:\n{joined}"
        );
    }
}

#[test]
fn wrapping_flow_keeps_wrap_height_when_px_sibling_oversubscribes() {
    // Each 10-cell chip needs its own row at width 12, so the Flow's wrap
    // height is 3. A Px(4) sibling would consume the 5-row stack unless it
    // yields.
    let joined = render(Length::Px(4), false);
    assert_whole_wrap(&joined);
    assert!(
        joined.contains("LIST"),
        "px sibling should remain visible after yielding; got:\n{joined}"
    );
}

#[test]
fn wrapping_flow_keeps_wrap_height_when_flex_sibling_takes_leftover() {
    let joined = render(Length::Flex(1), false);
    assert_whole_wrap(&joined);
    assert!(
        joined.contains("LIST"),
        "flex sibling should keep leftover after the wrap floor; got:\n{joined}"
    );
}

#[test]
fn shrinkable_flow_still_yields_below_wrap_height() {
    let joined = render(Length::Px(4), true);
    assert!(
        joined.contains("LIST"),
        "px sibling should keep its requested height; got:\n{joined}"
    );
    let visible = ["AAAAAAAAAA", "BBBBBBBBBB", "CCCCCCCCCC"]
        .iter()
        .filter(|item| joined.contains(**item))
        .count();
    assert!(
        visible < 3,
        "shrinkable Flow should clip below wrap height; got:\n{joined}"
    );
}
