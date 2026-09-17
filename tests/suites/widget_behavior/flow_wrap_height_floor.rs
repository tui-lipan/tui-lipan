//! A wrapping Auto `Flow` in a bounded `VStack` keeps its wrap height. Flex and
//! Px siblings yield first, stopping at a hard min; `Flow::shrinkable` still
//! truncates on purpose. An explicit Px Flow height is not that floor.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Host {
    list: Length,
    list_min: Option<Length>,
    flow_height: Length,
    shrinkable_flow: bool,
}

fn wrap_flow(height: Length, shrinkable: bool) -> Element {
    Flow::new()
        .gap(1)
        .row_gap(0)
        .width(Length::Flex(1))
        .height(height)
        .shrinkable(shrinkable)
        .child(Text::new("AAAAAAAAAA"))
        .child(Text::new("BBBBBBBBBB"))
        .child(Text::new("CCCCCCCCCC"))
        .into()
}

fn list_block(height: Length, min: Option<Length>) -> Element {
    let text = Text::new("1\n2\n3\n4").height(height);
    match min {
        Some(min) => Element::from(text).min_height(min),
        None => text.into(),
    }
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
                list_block(self.list, self.list_min),
                wrap_flow(self.flow_height, self.shrinkable_flow),
            }
        }
    }
}

fn render(
    list: Length,
    list_min: Option<Length>,
    flow_height: Length,
    shrinkable_flow: bool,
) -> Vec<String> {
    let mut backend = TestBackend::new(Host {
        list,
        list_min,
        flow_height,
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
        .collect()
}

fn joined(lines: &[String]) -> String {
    lines.join("\n")
}

fn assert_whole_wrap(lines: &[String]) {
    let text = joined(lines);
    for whole in ["AAAAAAAAAA", "BBBBBBBBBB", "CCCCCCCCCC"] {
        assert!(
            text.contains(whole),
            "expected whole wrap item '{whole}'; got:\n{text}"
        );
    }
}

fn wrap_items_visible(lines: &[String]) -> usize {
    ["AAAAAAAAAA", "BBBBBBBBBB", "CCCCCCCCCC"]
        .iter()
        .filter(|item| lines.iter().any(|line| line.contains(*item)))
        .count()
}

#[test]
fn wrapping_flow_keeps_wrap_height_when_px_sibling_oversubscribes() {
    // Each 10-cell chip needs its own row at width 12, so the Flow's wrap
    // height is 3. A Px(4) sibling would consume the 5-row stack unless it
    // yields.
    let lines = render(Length::Px(4), None, Length::Auto, false);
    assert_whole_wrap(&lines);
    assert!(
        lines.iter().any(|line| line == "1"),
        "px sibling should remain visible after yielding; got:\n{}",
        joined(&lines)
    );
}

#[test]
fn wrapping_flow_keeps_wrap_height_when_flex_sibling_takes_leftover() {
    let lines = render(Length::Flex(1), None, Length::Auto, false);
    assert_whole_wrap(&lines);
    assert!(
        lines.iter().any(|line| line == "1"),
        "flex sibling should keep leftover after the wrap floor; got:\n{}",
        joined(&lines)
    );
}

#[test]
fn shrinkable_flow_still_yields_below_wrap_height() {
    let lines = render(Length::Px(4), None, Length::Auto, true);
    let text = joined(&lines);
    assert_eq!(
        lines
            .get(..4)
            .map(|rows| rows.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["1", "2", "3", "4"]),
        "px sibling should keep its requested 4-row height; got:\n{text}"
    );
    assert!(
        wrap_items_visible(&lines) < 3,
        "shrinkable Flow should clip below wrap height; got:\n{text}"
    );
}

#[test]
fn px_sibling_does_not_yield_below_hard_min_for_flow_floor() {
    // Same 4+3-into-5 squeeze as the core wrap-floor case, but the Px sibling
    // also has min_height(3). Tier 3 may take one cell, not two.
    let lines = render(Length::Px(4), Some(Length::Px(3)), Length::Auto, false);
    let text = joined(&lines);
    assert_eq!(
        lines
            .get(..3)
            .map(|rows| rows.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["1", "2", "3"]),
        "px sibling must not shrink below min_height(3); got:\n{text}"
    );
    assert_ne!(
        lines.get(2).map(String::as_str),
        Some("AAAAAAAAAA"),
        "yielding below the hard min would let wrap occupy row 3; got:\n{text}"
    );
}

#[test]
fn fixed_height_flow_does_not_activate_auto_wrap_floor() {
    // A Px Flow can still wrap internally, but that is not the Auto wrap-height
    // floor: the rigid list keeps Px(4) and the Flow is clipped to leftover.
    let lines = render(Length::Px(4), None, Length::Px(4), false);
    let text = joined(&lines);
    assert_eq!(
        lines
            .get(..4)
            .map(|rows| rows.iter().map(String::as_str).collect::<Vec<_>>()),
        Some(vec!["1", "2", "3", "4"]),
        "px list should keep its requested height next to a Px Flow; got:\n{text}"
    );
    assert!(
        wrap_items_visible(&lines) < 3,
        "fixed-height Flow should not force the list to yield; got:\n{text}"
    );
}
