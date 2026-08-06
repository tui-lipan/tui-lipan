//! One-call sketch capture for design iteration.
//!
//! [`Sketch`] collapses the render-capture-write cycle into a single builder so a
//! kept sketch file stays small enough to be worth keeping. Without it, every
//! design iteration needs a throwaway `main()` that mounts a [`TestBackend`], sets
//! a viewport, renders, captures, and writes each artifact by hand.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::core::component::Component;
use crate::core::element::Element;
use crate::mockup::Mockup;
use crate::style::Rect;
use crate::test_backend::TestBackend;

#[cfg(feature = "ui-snapshot-png")]
use super::baseline::{self, BaselineComparison};
use super::options::UiSnapshotOptions;

/// Default viewport used when a sketch declares no viewport of its own.
const DEFAULT_VIEWPORT: (u16, u16) = (80, 24);
/// Default fit-to-content margin, wide enough to expose flex distribution.
const DEFAULT_FIT_MARGIN: (u16, u16) = (20, 8);

/// A single capture pass requested by a [`Sketch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SketchViewport {
    /// Lay out at exactly these dimensions.
    Fixed { w: u16, h: u16 },
    /// Lay out at content minimum size plus a margin.
    Fit { margin_w: u16, margin_h: u16 },
}

impl SketchViewport {
    /// Returns the filename suffix identifying this capture pass.
    fn slug(&self) -> String {
        match self {
            Self::Fixed { w, h } => format!("{w}x{h}"),
            Self::Fit { .. } => "fit".to_owned(),
        }
    }
}

/// Renders a view at one or more viewports and writes snapshot artifacts to disk.
///
/// Artifacts land in `target/ui-sketches/` by default, so a kept sketch never
/// needs a `.gitignore` entry and `cargo clean` removes the output.
///
/// # Example
///
/// ```rust,no_run
/// use tui_lipan::Sketch;
/// use tui_lipan::prelude::*;
///
/// fn login_screen() -> Element {
///     Frame::new()
///         .header_left("Sign In")
///         .child(Text::new("Welcome back."))
///         .into()
/// }
///
/// fn main() -> Result<()> {
///     Sketch::view("login", login_screen).write()?;
///     Ok(())
/// }
/// ```
///
/// With no explicit viewport this captures `80x24` plus a fit-to-content pass, the
/// pairing that exposes flex-distribution bugs a single viewport hides.
pub struct Sketch<C: Component> {
    name: String,
    component: C,
    viewports: Vec<SketchViewport>,
    dir: Option<PathBuf>,
    markdown: bool,
    png: bool,
    json: bool,
    quiet: bool,
    focus_steps: usize,
    options: UiSnapshotOptions,
    key_script: Option<String>,
    #[cfg(feature = "ui-snapshot-png")]
    baseline_dir: Option<PathBuf>,
    #[cfg(feature = "ui-snapshot-png")]
    tolerance: f64,
}

impl<F> Sketch<Mockup<F>>
where
    F: Fn() -> Element + 'static,
{
    /// Sketch a plain view function, with no `Component` boilerplate.
    ///
    /// The closure is mounted through [`Mockup`], so it needs no `State`,
    /// `Message`, or `update()`.
    pub fn view(name: impl Into<String>, view: F) -> Self {
        Self::component(name, Mockup::new(view))
    }
}

