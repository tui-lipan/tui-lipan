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
/// Full action script (`click:#go; wait:200`), taking precedence over keys.
const SCRIPT_ENV: &str = "TUI_LIPAN_RECORD_SCRIPT";
/// Pause held after each key, in milliseconds.
const KEY_DELAY_ENV: &str = "TUI_LIPAN_RECORD_KEY_DELAY_MS";
/// Hold on the final frame, in milliseconds.
const SETTLE_ENV: &str = "TUI_LIPAN_RECORD_SETTLE_MS";
/// Directory for truecolor PNG frame export, for encoding to video.
const FRAMES_ENV: &str = "TUI_LIPAN_RECORD_FRAMES";

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
    /// Actions to play, in order.
    pub(super) actions: Vec<crate::ui_snapshot::Action>,
    /// Pause held after each key.
    pub(super) key_delay: Duration,
    /// Hold on the final frame.
    pub(super) settle: Duration,
    /// Optional directory receiving one PNG per frame.
    pub(super) frames_dir: Option<PathBuf>,
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

        // A bad script is reported rather than ignored: recording the wrong
        // sequence silently is worse than refusing to record.
        let actions = match crate::ui_snapshot::resolve_actions(
            std::env::var(SCRIPT_ENV).ok().as_deref(),
            std::env::var(KEYS_ENV).ok().as_deref(),
        ) {
            Ok(actions) => actions,
            Err(err) => return Some(Err(err)),
        };

        Some(Ok(Self {
            path: PathBuf::from(raw),
            viewport: env_viewport().unwrap_or(DEFAULT_VIEWPORT),
            fps: env_u64(FPS_ENV)
                .and_then(|fps| u16::try_from(fps).ok())
                .unwrap_or(DEFAULT_FPS)
                .max(1),
            actions,
            key_delay: Duration::from_millis(
                env_u64(KEY_DELAY_ENV).unwrap_or(DEFAULT_KEY_DELAY_MS),
            ),
            settle: Duration::from_millis(env_u64(SETTLE_ENV).unwrap_or(DEFAULT_SETTLE_MS)),
            frames_dir: std::env::var_os(FRAMES_ENV)
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
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

/// Writes one PNG per captured frame, when frame export is requested.
///
/// Frames land at a constant rate - one per tick, including unchanged ones -
/// because an encoder reproduces timing from a numbered sequence. Unchanged
/// frames reuse the previous encode rather than paying for it again.
pub(super) struct FrameSink {
    dir: Option<PathBuf>,
    #[cfg(feature = "ui-snapshot-png")]
    written: usize,
    #[cfg(feature = "ui-snapshot-png")]
    previous: Option<(crate::capture::CapturedFrame, Vec<u8>)>,
}

impl FrameSink {
    /// Create a sink writing into `dir`, or an inert one when `dir` is `None`.
    pub(super) fn new(dir: Option<PathBuf>) -> crate::Result<Self> {
        if let Some(dir) = dir.as_ref() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self {
            dir,
            #[cfg(feature = "ui-snapshot-png")]
            written: 0,
            #[cfg(feature = "ui-snapshot-png")]
            previous: None,
        })
    }

    /// Write `frame` as the next numbered PNG, if frame export is on.
    #[cfg(feature = "ui-snapshot-png")]
    pub(super) fn capture(&mut self, frame: &crate::capture::CapturedFrame) -> crate::Result<()> {
        let Some(dir) = self.dir.as_ref() else {
            return Ok(());
        };
        let bytes = match self.previous.as_ref() {
            Some((last, bytes)) if last == frame => bytes.clone(),
            _ => frame.to_png(&crate::capture::PngOptions::default())?,
        };
        std::fs::write(dir.join(format!("frame_{:05}.png", self.written)), &bytes)?;
        self.previous = Some((frame.clone(), bytes));
        self.written += 1;
        Ok(())
    }

    /// Frame export needs `ui-snapshot-png`; without it this is a no-op.
    #[cfg(not(feature = "ui-snapshot-png"))]
    pub(super) fn capture(&mut self, _frame: &crate::capture::CapturedFrame) -> crate::Result<()> {
        Ok(())
    }

    /// Print the written frame count and a ready-to-run encode command.
    pub(super) fn report(&self, fps: u16) {
        let Some(dir) = self.dir.as_ref() else {
            return;
        };
        #[cfg(not(feature = "ui-snapshot-png"))]
        {
            let _ = fps;
            eprintln!(
                "warning: no frames written to {}; frame export needs `--features ui-snapshot-png`",
                dir.display()
            );
        }
        #[cfg(feature = "ui-snapshot-png")]
        {
            println!("wrote {} frames to {}", self.written, dir.display());
            println!(
                "  ffmpeg -framerate {fps} -i {}/frame_%05d.png -pix_fmt yuv420p -movflags +faststart out.mp4",
                dir.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(fps: u16) -> HeadlessRecordConfig {
        HeadlessRecordConfig {
            path: PathBuf::from("out.cast"),
            viewport: DEFAULT_VIEWPORT,
            fps,
            actions: Vec::new(),
            key_delay: Duration::from_millis(DEFAULT_KEY_DELAY_MS),
            settle: Duration::from_millis(DEFAULT_SETTLE_MS),
            frames_dir: None,
        }
    }

    #[test]
    fn a_sink_without_a_directory_is_inert() {
        let mut sink = FrameSink::new(None).expect("inert sink");
        // Capturing into an inert sink must be a silent no-op, not an error:
        // the recorder always feeds it, whether or not frames were requested.
        let frame = crate::capture::CapturedFrame {
            viewport: DEFAULT_VIEWPORT,
            width: 1,
            height: 1,
            cells: vec![crate::capture::CapturedCell {
                symbol: " ".to_owned(),
                fg: crate::style::Color::Reset,
                bg: crate::style::Color::Reset,
                underline_color: crate::style::Color::Reset,
                modifiers: crate::capture::CellModifiers::default(),
            }],
            cursor: None,
        };
        sink.capture(&frame).expect("inert capture succeeds");
        sink.report(30);
    }

    #[test]
    fn step_is_the_reciprocal_of_the_frame_rate() {
        assert_eq!(config(30).step(), Duration::from_secs_f64(1.0 / 30.0));
        assert_eq!(config(1).step(), Duration::from_secs(1));
    }
}
