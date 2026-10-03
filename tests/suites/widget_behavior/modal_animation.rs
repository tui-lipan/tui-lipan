//! Declarative modal transitions retain their frame and composite over the live application.
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use tui_lipan::prelude::*;
use tui_lipan::{
    TestBackend, VisibilityAnimation, VisibilityAnimationContext, VisibilityAnimationPhase,
};

#[derive(Debug)]
struct Wipe(f32);
impl CellEffect for Wipe {
    fn apply(&self, _: &mut EffectCell, _: &EffectContext) {}
    fn uses_backdrop(&self) -> bool {
        true
    }
    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        let x = f32::from(ctx.x - ctx.bounds.x);
        if x >= self.0 * f32::from(ctx.bounds.w) {
            *cell = backdrop.clone();
        }
    }
}

#[derive(Default)]
struct State {
    open: bool,
    background: char,
    clicks: usize,
}
enum Msg {
    Open,
    Close,
    Click,
}
struct Host {
    animation: VisibilityAnimation,
}
impl Component for Host {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State {
            open: true,
            background: 'b',
            clicks: 0,
        }
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Open => ctx.state.open = true,
            Msg::Close => ctx.state.open = false,
            Msg::Click => ctx.state.clicks += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let row = ctx.state.background.to_string().repeat(40);
        let background = Text::new(format!("{row}\n").repeat(12))
            .style(Style::new().bg(Color::Rgb(20, 180, 20)).fg(Color::Black))
            .height(Length::Flex(1));
        let mut root = ZStack::new().child(background);
        if ctx.state.open {
            let modal: Element = Modal::new()
                .title("Picker")
                .width(Length::Px(20))
                .height(Length::Px(6))
                .padding(0)
                .frame_style(Style::new().bg(Color::Rgb(200, 20, 20)))
                .focus_style(Style::new().bg(Color::Rgb(200, 20, 20)))
                .animation(self.animation.clone())
                .child(
                    Button::new("PICKER")
                        .on_click(ctx.link().callback(|_| Msg::Click))
                        .key("action"),
                )
                .into();
            root = root.child(modal.key("picker"));
        }
        root.into()
    }
}
fn modal_backend(animation: VisibilityAnimation) -> TestBackend<Host> {
    let mut backend = TestBackend::new(Host { animation });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    backend
}
fn timing(ms: u64) -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(ms),
        easing: Easing::Linear,
    }
}
fn custom_animation(samples: Rc<RefCell<Vec<VisibilityAnimationContext>>>) -> VisibilityAnimation {
    VisibilityAnimation::new()
        .enter(timing(200))
        .exit(timing(200))
        .effect(move |ctx| {
            samples.borrow_mut().push(ctx);
            VisualEffect::Custom(Arc::new(Wipe(ctx.progress)))
        })
}

#[test]
fn custom_modal_effects_restore_the_live_backdrop_including_title_and_border() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut backend = modal_backend(custom_animation(samples.clone()));
    assert_eq!(
        backend.capture_frame().cell(10, 3).symbol,
        "b",
        "hidden title restores backdrop"
    );
    backend.advance(Duration::from_millis(100));
    let halfway = backend.capture_frame();
    assert_ne!(
        halfway.cell(10, 3).symbol,
        "b",
        "border reveals with content"
    );
    assert_eq!(
        halfway.cell(29, 3).symbol,
        "b",
        "hidden border reveals backdrop"
    );
    backend.state_mut().background = 'c';
    backend.render();
    assert_eq!(
        backend.capture_frame().cell(29, 3).symbol,
        "c",
        "backdrop is live, not frozen"
    );
    backend.advance(Duration::from_millis(100));
    assert!(backend.capture_frame().plain_text().contains("PICKER"));
    backend.dispatch(Msg::Close).expect("close");
    backend.advance(Duration::from_millis(100));
    let exiting = backend.capture_frame();
    assert_ne!(
        exiting.cell(10, 3).symbol,
        "c",
        "retained border still paints"
    );
    assert_eq!(exiting.cell(29, 3).symbol, "c");
    assert!(
        samples
            .borrow()
            .iter()
            .any(|ctx| ctx.phase == VisibilityAnimationPhase::Exiting)
    );
    backend.advance(Duration::from_millis(100));
    let closed = backend.capture_frame();
    assert_eq!(closed.cell(10, 3).symbol, "c");
    assert!(!closed.plain_text().contains("PICKER"));
}

#[test]
fn reopening_a_modal_reverses_from_its_current_visibility() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut backend = modal_backend(custom_animation(samples.clone()));
    backend.advance(Duration::from_millis(200));
    backend.dispatch(Msg::Close).expect("close");
    backend.advance(Duration::from_millis(80));
    backend.capture_frame();
    let exiting = *samples.borrow().last().expect("exit sample");
    assert!((exiting.progress - 0.6).abs() < 0.01);
    backend.dispatch(Msg::Open).expect("reopen");
    backend.capture_frame();
    let reopening = *samples.borrow().last().expect("reopening sample");
    assert_eq!(reopening.phase, VisibilityAnimationPhase::Entering);
    assert!((reopening.progress - exiting.progress).abs() < 0.01);
    backend.advance(Duration::from_millis(200));
    assert!(backend.capture_frame().plain_text().contains("PICKER"));
}

#[test]
fn closing_modal_content_is_inert_and_zero_duration_removal_is_immediate() {
    let mut backend = modal_backend(
        VisibilityAnimation::new()
            .enter(timing(0))
            .exit(timing(200)),
    );
    assert!(backend.focus_key(&Key::from("action")));
    backend
        .send_key(KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        })
        .expect("key");
    backend.pump().expect("click pumps");
    assert_eq!(backend.state().clicks, 1, "live content handles input");
    backend.dispatch(Msg::Close).expect("close");
    backend
        .send_key(KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        })
        .expect("key");
    backend.pump().expect("closing input pumps");
    assert_eq!(
        backend.state().clicks,
        1,
        "retained content must not handle input"
    );
    backend.advance(Duration::from_millis(200));
    assert!(!backend.capture_frame().plain_text().contains("PICKER"));
    let mut instant = modal_backend(VisibilityAnimation::new().enter(timing(0)).exit(timing(0)));
    instant.dispatch(Msg::Close).expect("close");
    assert!(!instant.capture_frame().plain_text().contains("PICKER"));
}

#[test]
fn default_fade_composites_the_complete_frame_on_entry_and_exit() {
    let mut backend = modal_backend(
        VisibilityAnimation::new()
            .enter(timing(200))
            .exit(timing(200)),
    );
    let hidden = backend.capture_frame();
    assert_eq!(hidden.cell(10, 3).symbol, "b");
    backend.advance(Duration::from_millis(100));
    let entering = backend.capture_frame();
    backend.advance(Duration::from_millis(100));
    let visible = backend.capture_frame();
    assert_ne!(entering.cell(10, 3).bg, hidden.cell(10, 3).bg);
    assert_ne!(entering.cell(10, 3).bg, visible.cell(10, 3).bg);
    backend.dispatch(Msg::Close).expect("close");
    backend.advance(Duration::from_millis(100));
    let exiting = backend.capture_frame();
    assert_eq!(exiting.cell(10, 3).bg, entering.cell(10, 3).bg);
    backend.advance(Duration::from_millis(100));
    assert_eq!(backend.capture_frame().cell(10, 3).symbol, "b");
}
