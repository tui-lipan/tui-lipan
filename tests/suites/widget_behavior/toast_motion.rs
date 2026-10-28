//! Toasts that slide in and out, and glide when the stack around them changes.
use std::time::Duration;
use tui_lipan::prelude::*;
use tui_lipan::{OverlayId, TestBackend};

const WIDTH: u16 = 40;

fn timing(ms: u64) -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(ms),
        easing: Easing::Linear,
    }
}

struct Host {
    toast: Toast,
}

#[derive(Default)]
struct State {
    shown: Vec<OverlayId>,
}

enum Msg {
    Push(&'static str),
    DismissFirst,
    DismissLast,
}

impl Component for Host {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Push(message) => {
                let mut toast = self.toast.clone();
                toast.message = message.into();
                let id = ctx.toast().push(toast);
                ctx.state.shown.push(id);
            }
            Msg::DismissFirst => {
                let id = ctx.state.shown.remove(0);
                ctx.toast().dismiss(id);
            }
            Msg::DismissLast => {
                if let Some(id) = ctx.state.shown.pop() {
                    ctx.toast().dismiss(id);
                }
            }
        }
        Update::full()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        Text::new("Background").into()
    }
}

fn backend(toast: Toast) -> TestBackend<Host> {
    let mut backend = TestBackend::new(Host { toast });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: WIDTH,
        h: 16,
    });
    backend.render();
    backend
}

/// Column and row of the first cell showing `needle`.
fn locate(backend: &TestBackend<Host>, needle: &str) -> Option<(usize, usize)> {
    backend
        .capture_frame()
        .to_fixed_grid_lines()
        .iter()
        .enumerate()
        .find_map(|(row, line)| {
            line.find(needle)
                .map(|byte| (line[..byte].chars().count(), row))
        })
}

fn sliding(enter_ms: u64, exit_ms: u64) -> Toast {
    Toast::new("").duration(10.0).animation(
        VisibilityAnimation::new()
            .enter(timing(enter_ms))
            .exit(timing(exit_ms))
            .slide(1.0, 0.0),
    )
}

#[test]
fn a_sliding_toast_travels_in_from_its_own_width_and_leaves_the_same_way() {
    let mut backend = backend(sliding(200, 200));
    backend.dispatch(Msg::Push("ALPHA")).unwrap();
    backend.advance(Duration::from_millis(200));
    let (rest_x, rest_y) = locate(&backend, "ALPHA").expect("toast at rest");
    // `ALPHA` in a bordered frame is seven columns wide; half hidden is four of them, rounded.
    let mut entering = self::backend(sliding(200, 200));
    entering.dispatch(Msg::Push("ALPHA")).unwrap();
    entering.advance(Duration::from_millis(100));
    let frame = entering.capture_frame().to_fixed_grid_lines();
    assert_eq!(
        frame[rest_y].chars().nth(rest_x + 4),
        Some('A'),
        "half-entered toast paints four columns right of its place:\n{}",
        frame.join("\n")
    );

    backend.dispatch(Msg::DismissFirst).unwrap();
    backend.advance(Duration::from_millis(100));
    let frame = backend.capture_frame().to_fixed_grid_lines();
    assert_eq!(
        frame[rest_y].chars().nth(rest_x + 4),
        Some('A'),
        "half-exited toast is back at the same offset:\n{}",
        frame.join("\n")
    );
    backend.advance(Duration::from_millis(100));
    assert_eq!(locate(&backend, "ALPHA"), None);
}

#[test]
fn a_toast_without_slide_stays_in_place_while_it_fades() {
    let mut backend = backend(
        Toast::new("")
            .duration(10.0)
            .animation(VisibilityAnimation::new().enter(timing(200))),
    );
    backend.dispatch(Msg::Push("ALPHA")).unwrap();
    backend.advance(Duration::from_millis(100));
    let entering = locate(&backend, "ALPHA");
    backend.advance(Duration::from_millis(100));
    assert_eq!(entering, locate(&backend, "ALPHA"));
}

fn restacking() -> Toast {
    Toast::new("")
        .duration(10.0)
        .animation(VisibilityAnimation::new().enter(timing(0)).exit(timing(0)))
        .restack(timing(200))
}

#[test]
fn a_restacking_toast_glides_up_when_a_newer_one_arrives_beneath_it() {
    let mut backend = backend(restacking());
    backend.dispatch(Msg::Push("ALPHA")).unwrap();
    backend.advance(Duration::from_millis(50));
    let (_, low) = locate(&backend, "ALPHA").unwrap();

    // It starts from where it was painted, under the newcomer that now owns that place.
    backend.dispatch(Msg::Push("BRAVO")).unwrap();
    backend.render();
    assert_eq!(locate(&backend, "ALPHA"), None);
    backend.advance(Duration::from_millis(100));
    let (_, mid) = locate(&backend, "ALPHA").unwrap();
    backend.advance(Duration::from_millis(100));
    let (_, high) = locate(&backend, "ALPHA").unwrap();
    // A three-row toast plus the one-row gap.
    assert_eq!(high, low - 4);
    assert!(high < mid && mid < low, "{high} < {mid} < {low}");
}

#[test]
fn a_restacking_toast_closes_the_gap_a_dismissed_one_leaves() {
    let mut backend = backend(restacking());
    backend.dispatch(Msg::Push("ALPHA")).unwrap();
    backend.dispatch(Msg::Push("BRAVO")).unwrap();
    backend.advance(Duration::from_millis(300));
    let (_, high) = locate(&backend, "ALPHA").unwrap();
    let (_, low) = locate(&backend, "BRAVO").unwrap();

    backend.dispatch(Msg::DismissLast).unwrap();
    backend.advance(Duration::from_millis(100));
    let (_, moving) = locate(&backend, "ALPHA").unwrap();
    assert!(high < moving && moving < low, "{high} < {moving} < {low}");
    backend.advance(Duration::from_millis(300));
    assert_eq!(locate(&backend, "ALPHA").map(|(_, row)| row), Some(low));
}

#[test]
fn a_restacking_toast_paints_like_a_plain_one_at_rest() {
    let surface = |toast: Toast| {
        toast
            .duration(10.0)
            .min_width(Length::Px(12))
            .max_width(Length::Px(16))
            .frame_style(Style::new().bg_alpha(Color::Rgb(40, 40, 120), 0.5))
            .animation(VisibilityAnimation::new().enter(timing(0)))
    };
    let mut plain = backend(surface(Toast::new("")));
    let mut wrapped = backend(surface(Toast::new("")).restack(timing(200)));
    for backend in [&mut plain, &mut wrapped] {
        backend
            .dispatch(Msg::Push("a long message that wraps"))
            .unwrap();
        backend.advance(Duration::from_millis(50));
    }
    assert_eq!(plain.capture_frame(), wrapped.capture_frame());
}

#[test]
fn a_restacking_toast_keeps_a_percentage_width_cap() {
    let surface = |toast: Toast| {
        toast
            .duration(10.0)
            // Lift the frame's intrinsic minimum so the percentage cap decides the width.
            .min_width(Length::Px(0))
            .max_width(Length::Percent(80))
            .animation(VisibilityAnimation::new().enter(timing(0)))
    };
    let mut plain = backend(surface(Toast::new("")));
    let mut wrapped = backend(surface(Toast::new("")).restack(timing(200)));
    for backend in [&mut plain, &mut wrapped] {
        backend
            .dispatch(Msg::Push(
                "a long message that wraps across several lines of the toast",
            ))
            .unwrap();
        backend.advance(Duration::from_millis(50));
    }
    assert_eq!(plain.capture_frame(), wrapped.capture_frame());
}