impl<C: Component> Sketch<C>
where
    C::Properties: Default,
{
    /// Sketch a mounted [`Component`], using its default properties.
    pub fn component(name: impl Into<String>, component: C) -> Self {
        Self {
            name: name.into(),
            component,
            viewports: Vec::new(),
            dir: None,
            markdown: true,
            png: true,
            json: false,
            quiet: false,
            focus_steps: 0,
            options: UiSnapshotOptions::default(),
            key_script: None,
            #[cfg(feature = "ui-snapshot-png")]
            baseline_dir: None,
            #[cfg(feature = "ui-snapshot-png")]
            tolerance: 0.0,
        }
    }

    /// Capture at an exact viewport size.
    ///
    /// Repeat to capture several breakpoints; each one writes its own artifacts.
    /// Declaring any viewport replaces the default `80x24` plus fit-to-content pair.
    #[must_use]
    pub fn viewport(mut self, w: u16, h: u16) -> Self {
        self.viewports.push(SketchViewport::Fixed { w, h });
        self
    }

    /// Capture at content minimum size plus `(margin_w, margin_h)`.
    ///
    /// The margin is what reveals accidental flex distribution: buttons drifting
    /// away from their form, a sidebar growing past its intended width.
    #[must_use]
    pub fn fit(mut self, margin_w: u16, margin_h: u16) -> Self {
        self.viewports
            .push(SketchViewport::Fit { margin_w, margin_h });
        self
    }

    /// Write artifacts to `dir` instead of `target/ui-sketches/`.
    #[must_use]
    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Advance focus `steps` times before capturing, giving the sketch a visible
    /// focus state instead of an idle tree.
    #[must_use]
    pub fn focus_next(mut self, steps: usize) -> Self {
        self.focus_steps = steps;
        self
    }

    /// Override the semantic describe options.
    ///
    /// Pass [`UiSnapshotOptions::diagnostic`] when content vanishes; the markdown
    /// then flags `zero-area` widgets.
    #[must_use]
    pub fn options(mut self, options: UiSnapshotOptions) -> Self {
        self.options = options;
        self
    }

    /// Toggle the markdown artifact. Enabled by default.
    #[must_use]
    pub fn markdown(mut self, enabled: bool) -> Self {
        self.markdown = enabled;
        self
    }

    /// Toggle the PNG artifact. Enabled by default, and skipped with a hint when
    /// the `ui-snapshot-png` feature is off.
    #[must_use]
    pub fn png(mut self, enabled: bool) -> Self {
        self.png = enabled;
        self
    }

    /// Toggle the JSON artifact. Disabled by default; needs `ui-snapshot-json`.
    #[must_use]
    pub fn json(mut self, enabled: bool) -> Self {
        self.json = enabled;
        self
    }

    /// Stop printing written paths to stdout.
    ///
    /// Paths are printed by default because a sketch is normally run through
    /// `cargo run` and the artifact is only useful once you can find it.
    #[must_use]
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Dispatch a key script before capturing, e.g. `"tab,tab,enter"`.
    ///
    /// Entries use ordinary keybinding syntax (`ctrl+n`, `esc`, `f12`) and are
    /// sent in order after the first render, so states behind a keystroke - an
    /// open modal, a submitted form, an error - can be captured without wiring a
    /// harness.
    ///
    /// An unparseable script is reported by [`Self::write`] rather than skipped,
    /// since a dropped keystroke silently captures the wrong state.
    #[must_use]
    pub fn keys(mut self, script: impl AsRef<str>) -> Self {
        self.key_script = Some(script.as_ref().to_owned());
        self
    }

    /// Compare each capture against a stored baseline image in `dir`.
    ///
    /// The first run records baselines; later runs compare against them and write
    /// a highlighted `*.diff.png` beside any baseline that changed.
    ///
    /// Baseline captures force [`PngTextRenderer::Bitmap`](crate::PngTextRenderer::Bitmap):
    /// the default font discovery picks whatever the host has installed, which
    /// makes pixel comparison meaningless across machines.
    ///
    /// Set `TUI_LIPAN_UPDATE_BASELINES=1` to accept the current render as the new
    /// baseline.
    #[cfg(feature = "ui-snapshot-png")]
    #[must_use]
    pub fn baseline(mut self, dir: impl Into<PathBuf>) -> Self {
        self.baseline_dir = Some(dir.into());
        self
    }

    /// Maximum fraction of differing pixels still counted as a match.
    ///
    /// Defaults to `0.0`, demanding an exact image. Raise it only when a capture
    /// has a genuinely nondeterministic region; prefer removing the nondeterminism.
    #[cfg(feature = "ui-snapshot-png")]
    #[must_use]
    pub fn tolerance(mut self, ratio: f64) -> Self {
        self.tolerance = ratio.clamp(0.0, 1.0);
        self
    }

    /// Render every requested viewport and write the artifacts.
    ///
    /// Returns the written paths in capture order. When a baseline directory is
    /// configured, use [`Self::check`] instead to receive the comparison results.
    pub fn write(self) -> Result<Vec<PathBuf>> {
        Ok(self.run()?.written)
    }

    /// Render, write artifacts, and compare each capture against its baseline.
    ///
    /// Requires [`Self::baseline`]; without it the returned comparison list is
    /// empty. Callers decide what a regression means - see
    /// [`BaselineOutcome::is_regression`](super::BaselineOutcome::is_regression).
    #[cfg(feature = "ui-snapshot-png")]
    pub fn check(self) -> Result<Vec<BaselineComparison>> {
        Ok(self.run()?.comparisons)
    }

    /// Render, write artifacts, and fail if any capture regressed against its baseline.
    ///
    /// The intended shape for a regression test. Reports every changed capture at
    /// once rather than stopping at the first, so one run tells you the full
    /// blast radius.
    #[cfg(feature = "ui-snapshot-png")]
    pub fn assert_baseline(self) -> Result<()> {
        let comparisons = self.check()?;
        let regressions: Vec<String> = comparisons
            .iter()
            .filter(|comparison| comparison.outcome.is_regression())
            .map(|comparison| comparison.outcome.summary(&comparison.name))
            .collect();

        if regressions.is_empty() {
            return Ok(());
        }
        Err(std::io::Error::other(format!(
            "{} visual baseline regression(s):\n  {}\n\nRe-run with `{}=1` to accept them.",
            regressions.len(),
            regressions.join("\n  "),
            baseline::UPDATE_ENV,
        ))
        .into())
    }

    /// Shared implementation behind `write`, `check`, and `assert_baseline`.
    fn run(self) -> Result<SketchRun> {
        let Self {
            name,
            component,
            viewports,
            dir,
            markdown,
            png,
            json,
            quiet,
            focus_steps,
            options,
            key_script,
            #[cfg(feature = "ui-snapshot-png")]
            baseline_dir,
            #[cfg(feature = "ui-snapshot-png")]
            tolerance,
        } = self;
        let formats = SketchFormats {
            markdown,
            png,
            json,
        };
        let keys = match key_script.as_deref() {
            Some(script) => super::keys::parse_key_script(script)?,
            None => Vec::new(),
        };

        let dir = dir.unwrap_or_else(default_sketch_dir);
        std::fs::create_dir_all(&dir)?;

        let viewports = if viewports.is_empty() {
            vec![
                SketchViewport::Fixed {
                    w: DEFAULT_VIEWPORT.0,
                    h: DEFAULT_VIEWPORT.1,
                },
                SketchViewport::Fit {
                    margin_w: DEFAULT_FIT_MARGIN.0,
                    margin_h: DEFAULT_FIT_MARGIN.1,
                },
            ]
        } else {
            viewports
        };

        let stem = slugify(&name);
        let mut backend = TestBackend::new(component);
        let mut run = SketchRun::default();

        for viewport in viewports {
            // A fit capture measures content minimum size, which needs a laid-out
            // tree first; the fixed pass below primes it either way.
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: DEFAULT_VIEWPORT.0,
                h: DEFAULT_VIEWPORT.1,
            });
            if let SketchViewport::Fixed { w, h } = viewport {
                backend.set_viewport(Rect {
                    x: 0,
                    y: 0,
                    w: w.max(1),
                    h: h.max(1),
                });
            }
            backend.render();

            for _ in 0..focus_steps {
                backend.focus_next();
            }
            if focus_steps > 0 {
                backend.render();
            }

            // Keys run after layout exists, so handlers see real rects and a
            // resolved focus target.
            for key in &keys {
                backend.send_key(*key)?;
            }

            let snapshot = match viewport {
                SketchViewport::Fixed { .. } => backend.capture_ui_snapshot_with_options(&options),
                SketchViewport::Fit { margin_w, margin_h } => {
                    backend.capture_ui_snapshot_with_margin(margin_w, margin_h, &options)
                }
            };

            let base = format!("{stem}-{}", viewport.slug());
            run.written
                .extend(write_sketch_artifacts(&formats, &dir, &base, &snapshot)?);

            #[cfg(feature = "ui-snapshot-png")]
            if let Some(baseline_dir) = baseline_dir.as_ref() {
                let baseline_path = baseline_dir.join(format!("{base}.png"));
                // Bitmap rendering only: font discovery differs per machine, and a
                // baseline that fails on someone else's font stack is worthless.
                let deterministic = crate::capture::PngOptions {
                    text_renderer: crate::capture::PngTextRenderer::Bitmap,
                    ..crate::capture::PngOptions::default()
                };
                let current = snapshot.to_png(&deterministic)?;
                run.comparisons.push(baseline::compare_or_create(
                    &base,
                    &baseline_path,
                    &current,
                    tolerance,
                )?);
            }
        }

        if !quiet {
            for path in &run.written {
                println!("wrote {}", path.display());
            }
            #[cfg(feature = "ui-snapshot-png")]
            for comparison in &run.comparisons {
                println!("{}", comparison.outcome.summary(&comparison.name));
            }
            #[cfg(not(feature = "ui-snapshot-png"))]
            if formats.png {
                println!(
                    "note: PNG skipped - rerun with `--features ui-snapshot-png` (or `cargo snap`)"
                );
            }
        }

        Ok(run)
    }
}

