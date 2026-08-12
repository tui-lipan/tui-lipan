//! Visual baseline comparison for snapshot captures.
//!
//! A kept sketch only protects against regressions if something notices when the
//! picture changes. This module stores a baseline PNG per capture, compares the
//! next render against it pixel by pixel, and writes a highlighted diff image
//! when they disagree.
//!
//! # Determinism
//!
//! Comparison is only meaningful when the same UI produces the same pixels on
//! every machine. [`PngTextRenderer::Auto`] does not: it picks whichever system
//! font it can discover, so a capture on CI and a capture on a laptop differ in
//! every glyph. Baseline captures therefore force
//! [`PngTextRenderer::Bitmap`](crate::PngTextRenderer::Bitmap), whose built-in
//! font ships with the crate. Font-rendered artifacts remain available for
//! human review - they are simply not what gets compared.

use std::path::{Path, PathBuf};

use image::RgbImage;

use crate::Result;

/// Set to `1` to overwrite baselines with the current render instead of failing.
pub(crate) const UPDATE_ENV: &str = "TUI_LIPAN_UPDATE_BASELINES";

/// Colour painted over pixels that differ from the baseline.
const DIFF_HIGHLIGHT: [u8; 3] = [255, 0, 128];
/// Scale applied to unchanged pixels in the diff image, dimming the background.
const DIFF_DIM_NUMERATOR: u32 = 3;
/// Divisor paired with [`DIFF_DIM_NUMERATOR`].
const DIFF_DIM_DENOMINATOR: u32 = 10;

/// How a capture compared against its stored baseline.
#[derive(Clone, Debug, PartialEq)]
pub enum BaselineOutcome {
    /// No baseline existed; the current render was written as the new baseline.
    Created,
    /// The capture matched within the configured tolerance.
    Match {
        /// Fraction of pixels that differ, `0.0` when byte-identical.
        ratio: f64,
    },
    /// The capture differs beyond tolerance.
    Changed {
        /// Fraction of pixels that differ.
        ratio: f64,
        /// Number of differing pixels.
        changed_pixels: u64,
        /// Total pixels compared.
        total_pixels: u64,
        /// Highlighted diff image written next to the baseline.
        diff_path: PathBuf,
    },
    /// Image dimensions changed, so pixels cannot be compared.
    SizeChanged {
        /// Baseline dimensions.
        baseline: (u32, u32),
        /// Current dimensions.
        current: (u32, u32),
    },
    /// The baseline was overwritten because update mode is on.
    Updated,
}

impl BaselineOutcome {
    /// Returns `true` when this outcome should fail a regression check.
    ///
    /// [`Self::Created`] is not a failure: recording a new baseline is the normal
    /// first run for a new sketch.
    pub fn is_regression(&self) -> bool {
        matches!(self, Self::Changed { .. } | Self::SizeChanged { .. })
    }

    /// A one-line human-readable summary.
    pub fn summary(&self, name: &str) -> String {
        match self {
            Self::Created => format!("{name}: baseline created"),
            Self::Match { ratio } if *ratio == 0.0 => format!("{name}: identical"),
            Self::Match { ratio } => {
                format!(
                    "{name}: within tolerance ({:.4}% pixels differ)",
                    ratio * 100.0
                )
            }
            Self::Changed {
                ratio,
                changed_pixels,
                total_pixels,
                diff_path,
            } => format!(
                "{name}: CHANGED - {changed_pixels}/{total_pixels} pixels ({:.4}%) differ; diff at {}",
                ratio * 100.0,
                diff_path.display()
            ),
            Self::SizeChanged { baseline, current } => format!(
                "{name}: CHANGED - size {}x{} -> {}x{}",
                baseline.0, baseline.1, current.0, current.1
            ),
            Self::Updated => format!("{name}: baseline updated"),
        }
    }
}

/// One capture's comparison result.
#[derive(Clone, Debug)]
pub struct BaselineComparison {
    /// Capture name, e.g. `login-80x24`.
    pub name: String,
    /// Path of the stored baseline image.
    pub baseline_path: PathBuf,
    /// What the comparison found.
    pub outcome: BaselineOutcome,
}

/// Compare a single [`UiSnapshot`](super::UiSnapshot) against a stored baseline image.
///
/// This is the [`TestBackend`](crate::TestBackend) / [`UiSnapshot`](super::UiSnapshot)
/// counterpart of [`Sketch::baseline`](super::Sketch::baseline): first run records, later
/// runs compare, and `TUI_LIPAN_UPDATE_BASELINES=1` accepts the current render.
pub struct SnapshotBaseline {
    snapshot: super::UiSnapshot,
    dir: PathBuf,
    name: Option<String>,
    tolerance: f64,
}

