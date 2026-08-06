//! Scripted terminal recordings.
//!
//! [`Recording`] drives a component through a key script on a fixed time step,
//! capturing a frame per step into an asciinema cast. It is the moving-picture
//! counterpart to [`Sketch`](super::Sketch): same shape, same no-throwaway-code
//! rule, but it produces a demo instead of a still.
//!
//! # Why a fixed step
//!
//! A TUI renders on demand, and a headless run has no wall clock to sample. The
//! recorder therefore advances a synthetic clock in `1/fps` increments and ticks
//! animations by the same amount, so a run is deterministic: the same view and
//! the same script always produce the same cast bytes, which makes a committed
//! recording diffable.
//!
//! The cost is that work depending on real elapsed time - a PTY child's output,
//! a network response - does not arrive on a synthetic clock. Recordings capture
//! an app's own rendering, not a live session.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::Result;
use crate::capture::CastRecording;
use crate::core::component::Component;
use crate::core::element::Element;
use crate::mockup::Mockup;
use crate::style::Rect;
use crate::test_backend::TestBackend;

/// Default capture rate.
const DEFAULT_FPS: u16 = 30;
/// Default viewport, matching the headless snapshot default.
const DEFAULT_VIEWPORT: (u16, u16) = (100, 30);
/// Default pause after each scripted key, so a viewer can follow the action.
const DEFAULT_KEY_DELAY: Duration = Duration::from_millis(400);
/// Default hold on the final frame before the recording ends.
const DEFAULT_SETTLE: Duration = Duration::from_millis(1200);
/// Upper bound on a single animation tick, matching the runner's per-frame clamp.
const MAX_TICK: Duration = Duration::from_millis(50);

/// Records a component driven by a key script as an asciinema cast.
///
/// # Example
///
/// ```rust,no_run
/// use tui_lipan::Recording;
/// use tui_lipan::prelude::*;
///
/// fn view() -> Element {
///     Frame::new().child(Text::new("hello")).into()
/// }
///
/// fn main() -> Result<()> {
///     Recording::view("demo", view)
///         .viewport(100, 30)
///         .keys("tab,enter")
///         .write("demo.cast")?;
///     Ok(())
/// }
/// ```
pub struct Recording<C: Component> {
    title: String,
    component: C,
    viewport: (u16, u16),
    fps: u16,
    key_script: Option<String>,
    action_script: Option<String>,
    key_delay: Duration,
    settle: Duration,
    quiet: bool,
    #[cfg(feature = "ui-snapshot-png")]
    png_options: Option<crate::capture::PngOptions>,
}

impl<F> Recording<Mockup<F>>
where
    F: Fn() -> Element + 'static,
{
    /// Record a plain view function, with no `Component` boilerplate.
    pub fn view(title: impl Into<String>, view: F) -> Self {
        Self::component(title, Mockup::new(view))
    }
}

