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

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::style::Rect;
use crate::ui_snapshot::{UiSnapshotFileFormat, UiSnapshotOptions};

/// Output path; setting it switches the runner into headless snapshot mode.
const SNAPSHOT_ENV: &str = "TUI_LIPAN_SNAPSHOT";
/// Viewport override, formatted `WIDTHxHEIGHT` (for example `120x40`).
const VIEWPORT_ENV: &str = "TUI_LIPAN_SNAPSHOT_VIEWPORT";
/// Comma-separated viewport list, e.g. `80x24,120x30,160x40`. Wins over `_VIEWPORT`.
const VIEWPORTS_ENV: &str = "TUI_LIPAN_SNAPSHOT_VIEWPORTS";
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
/// Virtual-clock advance in milliseconds before capturing time-gated UI.
const ADVANCE_ENV: &str = "TUI_LIPAN_SNAPSHOT_ADVANCE_MS";
/// Real milliseconds to wait, pumping, so asynchronous work can land before capturing.
const SETTLE_ENV: &str = "TUI_LIPAN_SNAPSHOT_SETTLE_MS";

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
    /// Layout viewports, in capture order. Always contains at least one entry.
    pub(super) viewports: Vec<Rect>,
    /// When true, each capture is written as `{stem}-{w}x{h}{ext}` rather than
    /// the exact `path`. Set when `TUI_LIPAN_SNAPSHOT_VIEWPORTS` is used, so a
    /// single-size list still names the size; the historical `_VIEWPORT` path
    /// keeps writing to `path` unchanged.
    pub(super) suffix_viewports: bool,
    /// Render/message passes before capture. At least 1.
    pub(super) frames: usize,
    /// Focus advances before capture.
    pub(super) focus_steps: usize,
    /// Actions performed before capture, in order.
    pub(super) actions: Vec<crate::ui_snapshot::Action>,
    /// Whether to capture with diagnostic describe options.
    pub(super) diagnostic: bool,
    /// Virtual time to advance (and tick) before capturing.
    pub(super) advance: Duration,
    /// Real time to wait, pumping messages, before and after the action script.
    ///
    /// Distinct from `advance`, which moves a virtual clock and cannot make a subprocess answer or a
    /// socket deliver. Zero unless asked for, so no capture slows down by default.
    pub(super) settle: Duration,
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

        let (viewports, suffix_viewports) = match env_viewports() {
            Ok(resolved) => resolved,
            Err(err) => return Some(Err(err)),
        };

        Some(Ok(Self {
            path,
            format,
            viewports,
            suffix_viewports,
            frames: env_usize(FRAMES_ENV).unwrap_or(1).max(1),
            focus_steps: env_usize(FOCUS_ENV).unwrap_or(0),
            actions,
            diagnostic: std::env::var(DIAGNOSTIC_ENV).as_deref() == Ok("1"),
            advance: env_duration_ms(ADVANCE_ENV).unwrap_or(Duration::ZERO),
            settle: env_duration_ms(SETTLE_ENV).unwrap_or(Duration::ZERO),
        }))
    }

    /// First (or only) layout viewport.
    pub(super) fn primary_viewport(&self) -> Rect {
        self.viewports.first().copied().unwrap_or(DEFAULT_VIEWPORT)
    }

    /// Path to write for `viewport`.
    pub(super) fn output_path(&self, viewport: Rect) -> PathBuf {
        snapshot_output_path(&self.path, viewport, self.suffix_viewports)
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

/// Resolve the viewport list from `TUI_LIPAN_SNAPSHOT_VIEWPORTS` or `_VIEWPORT`.
///
/// `_VIEWPORTS` wins when set to a non-empty value, and those captures are
/// always suffixed. A malformed entry fails the run rather than being skipped,
/// because a dropped size silently captures a smaller matrix than requested.
/// Otherwise a single `_VIEWPORT` (or the default) writes to the exact snapshot
/// path, matching historical behaviour.
fn env_viewports() -> crate::Result<(Vec<Rect>, bool)> {
    if let Some(raw) = std::env::var(VIEWPORTS_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return Ok((parse_viewports(&raw)?, true));
    }
    Ok((vec![env_viewport().unwrap_or(DEFAULT_VIEWPORT)], false))
}

/// Parse `TUI_LIPAN_SNAPSHOT_VIEWPORT` as `WIDTHxHEIGHT`.
///
/// Returns `None` for malformed or zero-sized values so the caller falls back to
/// the default rather than laying out at an unusable size.
fn env_viewport() -> Option<Rect> {
    parse_viewport_spec(&std::env::var(VIEWPORT_ENV).ok()?)
}

/// Parse a comma-separated `WIDTHxHEIGHT` list.
///
/// Every entry must be a usable size. Skipping a typo would write fewer files
/// than the caller asked for, and a screenshot matrix with a missing breakpoint
/// is easy to miss.
fn parse_viewports(raw: &str) -> crate::Result<Vec<Rect>> {
    let mut viewports = Vec::new();
    for spec in raw.split(',') {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err(std::io::Error::other(format!(
                "invalid {VIEWPORTS_ENV} entry: empty size \
                 (check for a trailing comma); expected WIDTHxHEIGHT"
            ))
            .into());
        }
        let Some(rect) = parse_viewport_spec(spec) else {
            return Err(std::io::Error::other(format!(
                "invalid {VIEWPORTS_ENV} entry `{spec}`: \
                 expected WIDTHxHEIGHT with non-zero width and height"
            ))
            .into());
        };
        viewports.push(rect);
    }
    Ok(viewports)
}

