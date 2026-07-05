//! Regression: two `Auto`-width `Flow`s in a `SpaceBetween` `HStack` must each
//! wrap onto more rows when the row is too narrow, instead of being shrunk into
//! truncation or clipped to a stale one-row height.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Demo;

fn group(labels: &[&str], gap: u16, shrinkable: bool) -> Element {
    let mut flow = Flow::new()
        .gap(gap)
        .row_gap(0)
        .width(Length::Auto)
        .shrinkable(shrinkable);
    for l in labels {
        flow = flow.child(Text::new((*l).to_string()).overflow(Overflow::Ellipsis));
    }
    flow.into()
}

struct Pair(bool);

impl Component for Pair {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &Self::Properties) -> Self::State {}
    fn update(&mut self, _: Self::Message, _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        // Left group rigid; right group shrinkable when `self.0`.
        ui! {
            HStack::new().height(Length::Auto).justify(Justify::SpaceBetween) => {
                group(&["Allow once", "Allow always", "Reject"], 1, false),
                group(&["ctrl+f fullscreen", "select", "enter confirm"], 2, self.0),
            }
        }
    }
}

impl Component for Demo {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &Self::Properties) -> Self::State {}
    fn update(&mut self, _: Self::Message, _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        ui! {
            HStack::new().height(Length::Auto).justify(Justify::SpaceBetween) => {
                group(&["Allow once", "Allow always", "Reject"], 1, false),
                group(&["ctrl+f fullscreen", "select", "enter confirm"], 2, false),
            }
        }
    }
}

fn render(w: u16) -> Vec<String> {
    let mut b = TestBackend::new(Demo);
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w,
        h: 8,
    });
    b.render();
    b.capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

fn render_pair(shrinkable_right: bool, w: u16) -> Vec<String> {
    let mut b = TestBackend::new(Pair(shrinkable_right));
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w,
        h: 8,
    });
    b.render();
    b.capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

#[test]
fn both_groups_wrap_whole_when_too_narrow_for_one_row() {
    // ~44 cells: the two groups can't share a single row, but each group's
    // widest item still fits, so both must wrap onto multiple rows with every
    // item intact.
    let lines = render(44);
    let joined = lines.join("\n");

    for whole in [
        "Allow once",
        "Allow always",
        "Reject",
        "ctrl+f fullscreen",
        "enter confirm",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(whole)),
            "expected whole item '{whole}'; got:\n{joined}"
        );
    }
    assert!(
        !joined.contains('…'),
        "an item was truncated to an ellipsis; got:\n{joined}"
    );
}

#[test]
fn shrinkable_group_yields_width_and_truncates_so_rigid_group_stays_whole() {
    // At a width too narrow to fit even both groups' min-content side by side,
    // the rigid left group keeps every item whole (wrapping to its widest item),
    // while the shrinkable right group yields below its min-content and
    // ellipsizes. The rigid group must never be dropped.
    let lines = render_pair(true, 24);
    let joined = lines.join("\n");

    for whole in ["Allow once", "Allow always", "Reject"] {
        assert!(
            lines.iter().any(|l| l.contains(whole)),
            "rigid item '{whole}' must stay whole; got:\n{joined}"
        );
    }
    assert!(
        joined.contains('…'),
        "shrinkable group should have ellipsized at this width; got:\n{joined}"
    );
}

#[test]
fn rigid_group_clips_rather_than_disappearing_at_extreme_narrow() {
    // Below the width where even the rigid group's widest item fits, it clips its
    // content (one visible cell as the hard floor) instead of being dropped — so
    // the priority group stays at least partially visible. The shrinkable group
    // yields / drops first.
    let lines = render_pair(true, 12);
    let joined = lines.join("\n");

    assert!(
        lines.iter().any(|l| l.contains("Allow")),
        "rigid group must remain at least partially visible (clipped), not vanish; got:\n{joined}"
    );
}

#[test]
fn shrinkable_group_hugs_right_edge_with_a_gap_when_there_is_room() {
    // Wide enough that the rigid group stays on one row and the shrinkable group
    // fits too: SpaceBetween must keep the rigid group at the left and push the
    // shrinkable group to the right edge, leaving a visible gap between them
    // (rather than the shrinkable content hugging the rigid group with empty
    // space trailing on the right).
    let lines = render_pair(true, 80);
    let rigid_line = lines
        .iter()
        .find(|l| l.contains("Reject"))
        .expect("rigid group should be on one row");

    // The rigid group is whole on this line, and the shrinkable group's first
    // item appears after a run of gap spaces.
    assert!(rigid_line.contains("Allow once") && rigid_line.contains("Allow always"));
    let after_reject = &rigid_line[rigid_line.find("Reject").unwrap() + "Reject".len()..];
    let hint_pos = after_reject
        .find("ctrl+f fullscreen")
        .expect("shrinkable item should share the rigid group's row");
    assert!(
        after_reject[..hint_pos]
            .chars()
            .filter(|c| *c == ' ')
            .count()
            >= 3,
        "expected a gap between the left and right groups (SpaceBetween), not the \
         hints hugging the buttons; got:\n{}",
        lines.join("\n")
    );
}

#[test]
fn wrapping_rigid_group_donates_slack_so_shrinkable_sibling_stays_readable() {
    // Once the rigid left group is squeezed below its one-row width it wraps, and
    // its widest row is narrower than its allocation. That dead width is handed
    // to the shrinkable right group, which then shows its items in full instead
    // of ellipsizing next to empty space.
    let lines = render_pair(true, 30);
    let joined = lines.join("\n");

    for whole in [
        "Allow once",
        "Allow always",
        "Reject",
        "ctrl+f fullscreen",
        "enter confirm",
    ] {
        assert!(
            lines.iter().any(|l| l.contains(whole)),
            "expected whole item '{whole}' after slack redistribution; got:\n{joined}"
        );
    }
    assert!(
        !joined.contains('…'),
        "no item should be truncated once the rigid group donates its slack; got:\n{joined}"
    );
}