impl<C: Component> Recording<C>
where
    C::Properties: Default,
{
    /// Record a mounted [`Component`], using its default properties.
    pub fn component(title: impl Into<String>, component: C) -> Self {
        Self {
            title: title.into(),
            component,
            viewport: DEFAULT_VIEWPORT,
            fps: DEFAULT_FPS,
            key_script: None,
            action_script: None,
            key_delay: DEFAULT_KEY_DELAY,
            settle: DEFAULT_SETTLE,
            quiet: false,
            #[cfg(feature = "ui-snapshot-png")]
            png_options: None,
        }
    }

    /// Set the recorded terminal size.
    #[must_use]
    pub fn viewport(mut self, w: u16, h: u16) -> Self {
        self.viewport = (w.max(1), h.max(1));
        self
    }

    /// Set the capture rate. Clamped to at least 1.
    ///
    /// Higher rates make animations smoother at the cost of file size; identical
    /// frames are dropped, so a mostly-static app costs little at any rate.
    #[must_use]
    pub fn fps(mut self, fps: u16) -> Self {
        self.fps = fps.max(1);
        self
    }

    /// Keys to play, in ordinary keybinding syntax, e.g. `"tab,tab,enter"`.
    ///
    /// Shorthand for a keys-only [`Self::script`]; use that when the recording
    /// also needs to click, hover, focus, scroll, drag, or wait.
    #[must_use]
    pub fn keys(mut self, script: impl AsRef<str>) -> Self {
        self.key_script = Some(script.as_ref().to_owned());
        self
    }

    /// Actions to play, e.g. `"click:#open; wait:300; key:esc"`.
    ///
    /// Widgets are targeted by reconciliation key. Takes precedence over
    /// [`Self::keys`]. See [`Action`](crate::Action) for the full syntax.
    #[must_use]
    pub fn script(mut self, script: impl AsRef<str>) -> Self {
        self.action_script = Some(script.as_ref().to_owned());
        self
    }

    /// Pause held after each key, letting animations and effects play out.
    #[must_use]
    pub fn key_delay(mut self, delay: Duration) -> Self {
        self.key_delay = delay;
        self
    }

    /// Time held on the final frame before the recording ends.
    #[must_use]
    pub fn settle(mut self, settle: Duration) -> Self {
        self.settle = settle;
        self
    }

    /// Stop printing the written path and duration.
    #[must_use]
    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Rendering options for [`Self::write_frames`].
    ///
    /// Defaults to [`PngOptions::default`](crate::PngOptions); raise `scale` for
    /// a higher-resolution video, or force a font for consistent glyphs.
    #[cfg(feature = "ui-snapshot-png")]
    #[must_use]
    pub fn png_options(mut self, options: crate::capture::PngOptions) -> Self {
        self.png_options = Some(options);
        self
    }

    /// Drive the timeline, handing every captured frame to `sink`.
    ///
    /// One driver serves both outputs so a cast and a frame sequence recorded
    /// from the same script stay in step. The sink sees *every* tick, including
    /// unchanged ones; dropping duplicates is the cast's business, while video
    /// needs a frame per tick to hold a constant rate.
    fn play(
        self,
        mut sink: impl FnMut(f64, &crate::capture::CapturedFrame) -> Result<()>,
    ) -> Result<()> {
        let Self {
            title: _,
            component,
            viewport,
            fps,
            key_script,
            action_script,
            key_delay,
            settle,
            quiet: _,
            #[cfg(feature = "ui-snapshot-png")]
                png_options: _,
        } = self;

        let actions = resolve_actions(action_script.as_deref(), key_script.as_deref())?;

        let (w, h) = viewport;
        let mut backend = TestBackend::new(component);
        backend.set_viewport(Rect { x: 0, y: 0, w, h });
        backend.render();

        let step = Duration::from_secs_f64(1.0 / f64::from(fps));
        let mut clock = Duration::ZERO;

        sink(clock.as_secs_f64(), &backend.capture_frame())?;

        for action in &actions {
            // A wait is timeline, not input: it spends its own duration rather
            // than taking a step and then the usual pause.
            if let super::Action::Wait(dt) = action {
                hold(&mut backend, &mut sink, &mut clock, *dt, step)?;
                continue;
            }

            super::execute(&mut backend, action)?;
            clock += step;
            sink(clock.as_secs_f64(), &backend.capture_frame())?;
            hold(&mut backend, &mut sink, &mut clock, key_delay, step)?;
        }

        hold(&mut backend, &mut sink, &mut clock, settle, step)?;
        Ok(())
    }

    /// Play the script and return the recording without writing it.
    pub fn record(self) -> Result<CastRecording> {
        let (w, h) = self.viewport;
        let title = self.title.clone();
        let mut cast = CastRecording::new(w, h).title(title);

        let mut last_time = 0.0;
        self.play(|time, frame| {
            last_time = time;
            cast.push_frame(time, frame);
            Ok(())
        })?;

        // Identical frames were dropped, so without this the cast would end at
        // the last visible change and a player would cut the settle short.
        cast.mark_time(last_time);
        Ok(cast)
    }

    /// Play the script and write one PNG per frame into `dir`.
    ///
    /// Frames are written at a constant rate, one per `1/fps` tick including
    /// unchanged ones, which is what an encoder needs to reproduce the original
    /// timing from a numbered sequence. Unchanged frames reuse the previously
    /// encoded bytes rather than re-encoding.
    ///
    /// Returns the written paths in order. Unlike a cast, these are truecolor -
    /// use them when GIF's 256-color quantization would hurt.
    ///
    /// ```sh
    /// ffmpeg -framerate 30 -i frames/frame_%05d.png \
    ///        -pix_fmt yuv420p -movflags +faststart demo.mp4
    /// ```
    #[cfg(feature = "ui-snapshot-png")]
    pub fn write_frames(self, dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let quiet = self.quiet;
        let fps = self.fps;
        let options = self.png_options.clone().unwrap_or_default();

        let mut written: Vec<PathBuf> = Vec::new();
        let mut previous: Option<(crate::capture::CapturedFrame, Vec<u8>)> = None;

        self.play(|_, frame| {
            let bytes = match previous.as_ref() {
                // Re-encoding an unchanged frame costs real time at 30fps and
                // produces identical bytes, so reuse them.
                Some((last, bytes)) if last == frame => bytes.clone(),
                _ => frame.to_png(&options)?,
            };
            let path = dir.join(format!("frame_{:05}.png", written.len()));
            std::fs::write(&path, &bytes)?;
            previous = Some((frame.clone(), bytes));
            written.push(path);
            Ok(())
        })?;

        if !quiet {
            println!("wrote {} frames to {}", written.len(), dir.display());
            println!(
                "  ffmpeg -framerate {fps} -i {}/frame_%05d.png -pix_fmt yuv420p -movflags +faststart out.mp4",
                dir.display()
            );
        }
        Ok(written)
    }

    /// Play the script and write the cast to `path`.
    pub fn write(self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        let quiet = self.quiet;
        let cast = self.record()?;
        cast.write(&path)?;

        if !quiet {
            println!(
                "wrote {} ({} frames, {:.1}s)",
                path.display(),
                cast.len(),
                cast.duration_secs()
            );
        }
        Ok(path)
    }
}

