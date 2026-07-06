#[allow(dead_code)]
mod support {
    pub(crate) mod inline_surface_fixture;
}

use support::inline_surface_fixture::{
    ResizeStep, expected_content_bounds, expected_inline_resize_clear_from,
    expected_logical_anchor_after_height_change, replay_resize_fixture,
};
use tui_lipan::prelude::*;
use tui_lipan::{InlineHeight, InlineStartupPolicy, SurfaceMode, TestBackend};

struct ViewportProbe;

impl Component for ViewportProbe {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let vp = ctx.viewport();
        Text::new(format!("{}x{}", vp.w, vp.h)).into()
    }
}

#[test]
fn resize_fixture_replays_width_and_height_changes() {
    let mode = SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(5),
        startup: InlineStartupPolicy::PreserveHost,
    };
    let mut backend = TestBackend::new(ViewportProbe);
    let fixture = [
        ResizeStep::new(80, 24),
        ResizeStep::new(64, 20),
        ResizeStep::new(37, 11),
        ResizeStep::new(19, 8),
        ResizeStep::new(120, 40),
    ];

    let snapshots = replay_resize_fixture(&mut backend, mode, &fixture);

    assert_eq!(snapshots.len(), fixture.len());
    for (snapshot, step) in snapshots.iter().zip(fixture.iter()) {
        assert_eq!(snapshot.viewport.w, step.width);
        assert_eq!(snapshot.viewport.h, step.height);

        let expected = expected_content_bounds(mode, step.width, step.height);
        assert_eq!(snapshot.bounds, expected);
        assert_eq!(snapshot.bounds.w, step.width.saturating_sub(1).max(1));
        assert_eq!(snapshot.bounds.h, 5_u16.min(step.height).max(1));
    }
}

#[test]
fn tiny_terminal_fixture_reports_mode_specific_fallback() {
    let tiny = ResizeStep::new(1, 1);

    let fullscreen_bounds =
        expected_content_bounds(SurfaceMode::Fullscreen, tiny.width, tiny.height);
    assert_eq!(
        fullscreen_bounds,
        Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        }
    );

    let ephemeral_bounds = expected_content_bounds(
        SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(6) },
        tiny.width,
        tiny.height,
    );
    assert_eq!(
        ephemeral_bounds,
        Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        }
    );

    let transcript_bounds = expected_content_bounds(
        SurfaceMode::InlineTranscript {
            height: InlineHeight::Fixed(9),
            startup: InlineStartupPolicy::ClearHost,
        },
        tiny.width,
        tiny.height,
    );
    assert_eq!(
        transcript_bounds,
        Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        }
    );
}

#[test]
fn inline_ephemeral_rapid_shrink_keeps_surface_clean() {
    let mode = SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(4) };
    let mut backend = TestBackend::new(ViewportProbe);
    let fixture = [
        ResizeStep::new(120, 24),
        ResizeStep::new(96, 24),
        ResizeStep::new(72, 24),
        ResizeStep::new(48, 24),
        ResizeStep::new(24, 24),
    ];

    let snapshots = replay_resize_fixture(&mut backend, mode, &fixture);

    for (snapshot, step) in snapshots.iter().zip(fixture.iter()) {
        assert_eq!(
            snapshot.bounds,
            expected_content_bounds(mode, step.width, step.height)
        );
    }

    let viewport_y_sequence = [8_u16, 6, 5, 3, 2];
    for pair in viewport_y_sequence.windows(2) {
        let old_y = pair[0];
        let new_y = pair[1];
        let clear_from = expected_inline_resize_clear_from(old_y, new_y);
        assert!(clear_from <= old_y);
        assert!(clear_from <= new_y);
    }
}

#[test]
fn inline_ephemeral_height_shrink_preserves_logical_anchor() {
    let mode = SurfaceMode::InlineEphemeral { height: InlineHeight::Fixed(8) };
    let mut backend = TestBackend::new(ViewportProbe);
    let fixture = [
        ResizeStep::new(80, 20),
        ResizeStep::new(80, 14),
        ResizeStep::new(80, 10),
        ResizeStep::new(80, 6),
        ResizeStep::new(80, 3),
    ];

    let snapshots = replay_resize_fixture(&mut backend, mode, &fixture);

    let mut anchor = 6_u16;
    for snapshot in snapshots {
        assert_eq!(snapshot.bounds.w, 79);
        anchor = expected_logical_anchor_after_height_change(anchor, snapshot.bounds.h);
        assert!(anchor < snapshot.bounds.h);
    }
    assert_eq!(anchor, 2);
}