impl SnapshotBaseline {
    pub(crate) fn new(snapshot: super::UiSnapshot, dir: impl Into<PathBuf>) -> Self {
        Self {
            snapshot,
            dir: dir.into(),
            name: None,
            tolerance: 0.0,
        }
    }

    /// Filename stem for the baseline image, without `.png`.
    ///
    /// Defaults to `snapshot-{width}x{height}` from the capture viewport.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Maximum fraction of differing pixels still counted as a match.
    ///
    /// Defaults to `0.0`, demanding an exact image.
    #[must_use]
    pub fn tolerance(mut self, ratio: f64) -> Self {
        self.tolerance = ratio.clamp(0.0, 1.0);
        self
    }

    fn capture_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            format!(
                "snapshot-{}x{}",
                self.snapshot.viewport.w, self.snapshot.viewport.h
            )
        })
    }

    /// Compare against the stored baseline and return the outcome.
    pub fn check(self) -> Result<BaselineComparison> {
        let name = self.capture_name();
        let baseline_path = self.dir.join(format!("{name}.png"));
        let deterministic = crate::capture::PngOptions {
            text_renderer: crate::capture::PngTextRenderer::Bitmap,
            ..crate::capture::PngOptions::default()
        };
        let current = self.snapshot.to_png(&deterministic)?;
        compare_or_create(&name, &baseline_path, &current, self.tolerance)
    }

    /// Compare and fail if the capture regressed against its baseline.
    pub fn assert_baseline(self) -> Result<()> {
        let comparison = self.check()?;
        if !comparison.outcome.is_regression() {
            return Ok(());
        }
        Err(std::io::Error::other(format!(
            "1 visual baseline regression(s):\n  {}\n\nRe-run with `{}=1` to accept them.",
            comparison.outcome.summary(&comparison.name),
            UPDATE_ENV,
        ))
        .into())
    }
}

/// Returns `true` when baselines should be rewritten rather than compared.
pub(crate) fn update_mode() -> bool {
    std::env::var(UPDATE_ENV).as_deref() == Ok("1")
}

/// Compare `current_png` against the baseline at `baseline_path`.
///
/// Writes the baseline when it does not yet exist, or when update mode is on.
/// `tolerance` is the maximum fraction of differing pixels still counted as a
/// match, so `0.0` demands an exact image.
pub(crate) fn compare_or_create(
    name: &str,
    baseline_path: &Path,
    current_png: &[u8],
    tolerance: f64,
) -> Result<BaselineComparison> {
    let outcome = compare_outcome(baseline_path, current_png, tolerance)?;
    Ok(BaselineComparison {
        name: name.to_owned(),
        baseline_path: baseline_path.to_path_buf(),
        outcome,
    })
}

/// Resolve the comparison outcome, writing baseline or diff files as needed.
fn compare_outcome(
    baseline_path: &Path,
    current_png: &[u8],
    tolerance: f64,
) -> Result<BaselineOutcome> {
    if let Some(parent) = baseline_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if !baseline_path.exists() {
        std::fs::write(baseline_path, current_png)?;
        return Ok(BaselineOutcome::Created);
    }
    if update_mode() {
        std::fs::write(baseline_path, current_png)?;
        return Ok(BaselineOutcome::Updated);
    }

    let baseline_bytes = std::fs::read(baseline_path)?;
    // Identical bytes are the common case; skip decoding entirely.
    if baseline_bytes == current_png {
        return Ok(BaselineOutcome::Match { ratio: 0.0 });
    }

    let baseline = decode(&baseline_bytes, "baseline")?;
    let current = decode(current_png, "current capture")?;

    if baseline.dimensions() != current.dimensions() {
        return Ok(BaselineOutcome::SizeChanged {
            baseline: baseline.dimensions(),
            current: current.dimensions(),
        });
    }

    let (changed_pixels, total_pixels) = count_differences(&baseline, &current);
    let ratio = if total_pixels == 0 {
        0.0
    } else {
        changed_pixels as f64 / total_pixels as f64
    };

    if ratio <= tolerance {
        return Ok(BaselineOutcome::Match { ratio });
    }

    let diff_path = diff_path_for(baseline_path);
    let diff = render_diff(&baseline, &current);
    diff.save(&diff_path)
        .map_err(|err| std::io::Error::other(err.to_string()))?;

    Ok(BaselineOutcome::Changed {
        ratio,
        changed_pixels,
        total_pixels,
        diff_path,
    })
}

/// Decode PNG bytes into an RGB image.
fn decode(bytes: &[u8], what: &str) -> Result<RgbImage> {
    image::load_from_memory(bytes)
        .map(|image| image.to_rgb8())
        .map_err(|err| std::io::Error::other(format!("failed to decode {what} PNG: {err}")).into())
}

/// Count pixels that differ between two same-sized images.
fn count_differences(baseline: &RgbImage, current: &RgbImage) -> (u64, u64) {
    let total = u64::from(baseline.width()) * u64::from(baseline.height());
    let changed = baseline
        .pixels()
        .zip(current.pixels())
        .filter(|(a, b)| a != b)
        .count() as u64;
    (changed, total)
}

