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
    key_delay: Duration,
    settle: Duration,
    quiet: bool,
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
            key_delay: DEFAULT_KEY_DELAY,
            settle: DEFAULT_SETTLE,
            quiet: false,
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
    #[must_use]
    pub fn keys(mut self, script: impl AsRef<str>) -> Self {
        self.key_script = Some(script.as_ref().to_owned());
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

    /// Play the script and return the recording without writing it.
    pub fn record(self) -> Result<CastRecording> {
        let Self {
            title,
            component,
            viewport,
            fps,
            key_script,
            key_delay,
            settle,
            quiet: _,
        } = self;

        let keys = match key_script.as_deref() {
            Some(script) => super::keys::parse_key_script(script)?,
            None => Vec::new(),
        };

        let (w, h) = viewport;
        let mut backend = TestBackend::new(component);
        backend.set_viewport(Rect { x: 0, y: 0, w, h });
        backend.render();

        let mut cast = CastRecording::new(w, h).title(title);
        let step = Duration::from_secs_f64(1.0 / f64::from(fps));
        let mut clock = Duration::ZERO;

        cast.push_frame(clock.as_secs_f64(), &backend.capture_frame());

        for key in &keys {
            backend.send_key(*key)?;
            clock += step;
            cast.push_frame(clock.as_secs_f64(), &backend.capture_frame());

            hold(&mut backend, &mut cast, &mut clock, key_delay, step);
        }

        hold(&mut backend, &mut cast, &mut clock, settle, step);
        // Identical frames were dropped, so without this the cast would end at
        // the last visible change and a player would cut the settle short.
        cast.mark_time(clock.as_secs_f64());

        Ok(cast)
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

/// Advance the synthetic clock by `total`, capturing a frame every `step`.
///
/// Animations are ticked in clamped increments so a long hold cannot skip a
/// transition, matching how the runner paces its own frames.
fn hold<C: Component>(
    backend: &mut TestBackend<C>,
    cast: &mut CastRecording,
    clock: &mut Duration,
    total: Duration,
    step: Duration,
) {
    let mut remaining = total;
    while !remaining.is_zero() {
        let tick = step.min(remaining).min(MAX_TICK);
        backend.advance(tick);
        *clock += tick;
        cast.push_frame(clock.as_secs_f64(), &backend.capture_frame());
        remaining = remaining.saturating_sub(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::component::{Context, KeyUpdate, Update};
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
