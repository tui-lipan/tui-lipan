use crate::app::context::{SurfaceMode, ViewportMode};
use crate::app::input::convert::translate_mouse_to_viewport;
use crate::core::event::MouseEvent;
use crate::style::Rect;

use super::ViewportMetrics;

#[derive(Default)]
pub(crate) struct InlineSurfaceState {
    pub(crate) viewport_metrics: ViewportMetrics,
    pub(crate) inline_cursor_offset: u16,
    pub(crate) last_terminal_size: (u16, u16),
    pub(crate) transcript_expanded: bool,
    pub(crate) transcript_reset_pending: bool,
    pub(crate) expanded_live_viewport_height: u16,
}

pub(crate) struct SurfaceDriver {
    mode: SurfaceMode,
    pub(crate) inline: InlineSurfaceState,
}

impl SurfaceDriver {
    pub(crate) fn new(mode: SurfaceMode) -> Self {
        let normalized = mode.normalized();
        Self {
            mode: normalized,
            inline: InlineSurfaceState::default(),
        }
    }

    pub(crate) fn mode(&self) -> SurfaceMode {
        self.mode
    }

    pub(crate) fn is_inline(&self) -> bool {
        self.mode.is_inline()
    }

    pub(crate) fn is_transcript(&self) -> bool {
        matches!(self.mode, SurfaceMode::InlineTranscript { .. })
    }

    pub(crate) fn content_bounds(&self, width: u16, height: u16) -> Rect {
        match self.mode {
            SurfaceMode::Fullscreen => Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            SurfaceMode::InlineTranscript {
                height: inline_height,
                ..
            } => {
                if self.inline.transcript_expanded {
                    let layout_h = if self.inline.expanded_live_viewport_height > 0 {
                        self.inline.expanded_live_viewport_height.min(height).max(1)
                    } else {
                        inline_height.min(height).max(1)
                    };
                    Rect {
                        x: 0,
                        y: 0,
                        w: width.saturating_sub(1).max(1),
                        h: layout_h,
                    }
                } else {
                    Rect {
                        x: 0,
                        y: 0,
                        w: width.saturating_sub(1).max(1),
                        h: inline_height.min(height).max(1),
                    }
                }
            }
            SurfaceMode::InlineEphemeral {
                height: inline_height,
            } => Rect {
                x: 0,
                y: 0,
                w: width.saturating_sub(1).max(1),
                h: inline_height.min(height).max(1),
            },
        }
    }

    pub(crate) fn convert_mouse_event(
        &self,
        mouse: MouseEvent,
        viewport_metrics: ViewportMetrics,
    ) -> Option<MouseEvent> {
        match self.mode.viewport_mode() {
            ViewportMode::Fullscreen => Some(mouse),
            ViewportMode::Inline { .. } => translate_mouse_to_viewport(
                mouse,
                viewport_metrics.x,
                viewport_metrics.y,
                viewport_metrics.width,
                viewport_metrics.height,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SurfaceDriver;
    use crate::app::context::{InlineStartupPolicy, SurfaceMode};
    use crate::core::event::{KeyMods, MouseEvent, MouseKind};
    use crate::style::Rect;

    #[test]
    fn inline_surface_state_single_owner() {
        let driver = SurfaceDriver::new(SurfaceMode::InlineEphemeral { height: 3 });
        assert_eq!(driver.inline.inline_cursor_offset, 0);
        assert_eq!(driver.inline.last_terminal_size, (0, 0));
        assert!(!driver.inline.transcript_expanded);
        assert!(!driver.inline.transcript_reset_pending);
        assert_eq!(driver.inline.expanded_live_viewport_height, 0);
        assert_eq!(driver.inline.viewport_metrics.width, 0);
        assert_eq!(driver.inline.viewport_metrics.height, 0);
    }

    #[test]
    fn inline_surface_driver_routes_runner_state() {
        let fullscreen = SurfaceDriver::new(SurfaceMode::Fullscreen);
        let inline = SurfaceDriver::new(SurfaceMode::InlineTranscript {
            height: 4,
            startup: InlineStartupPolicy::PreserveHost,
        });

        assert!(!fullscreen.is_inline());
        assert_eq!(
            fullscreen.content_bounds(80, 24),
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            }
        );

        assert!(inline.is_inline());
        assert_eq!(
            inline.content_bounds(80, 24),
            Rect {
                x: 0,
                y: 0,
                w: 79,
                h: 4,
            }
        );
        assert_eq!(
            inline.content_bounds(1, 1),
            Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            }
        );

        let mouse = MouseEvent {
            x: 2,
            y: 5,
            kind: MouseKind::Moved,
            mods: KeyMods::default(),
        };
        assert_eq!(
            inline.convert_mouse_event(
                mouse,
                crate::app::runner::ViewportMetrics {
                    x: 0,
                    y: 4,
                    width: 80,
                    height: 4,
                },
            ),
            Some(MouseEvent {
                x: 2,
                y: 1,
                kind: MouseKind::Moved,
                mods: KeyMods::default(),
            }),
        );
    }

    #[test]
    fn inline_transcript_expanded_uses_live_viewport_height_not_screen() {
        let mut driver = SurfaceDriver::new(SurfaceMode::InlineTranscript {
            height: 4,
            startup: InlineStartupPolicy::PreserveHost,
        });
        driver.inline.transcript_expanded = true;
        assert_eq!(
            driver.content_bounds(80, 24),
            Rect {
                x: 0,
                y: 0,
                w: 79,
                h: 4,
            }
        );
        driver.inline.expanded_live_viewport_height = 14;
        assert_eq!(
            driver.content_bounds(80, 24),
            Rect {
                x: 0,
                y: 0,
                w: 79,
                h: 14,
            }
        );
    }
}
