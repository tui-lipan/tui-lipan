//! Environment-driven headless recording.
//!
//! Setting `TUI_LIPAN_RECORD` makes [`AppRunner::run`](super::AppRunner::run)
//! play a key script against the app off-screen and write an asciinema cast,
//! without entering raw mode. Any existing app or example becomes a recordable
//! demo with no source change.
//!
//! Timing is a synthetic fixed step, so runs are reproducible; see
//! [`Recording`](crate::Recording) for what that trades away.

use std::path::PathBuf;
use std::time::Duration;

use crate::style::Rect;

/// Output path; setting it switches the runner into headless recording mode.
const RECORD_ENV: &str = "TUI_LIPAN_RECORD";
/// Viewport override, formatted `WIDTHxHEIGHT`.
const VIEWPORT_ENV: &str = "TUI_LIPAN_RECORD_VIEWPORT";
/// Capture rate in frames per second.
const FPS_ENV: &str = "TUI_LIPAN_RECORD_FPS";
/// Comma-separated key script to play.
const KEYS_ENV: &str = "TUI_LIPAN_RECORD_KEYS";
/// Pause held after each key, in milliseconds.
const KEY_DELAY_ENV: &str = "TUI_LIPAN_RECORD_KEY_DELAY_MS";
/// Hold on the final frame, in milliseconds.
const SETTLE_ENV: &str = "TUI_LIPAN_RECORD_SETTLE_MS";

/// Viewport used when `TUI_LIPAN_RECORD_VIEWPORT` is unset.
const DEFAULT_VIEWPORT: Rect = Rect {
    x: 0,
    y: 0,
    w: 100,
    h: 30,
};
/// Capture rate used when `TUI_LIPAN_RECORD_FPS` is unset.
const DEFAULT_FPS: u16 = 30;
/// Key pause used when `TUI_LIPAN_RECORD_KEY_DELAY_MS` is unset.
const DEFAULT_KEY_DELAY_MS: u64 = 400;
/// Final hold used when `TUI_LIPAN_RECORD_SETTLE_MS` is unset.
const DEFAULT_SETTLE_MS: u64 = 1200;

/// Resolved headless recording request read from the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HeadlessRecordConfig {
    /// Where to write the cast.
    pub(super) path: PathBuf,
    /// Layout viewport.
    pub(super) viewport: Rect,
    /// Capture rate, at least 1.
    pub(super) fps: u16,
    /// Key events to play, in order.
    pub(super) keys: Vec<crate::core::event::KeyEvent>,
    /// Pause held after each key.
    pub(super) key_delay: Duration,
    /// Hold on the final frame.
    pub(super) settle: Duration,
}

impl HeadlessRecordConfig {
    /// Read a recording request from the process environment.
    ///
    /// Returns `None` when `TUI_LIPAN_RECORD` is unset or empty, leaving the
    /// runner on its normal interactive path.
    pub(super) fn from_env() -> Option<crate::Result<Self>> {
        let raw = std::env::var_os(RECORD_ENV)?;
        if raw.is_empty() {
            return None;
        }

        // A bad key script is reported rather than ignored: recording the wrong
        // sequence silently is worse than refusing to record.
        let keys = match std::env::var(KEYS_ENV) {
            Ok(script) => match crate::ui_snapshot::parse_key_script(&script) {
                Ok(keys) => keys,
                Err(err) => return Some(Err(err)),
            },
            Err(_) => Vec::new(),
        };

        Some(Ok(Self {
            path: PathBuf::from(raw),
            viewport: env_viewport().unwrap_or(DEFAULT_VIEWPORT),
            fps: env_u64(FPS_ENV)
                .and_then(|fps| u16::try_from(fps).ok())
                .unwrap_or(DEFAULT_FPS)
                .max(1),
            keys,
            key_delay: Duration::from_millis(
                env_u64(KEY_DELAY_ENV).unwrap_or(DEFAULT_KEY_DELAY_MS),
            ),
            settle: Duration::from_millis(env_u64(SETTLE_ENV).unwrap_or(DEFAULT_SETTLE_MS)),
        }))
    }

    /// Seconds per captured frame.
    pub(super) fn step(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from(self.fps))
    }
}

/// Parse `TUI_LIPAN_RECORD_VIEWPORT` as `WIDTHxHEIGHT`.
fn env_viewport() -> Option<Rect> {
    let raw = std::env::var(VIEWPORT_ENV).ok()?;
    let (w, h) = raw.split_once(['x', 'X'])?;
    let w: u16 = w.trim().parse().ok()?;
    let h: u16 = h.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some(Rect { x: 0, y: 0, w, h })
}

/// Parse a non-negative integer environment variable.
fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(fps: u16) -> HeadlessRecordConfig {
        HeadlessRecordConfig {
            path: PathBuf::from("out.cast"),
            viewport: DEFAULT_VIEWPORT,
            fps,
            keys: Vec::new(),
            key_delay: Duration::from_millis(DEFAULT_KEY_DELAY_MS),
            settle: Duration::from_millis(DEFAULT_SETTLE_MS),
        }
    }

    #[test]
    fn step_is_the_reciprocal_of_the_frame_rate() {
        assert_eq!(config(30).step(), Duration::from_secs_f64(1.0 / 30.0));
        assert_eq!(config(1).step(), Duration::from_secs(1));
    }
}
