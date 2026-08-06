//! Environment-driven headless snapshot capture.
//!
//! Setting `TUI_LIPAN_SNAPSHOT` makes [`AppRunner::run`](super::AppRunner::run)
//! render the app off-screen, write one snapshot artifact, and exit without ever
//! entering raw mode. That lets any existing example or binary be captured
//! without editing its source - the workflow that otherwise forces a throwaway
//! harness.
//!
//! No terminal is opened, so this path also works on CI runners and in agent
//! sessions that have no tty.

use std::path::PathBuf;

use crate::style::Rect;
use crate::ui_snapshot::{UiSnapshotFileFormat, UiSnapshotOptions};

/// Output path; setting it switches the runner into headless snapshot mode.
const SNAPSHOT_ENV: &str = "TUI_LIPAN_SNAPSHOT";
/// Viewport override, formatted `WIDTHxHEIGHT` (for example `120x40`).
const VIEWPORT_ENV: &str = "TUI_LIPAN_SNAPSHOT_VIEWPORT";
/// Number of render/message passes to settle before capturing.
const FRAMES_ENV: &str = "TUI_LIPAN_SNAPSHOT_FRAMES";
/// Focus advances applied before capturing, for a visible focus state.
const FOCUS_ENV: &str = "TUI_LIPAN_SNAPSHOT_FOCUS";
/// Comma-separated key script dispatched before capturing.
const KEYS_ENV: &str = "TUI_LIPAN_SNAPSHOT_KEYS";
/// Full action script (`click:#go; wait:200`), taking precedence over keys.
const SCRIPT_ENV: &str = "TUI_LIPAN_SNAPSHOT_SCRIPT";
/// Set to `1` to capture with [`UiSnapshotOptions::diagnostic`].
const DIAGNOSTIC_ENV: &str = "TUI_LIPAN_SNAPSHOT_DIAGNOSTIC";

/// Viewport used when `TUI_LIPAN_SNAPSHOT_VIEWPORT` is unset.
///
/// Wider and taller than the usual 80x24 so flex distribution has room to show
/// itself, which is what a design capture is normally for.
const DEFAULT_VIEWPORT: Rect = Rect {
    x: 0,
    y: 0,
    w: 100,
    h: 30,
};

/// Resolved headless snapshot request read from the environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HeadlessSnapshotConfig {
    /// Where to write the artifact.
    pub(super) path: PathBuf,
    /// Format routed from the path extension.
    pub(super) format: UiSnapshotFileFormat,
    /// Layout viewport.
    pub(super) viewport: Rect,
    /// Render/message passes before capture. At least 1.
    pub(super) frames: usize,
    /// Focus advances before capture.
    pub(super) focus_steps: usize,
    /// Actions performed before capture, in order.
    pub(super) actions: Vec<crate::ui_snapshot::Action>,
    /// Whether to capture with diagnostic describe options.
    pub(super) diagnostic: bool,
}

impl HeadlessSnapshotConfig {
    /// Read a snapshot request from the process environment.
    ///
    /// Returns `None` when `TUI_LIPAN_SNAPSHOT` is unset or empty, leaving the
    /// runner on its normal interactive path.
    pub(super) fn from_env() -> Option<crate::Result<Self>> {
        let raw = std::env::var_os(SNAPSHOT_ENV)?;
        if raw.is_empty() {
            return None;
        }
        let path = PathBuf::from(raw);
        let format = UiSnapshotFileFormat::from_path(&path);

        // A bad script is reported rather than ignored: capturing the wrong
        // state silently is worse than refusing to capture.
        let actions = match crate::ui_snapshot::resolve_actions(
            std::env::var(SCRIPT_ENV).ok().as_deref(),
            std::env::var(KEYS_ENV).ok().as_deref(),
        ) {
            Ok(actions) => actions,
            Err(err) => return Some(Err(err)),
        };

        Some(Ok(Self {
            path,
            format,
            viewport: env_viewport().unwrap_or(DEFAULT_VIEWPORT),
            frames: env_usize(FRAMES_ENV).unwrap_or(1).max(1),
            focus_steps: env_usize(FOCUS_ENV).unwrap_or(0),
            actions,
            diagnostic: std::env::var(DIAGNOSTIC_ENV).as_deref() == Ok("1"),
        }))
    }

    /// Describe options matching this request.
    pub(super) fn snapshot_options(&self) -> UiSnapshotOptions {
        if self.diagnostic {
            UiSnapshotOptions::diagnostic()
        } else {
            UiSnapshotOptions::default()
        }
    }

    /// Returns the feature to enable when the path asked for a format this build
    /// cannot produce.
    ///
    /// Writing markdown to a `.png` path is a silent trap - the file looks like an
    /// image and fails only when something tries to read it - so the caller warns
    /// rather than degrading quietly.
    pub(super) fn missing_format_feature(&self) -> Option<&'static str> {
        if self.format != UiSnapshotFileFormat::Markdown {
            return None;
        }
        let extension = self.path.extension()?;
        if extension.eq_ignore_ascii_case("png") {
            return Some("ui-snapshot-png");
        }
        if extension.eq_ignore_ascii_case("json") {
            return Some("ui-snapshot-json");
        }
        None
    }
}

/// Parse `TUI_LIPAN_SNAPSHOT_VIEWPORT` as `WIDTHxHEIGHT`.
///
/// Returns `None` for malformed or zero-sized values so the caller falls back to
/// the default rather than laying out at an unusable size.
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
fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_the_fallback_for_unknown_extensions() {
        assert_eq!(
            UiSnapshotFileFormat::from_path(std::path::Path::new("out.txt")),
            UiSnapshotFileFormat::Markdown
        );
        assert_eq!(
            UiSnapshotFileFormat::from_path(std::path::Path::new("out")),
            UiSnapshotFileFormat::Markdown
        );
    }

    #[cfg(feature = "ui-snapshot-png")]
    #[test]
    fn png_extension_routes_to_png_case_insensitively() {
        assert_eq!(
            UiSnapshotFileFormat::from_path(std::path::Path::new("shot.PNG")),
            UiSnapshotFileFormat::Png
        );
    }

    #[test]
    fn diagnostic_flag_selects_diagnostic_options() {
        let config = HeadlessSnapshotConfig {
            path: PathBuf::from("out.md"),
            format: UiSnapshotFileFormat::Markdown,
            viewport: DEFAULT_VIEWPORT,
            frames: 1,
            focus_steps: 0,
            actions: Vec::new(),
            diagnostic: true,
        };
        assert_eq!(config.snapshot_options(), UiSnapshotOptions::diagnostic());
    }
}