/// Build a diff image: the current render dimmed, with changed pixels highlighted.
///
/// Dimming the unchanged background keeps the surrounding UI readable as context
/// while making the changed region unmissable.
fn render_diff(baseline: &RgbImage, current: &RgbImage) -> RgbImage {
    let mut out = current.clone();
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let same = baseline.get_pixel(x, y) == current.get_pixel(x, y);
        if same {
            for channel in pixel.0.iter_mut() {
                *channel =
                    ((u32::from(*channel) * DIFF_DIM_NUMERATOR) / DIFF_DIM_DENOMINATOR) as u8;
            }
        } else {
            pixel.0 = DIFF_HIGHLIGHT;
        }
    }
    out
}

/// Path for the diff image belonging to `baseline_path`.
fn diff_path_for(baseline_path: &Path) -> PathBuf {
    let stem = baseline_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "baseline".to_owned());
    baseline_path.with_file_name(format!("{stem}.diff.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(width, height, image::Rgb(color))
    }

    fn encode(image: &RgbImage) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image.clone())
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("encode");
        out.into_inner()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-baseline-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn missing_baseline_is_created_and_is_not_a_regression() {
        let dir = temp_dir("create");
        let path = dir.join("shot.png");
        let png = encode(&solid(4, 4, [10, 20, 30]));

        let result = compare_or_create("shot", &path, &png, 0.0).expect("compare");

        assert_eq!(result.outcome, BaselineOutcome::Created);
        assert!(!result.outcome.is_regression());
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identical_capture_matches_exactly() {
        let dir = temp_dir("match");
        let path = dir.join("shot.png");
        let png = encode(&solid(4, 4, [10, 20, 30]));
        std::fs::write(&path, &png).expect("seed baseline");

        let result = compare_or_create("shot", &path, &png, 0.0).expect("compare");

        assert_eq!(result.outcome, BaselineOutcome::Match { ratio: 0.0 });
        assert!(!result.outcome.is_regression());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn changed_capture_reports_ratio_and_writes_a_diff_image() {
        let dir = temp_dir("changed");
        let path = dir.join("shot.png");
        std::fs::write(&path, encode(&solid(4, 4, [0, 0, 0]))).expect("seed baseline");

        // One differing pixel out of sixteen.
        let mut changed = solid(4, 4, [0, 0, 0]);
        changed.put_pixel(1, 1, image::Rgb([255, 255, 255]));

        let result = compare_or_create("shot", &path, &encode(&changed), 0.0).expect("compare");

        match result.outcome {
            BaselineOutcome::Changed {
                changed_pixels,
                total_pixels,
                ref diff_path,
                ratio,
            } => {
                assert_eq!(changed_pixels, 1);
                assert_eq!(total_pixels, 16);
                assert!((ratio - 0.0625).abs() < f64::EPSILON);
                assert!(diff_path.exists(), "diff image should be written");

                let diff = image::open(diff_path).expect("diff decodes").to_rgb8();
                assert_eq!(diff.get_pixel(1, 1).0, DIFF_HIGHLIGHT);
            }
            other => panic!("expected Changed, got {other:?}"),
        }
        assert!(result.outcome.is_regression());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tolerance_absorbs_a_small_difference() {
        let dir = temp_dir("tolerance");
        let path = dir.join("shot.png");
        std::fs::write(&path, encode(&solid(4, 4, [0, 0, 0]))).expect("seed baseline");

        let mut changed = solid(4, 4, [0, 0, 0]);
        changed.put_pixel(1, 1, image::Rgb([255, 255, 255]));

        // 1/16 = 0.0625, just inside a 0.1 tolerance.
        let result = compare_or_create("shot", &path, &encode(&changed), 0.1).expect("compare");

        assert!(matches!(result.outcome, BaselineOutcome::Match { .. }));
        assert!(!result.outcome.is_regression());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resized_capture_reports_size_change_without_pixel_math() {
        let dir = temp_dir("resize");
        let path = dir.join("shot.png");
        std::fs::write(&path, encode(&solid(4, 4, [0, 0, 0]))).expect("seed baseline");

        let result = compare_or_create("shot", &path, &encode(&solid(8, 4, [0, 0, 0])), 1.0)
            .expect("compare");

        assert_eq!(
            result.outcome,
            BaselineOutcome::SizeChanged {
                baseline: (4, 4),
                current: (8, 4),
            }
        );
        assert!(result.outcome.is_regression());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn diff_path_sits_beside_the_baseline() {
        assert_eq!(
            diff_path_for(Path::new("/tmp/base/login-80x24.png")),
            PathBuf::from("/tmp/base/login-80x24.diff.png")
        );
    }
}