/// Parse one `WIDTHxHEIGHT` spec.
fn parse_viewport_spec(raw: &str) -> Option<Rect> {
    let (w, h) = raw.split_once(['x', 'X'])?;
    let w: u16 = w.trim().parse().ok()?;
    let h: u16 = h.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some(Rect { x: 0, y: 0, w, h })
}

/// Path written for one viewport of a snapshot request.
fn snapshot_output_path(path: &Path, viewport: Rect, suffix: bool) -> PathBuf {
    if !suffix {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "snapshot".to_owned());
    let name = match path.extension() {
        Some(ext) => format!(
            "{stem}-{}x{}.{}",
            viewport.w,
            viewport.h,
            ext.to_string_lossy()
        ),
        None => format!("{stem}-{}x{}", viewport.w, viewport.h),
    };
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Parse a non-negative integer environment variable.
fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.trim().parse().ok()
}

/// Parse a millisecond duration environment variable.
fn env_duration_ms(key: &str) -> Option<Duration> {
    let ms: u64 = std::env::var(key).ok()?.trim().parse().ok()?;
    Some(Duration::from_millis(ms))
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
            viewports: vec![DEFAULT_VIEWPORT],
            suffix_viewports: false,
            frames: 1,
            focus_steps: 0,
            actions: Vec::new(),
            diagnostic: true,
            advance: Duration::ZERO,
            settle: Duration::ZERO,
        };
        assert_eq!(config.snapshot_options(), UiSnapshotOptions::diagnostic());
    }

    #[test]
    fn parse_viewports_accepts_a_comma_separated_list() {
        assert_eq!(
            parse_viewports("80x24,120x30,160x40").expect("valid list"),
            vec![
                Rect {
                    x: 0,
                    y: 0,
                    w: 80,
                    h: 24
                },
                Rect {
                    x: 0,
                    y: 0,
                    w: 120,
                    h: 30
                },
                Rect {
                    x: 0,
                    y: 0,
                    w: 160,
                    h: 40
                },
            ]
        );
        assert_eq!(
            parse_viewports("100X20").expect("case-insensitive x"),
            vec![Rect {
                x: 0,
                y: 0,
                w: 100,
                h: 20
            }]
        );
    }

    #[test]
    fn parse_viewports_rejects_malformed_and_zero_sizes() {
        let err = parse_viewports("80x24, nope, 120x30").expect_err("typo");
        let message = err.to_string();
        assert!(
            message.contains("nope") && message.contains(VIEWPORTS_ENV),
            "{message}"
        );

        let zero = parse_viewports("80x24,0x10").expect_err("zero width");
        assert!(zero.to_string().contains("0x10"), "{zero}");

        let empty = parse_viewports("80x24,").expect_err("trailing comma");
        assert!(empty.to_string().contains(VIEWPORTS_ENV), "{empty}");
    }

    #[test]
    fn snapshot_output_path_suffixes_only_when_requested() {
        let path = PathBuf::from("/tmp/app.png");
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        assert_eq!(
            snapshot_output_path(&path, viewport, false),
            PathBuf::from("/tmp/app.png")
        );
        assert_eq!(
            snapshot_output_path(&path, viewport, true),
            PathBuf::from("/tmp/app-80x24.png")
        );
        assert_eq!(
            snapshot_output_path(Path::new("out"), viewport, true),
            PathBuf::from("out-80x24")
        );
    }
}