/// Everything one [`Sketch`] run produced.
#[derive(Default)]
struct SketchRun {
    /// Artifact paths, in capture order.
    written: Vec<PathBuf>,
    /// Baseline comparisons, empty unless a baseline directory was configured.
    #[cfg(feature = "ui-snapshot-png")]
    comparisons: Vec<BaselineComparison>,
}

/// Which artifacts a capture pass should write.
#[derive(Clone, Copy, Debug)]
struct SketchFormats {
    markdown: bool,
    png: bool,
    #[allow(dead_code)] // gated by `ui-snapshot-json`
    json: bool,
}

/// Write every enabled format for one captured viewport.
fn write_sketch_artifacts(
    formats: &SketchFormats,
    dir: &Path,
    base: &str,
    snapshot: &super::UiSnapshot,
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();

    if formats.markdown {
        let path = dir.join(format!("{base}.md"));
        std::fs::write(&path, snapshot.to_markdown())?;
        written.push(path);
    }

    #[cfg(feature = "ui-snapshot-json")]
    if formats.json {
        let path = dir.join(format!("{base}.json"));
        std::fs::write(&path, snapshot.to_json_pretty())?;
        written.push(path);
    }

    #[cfg(feature = "ui-snapshot-png")]
    if formats.png {
        let path = dir.join(format!("{base}.png"));
        std::fs::write(&path, snapshot.to_png_default()?)?;
        written.push(path);
    }

    Ok(written)
}

/// Returns the directory sketches are written to when none is configured.
///
/// Prefers `TUI_LIPAN_SKETCH_DIR`, then the cargo target directory, so artifacts
/// stay out of version control without a `.gitignore` entry.
fn default_sketch_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TUI_LIPAN_SKETCH_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("CARGO_MANIFEST_DIR").map(|dir| PathBuf::from(dir).join("target"))
        })
        .unwrap_or_else(|| PathBuf::from("target"));
    base.join("ui-sketches")
}

/// Converts a sketch name into a filename-safe stem.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        out.push_str("sketch");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_replaces_runs_of_separators_with_single_dash() {
        assert_eq!(slugify("Login Screen"), "login-screen");
        assert_eq!(slugify("  weird // name  "), "weird-name");
        assert_eq!(slugify("!!!"), "sketch");
    }

    #[test]
    fn viewport_slug_distinguishes_fixed_from_fit() {
        assert_eq!(SketchViewport::Fixed { w: 80, h: 24 }.slug(), "80x24");
        assert_eq!(
            SketchViewport::Fit {
                margin_w: 20,
                margin_h: 8
            }
            .slug(),
            "fit"
        );
    }
}
