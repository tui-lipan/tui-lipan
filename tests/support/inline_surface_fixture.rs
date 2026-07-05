use tui_lipan::prelude::*;
use tui_lipan::{SurfaceMode, TestBackend};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResizeStep {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl ResizeStep {
    pub(crate) const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BoundsSnapshot {
    pub(crate) viewport: Rect,
    pub(crate) bounds: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StormSummary {
    pub(crate) iterations: usize,
    pub(crate) final_bounds: Rect,
    pub(crate) final_viewport: Rect,
}

pub(crate) fn replay_resize_fixture<C>(
    backend: &mut TestBackend<C>,
    surface_mode: SurfaceMode,
    steps: &[ResizeStep],
) -> Vec<BoundsSnapshot>
where
    C: Component,
{
    let mut snapshots = Vec::with_capacity(steps.len());

    for step in steps {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: step.width,
            h: step.height,
        };
        backend.set_viewport(viewport);
        backend.render();

        snapshots.push(BoundsSnapshot {
            viewport,
            bounds: expected_content_bounds(surface_mode, step.width, step.height),
        });
    }

    snapshots
}

pub(crate) fn expected_content_bounds(surface_mode: SurfaceMode, width: u16, height: u16) -> Rect {
    match surface_mode {
        SurfaceMode::Fullscreen => Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        },
        SurfaceMode::InlineEphemeral {
            height: inline_height,
        }
        | SurfaceMode::InlineTranscript {
            height: inline_height,
            ..
        } => Rect {
            x: 0,
            y: 0,
            w: width.saturating_sub(1).max(1),
            h: inline_height.max(1).min(height).max(1),
        },
    }
}

pub(crate) fn expected_inline_resize_clear_from(old_y: u16, new_y: u16) -> u16 {
    old_y.min(new_y)
}

pub(crate) fn expected_logical_anchor_after_height_change(anchor: u16, new_height: u16) -> u16 {
    anchor.min(new_height.saturating_sub(1))
}

pub(crate) fn replay_streaming_resize_storm<C>(
    backend: &mut TestBackend<C>,
    mode: SurfaceMode,
    fixture: &[ResizeStep],
    rounds: usize,
    mut stream_tick: impl FnMut(&mut TestBackend<C>, usize),
) -> StormSummary
where
    C: Component,
{
    let mut iterations = 0usize;
    for round in 0..rounds {
        for (step_index, step) in fixture.iter().enumerate() {
            stream_tick(backend, round * fixture.len() + step_index);
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: step.width,
                h: step.height,
            });
            backend.render();
            iterations += 1;
        }
    }

    let last = fixture.last().copied().unwrap_or(ResizeStep::new(1, 1));
    StormSummary {
        iterations,
        final_bounds: expected_content_bounds(mode, last.width, last.height),
        final_viewport: Rect {
            x: 0,
            y: 0,
            w: last.width,
            h: last.height,
        },
    }
}