/// Turn the configured scripts into actions.
///
/// An action script wins when both are set; `keys` remains as the shorthand for
/// the common typing-only case.
pub(crate) fn resolve_actions(
    action_script: Option<&str>,
    key_script: Option<&str>,
) -> Result<Vec<super::Action>> {
    if let Some(script) = action_script {
        return super::parse_script(script);
    }
    match key_script {
        Some(script) => Ok(super::keys::parse_key_script(script)?
            .into_iter()
            .map(super::Action::Key)
            .collect()),
        None => Ok(Vec::new()),
    }
}

/// Advance the synthetic clock by `total`, capturing a frame every `step`.
///
/// Animations are ticked in clamped increments so a long hold cannot skip a
/// transition, matching how the runner paces its own frames.
fn hold<C: Component>(
    backend: &mut TestBackend<C>,
    sink: &mut impl FnMut(f64, &crate::capture::CapturedFrame) -> Result<()>,
    clock: &mut Duration,
    total: Duration,
    step: Duration,
) -> Result<()> {
    let mut remaining = total;
    while !remaining.is_zero() {
        let frame_span = step.min(remaining);

        // Animations tick in clamped sub-steps so a long frame cannot skip a
        // transition, but exactly one frame is emitted per `step`. Tying the two
        // together would emit frames at the clamp rate instead of the requested
        // rate, and a numbered sequence would then play back at the wrong speed.
        let mut advanced = Duration::ZERO;
        while advanced < frame_span {
            let tick = (frame_span - advanced).min(MAX_TICK);
            backend.advance(tick);
            advanced += tick;
        }

        *clock += frame_span;
        sink(clock.as_secs_f64(), &backend.capture_frame())?;
        remaining = remaining.saturating_sub(frame_span);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::component::{Context, KeyUpdate, Update};
    use crate::core::element::IntoElement;
    use crate::core::event::{KeyCode, KeyEvent};
    use crate::widgets::Text;

    struct Echo;

    impl Component for Echo {
        type Message = ();
        type Properties = ();
        type State = String;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            String::new()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
            if let KeyCode::Char(ch) = key.code {
                ctx.state.push(ch);
                return KeyUpdate::handled(Update::full());
            }
            KeyUpdate::unhandled(Update::none())
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(format!("typed:{}", ctx.state)).into()
        }
    }

    fn static_view() -> Element {
        Text::new("static").into()
    }

    #[test]
    fn a_static_view_records_one_frame_plus_a_hold() {
        // Identical frames are dropped, so however long the recording runs a still
        // view costs one frame event plus the marker that holds it on screen.
        let cast = Recording::view("static", static_view)
            .viewport(20, 3)
            .fps(30)
            .settle(Duration::from_secs(2))
            .quiet(true)
            .record()
            .expect("records");

        assert_eq!(
            cast.len(),
            2,
            "expected one frame and one hold marker, not 60 repeat frames"
        );
        assert!(
            cast.duration_secs() >= 2.0,
            "the hold should run the full settle: {}",
            cast.duration_secs()
        );
    }

    #[test]
    fn each_key_produces_a_distinct_frame() {
        let cast = Recording::view("keys", static_view)
            .viewport(20, 3)
            .settle(Duration::ZERO)
            .quiet(true)
            .record()
            .expect("records");
        let baseline = cast.len();

        let typed = Recording::component("keys", Echo)
            .viewport(20, 3)
            .keys("a,b,c")
            .key_delay(Duration::ZERO)
            .settle(Duration::ZERO)
            .quiet(true)
            .record()
            .expect("records");

        assert_eq!(baseline, 1);
        assert_eq!(
            typed.len(),
            4,
            "initial frame plus one per keystroke that changed the view"
        );
    }

    #[test]
    fn the_clock_advances_with_key_delay_and_settle() {
        let cast = Recording::component("timed", Echo)
            .viewport(20, 3)
            .fps(10)
            .keys("a")
            .key_delay(Duration::from_millis(500))
            .settle(Duration::from_millis(500))
            .quiet(true)
            .record()
            .expect("records");

        // 1 step for the key, then 500ms of hold, then 500ms of settle.
        assert!(
            cast.duration_secs() >= 1.0,
            "expected at least 1s of timeline, got {}",
            cast.duration_secs()
        );
    }

    #[test]
    fn recording_is_deterministic_across_runs() {
        let run = || {
            Recording::component("determinism", Echo)
                .viewport(24, 4)
                .keys("h,i")
                .key_delay(Duration::from_millis(100))
                .settle(Duration::from_millis(100))
                .quiet(true)
                .record()
                .expect("records")
                .to_cast()
        };
        assert_eq!(run(), run(), "same script must produce identical bytes");
    }

    #[cfg(feature = "ui-snapshot-png")]
    #[test]
    fn frames_are_written_at_a_constant_rate() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-frames-rate-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        // 10fps for 1s of settle, plus the opening frame: an encoder needs one
        // frame per tick even though the view never changes.
        let frames = Recording::view("rate", static_view)
            .viewport(16, 2)
            .fps(10)
            .settle(Duration::from_millis(1000))
            .quiet(true)
            .write_frames(&dir)
            .expect("writes frames");

        assert_eq!(
            frames.len(),
            11,
            "expected 1 opening frame + 10 ticks, got {}",
            frames.len()
        );
        assert!(frames.iter().all(|path| path.exists()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "ui-snapshot-png")]
    #[test]
    fn frames_are_zero_padded_and_ordered_for_ffmpeg() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-frames-order-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        let frames = Recording::view("order", static_view)
            .viewport(16, 2)
            .fps(4)
            .settle(Duration::from_millis(500))
            .quiet(true)
            .write_frames(&dir)
            .expect("writes frames");

        let names: Vec<String> = frames
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names[0], "frame_00000.png");
        assert_eq!(names[1], "frame_00001.png");
        // Lexical order must match capture order, or `-i frame_%05d.png` scrambles.
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "ui-snapshot-png")]
    #[test]
    fn frames_are_real_pngs_and_changed_frames_differ() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-frames-content-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        let frames = Recording::component("content", Echo)
            .viewport(20, 2)
            .fps(4)
            .keys("a,b")
            .key_delay(Duration::ZERO)
            .settle(Duration::ZERO)
            .quiet(true)
            .write_frames(&dir)
            .expect("writes frames");

        assert_eq!(frames.len(), 3, "opening frame plus one per key");
        let bytes: Vec<Vec<u8>> = frames.iter().map(|p| std::fs::read(p).unwrap()).collect();
        for (i, b) in bytes.iter().enumerate() {
            assert!(
                b.starts_with(&[0x89, b'P', b'N', b'G']),
                "frame {i} is not a PNG"
            );
        }
        assert_ne!(bytes[0], bytes[1], "typing should change the frame");
        assert_ne!(bytes[1], bytes[2]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(feature = "ui-snapshot-png")]
    #[test]
    fn unchanged_frames_reuse_identical_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-frames-reuse-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));

        let frames = Recording::view("reuse", static_view)
            .viewport(16, 2)
            .fps(4)
            .settle(Duration::from_millis(750))
            .quiet(true)
            .write_frames(&dir)
            .expect("writes frames");

        let first = std::fs::read(&frames[0]).unwrap();
        for path in &frames[1..] {
            assert_eq!(
                std::fs::read(path).unwrap(),
                first,
                "a still view must produce byte-identical frames"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_click_action_reaches_the_widget_it_targets() {
        // Clicking by key must hit the button even though the script never
        // mentions a coordinate.
        let cast = Recording::component("click", Clicker)
            .viewport(30, 5)
            .script("click:#go")
            .key_delay(Duration::ZERO)
            .settle(Duration::ZERO)
            .quiet(true)
            .record()
            .expect("records");

        let text = cast.to_cast();
        assert!(
            text.contains("clicked"),
            "the click should reach the button"
        );
    }

    #[test]
    fn a_click_on_a_missing_key_fails_loudly() {
        let err = Recording::component("missing", Clicker)
            .viewport(30, 5)
            .script("click:#nope")
            .quiet(true)
            .record()
            .expect_err("a missing key must not silently click nothing");
        assert!(err.to_string().contains("nope"), "{err}");
    }

    #[test]
    fn wait_actions_spend_timeline_rather_than_input() {
        let cast = Recording::view("waiting", static_view)
            .viewport(20, 3)
            .fps(10)
            .script("wait:500")
            .settle(Duration::ZERO)
            .quiet(true)
            .record()
            .expect("records");
        assert!(
            cast.duration_secs() >= 0.5,
            "a wait should advance the clock: {}",
            cast.duration_secs()
        );
    }

    /// Button that records having been clicked, for click-targeting tests.
    struct Clicker;

    impl Component for Clicker {
        type Message = ();
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state = true;
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let label = if ctx.state { "clicked" } else { "idle" };
            crate::widgets::VStack::new()
                .child(Text::new(label))
                .child(
                    crate::widgets::Button::new("Go")
                        .on_click(ctx.link().callback(|_| ()))
                        .key("go"),
                )
                .into()
        }
    }

    #[test]
    fn an_invalid_key_script_is_reported() {
        let err = Recording::view("bad", static_view)
            .keys("tab,not-a-real-key")
            .quiet(true)
            .record()
            .expect_err("unparseable script must fail");
        assert!(err.to_string().contains("not-a-real-key"), "{err}");
    }

    #[test]
    fn written_cast_starts_with_a_v2_header() {
        let dir = std::env::temp_dir().join(format!("tui-lipan-cast-{}", std::process::id()));
        let path = dir.join("demo.cast");

        Recording::view("demo", static_view)
            .viewport(20, 3)
            .settle(Duration::ZERO)
            .quiet(true)
            .write(&path)
            .expect("writes");

        let text = std::fs::read_to_string(&path).expect("cast file");
        assert!(text.starts_with("{\"version\":2"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
