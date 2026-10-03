//! Shared overlay and inline visibility lifecycle contracts.
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use tui_lipan::prelude::*;
use tui_lipan::{OverlayId, TestBackend};

fn timing(ms: u64) -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(ms),
        easing: Easing::Linear,
    }
}
#[derive(Debug)]
struct Reveal(f32);
impl CellEffect for Reveal {
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
        if f32::from(ctx.x - ctx.bounds.x) >= self.0 * f32::from(ctx.bounds.w) {
            *cell = backdrop.clone();
        }
    }
}
fn recipe(samples: Rc<RefCell<Vec<VisibilityAnimationContext>>>) -> VisibilityAnimation {
    VisibilityAnimation::new()
        .enter(timing(200))
        .exit(timing(200))
        .effect(move |ctx| {
            samples.borrow_mut().push(ctx);
            VisualEffect::Custom(Arc::new(Reveal(ctx.progress)))
        })
}
#[derive(Clone, Copy)]
enum Kind {
    Popover,
    Select,
    ComboBox,
    ContextMenu,
    Tooltip,
    Accordion,
    NarrowAccordion,
    Toast,
    CommandPalette,
}
struct Host {
    kind: Kind,
    animation: VisibilityAnimation,
}
#[derive(Default)]
struct State {
    open: bool,
    clicks: usize,
    trigger_clicks: usize,
    toast: Option<OverlayId>,
}
enum Msg {
    Open,
    Close,
    Click,
    Trigger,
}
impl Component for Host {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Open => {
                ctx.state.open = true;
                if matches!(self.kind, Kind::Toast) {
                    ctx.state.toast = Some(
                        ctx.toast().push(
                            Toast::new("ALPHA")
                                .duration(10.0)
                                .animation(self.animation.clone()),
                        ),
                    );
                }
            }
            Msg::Close => {
                ctx.state.open = false;
                if let Some(id) = ctx.state.toast {
                    ctx.toast().dismiss(id);
                }
            }
            Msg::Click => ctx.state.clicks += 1,
            Msg::Trigger => ctx.state.trigger_clicks += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let action = || {
            Button::new("ACTION")
                .on_click(ctx.link().callback(|_| Msg::Click))
                .key("action")
        };
        let trigger = || {
            Button::new("TRIGGER")
                .on_click(ctx.link().callback(|_| Msg::Trigger))
                .key("trigger")
        };
        let widget: Element = match self.kind {
            Kind::Popover => Popover::new()
                .trigger(trigger())
                .content(
                    Frame::new()
                        .width(Length::Px(20))
                        .height(Length::Px(5))
                        .child(action()),
                )
                .open(ctx.state.open)
                .animation(self.animation.clone())
                .into(),
            Kind::Select => Select::new()
                .options(["ALPHA", "BETA"])
                .expanded(ctx.state.open)
                .animation(self.animation.clone())
                .into(),
            Kind::ComboBox => ComboBox::new()
                .items(["ALPHA", "BETA"])
                .open(ctx.state.open)
                .animation(self.animation.clone())
                .into(),
            Kind::ContextMenu => ContextMenu::new(trigger())
                .items(["ALPHA", "BETA"])
                .open(ctx.state.open)
                .animation(self.animation.clone())
                .into(),
            Kind::Tooltip => Tooltip::new("ALPHA")
                .child(trigger())
                .open(ctx.state.open)
                .animation(self.animation.clone())
                .into(),
            Kind::NarrowAccordion => Frame::new()
                .width(Length::Px(9))
                .height(Length::Auto)
                .border(false)
                .child(
                    Accordion::new()
                        .gap(0)
                        .content_border(false)
                        .border(false)
                        .content_padding(0)
                        .animation(self.animation.clone())
                        .item(
                            AccordionItem::new(
                                "Section",
                                Text::new("abcdefgh abcdefgh abcdefgh abcdefgh")
                                    .overflow(Overflow::Wrap),
                            )
                            .expanded(ctx.state.open),
                        ),
                )
                .into(),
            Kind::Accordion => Accordion::new()
                .border(false)
                .gap(0)
                .content_padding(0)
                .animation(self.animation.clone())
                .item(
                    AccordionItem::new(
                        "Section",
                        VStack::new()
                            .height(Length::Auto)
                            .child(action())
                            .child(Text::new("one\ntwo\nthree\nfour\nfive")),
                    )
                    .expanded(ctx.state.open),
                )
                .into(),
            Kind::CommandPalette if ctx.state.open => CommandPalette::new()
                .title("PALETTE")
                .width(Length::Px(24))
                .height(Length::Px(6))
                .animation(self.animation.clone())
                .into(),
            Kind::CommandPalette => Spacer::new().into(),
            Kind::Toast => Text::new("Background").into(),
        };
        let mut root = VStack::new().gap(0);
        if !matches!(self.kind, Kind::CommandPalette) || ctx.state.open {
            root = root.child(widget.key("widget"));
        }
        root.child(Text::new("AFTER")).into()
    }
}
fn backend(kind: Kind, animation: VisibilityAnimation) -> TestBackend<Host> {
    let mut backend = TestBackend::new(Host { kind, animation });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 20,
    });
    backend.render();
    backend
}
fn enter(backend: &mut TestBackend<Host>) {
    backend
        .send_key(KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        })
        .unwrap();
    backend.pump().unwrap();
}
#[test]
fn popover_wrappers_forward_entry_exit_and_reversal() {
    for kind in [
        Kind::Popover,
        Kind::Select,
        Kind::ComboBox,
        Kind::ContextMenu,
        Kind::Tooltip,
    ] {
        let samples = Rc::new(RefCell::new(Vec::new()));
        let mut backend = backend(kind, recipe(samples.clone()));
        assert!(
            samples.borrow().is_empty(),
            "closed popups must not animate on mount"
        );
        backend.dispatch(Msg::Open).unwrap();
        backend.advance(Duration::from_millis(200));
        backend.dispatch(Msg::Close).unwrap();
        backend.advance(Duration::from_millis(100));
        backend.capture_frame();
        let exiting = *samples.borrow().last().expect("exit effect");
        assert_eq!(exiting.phase, VisibilityAnimationPhase::Exiting);
        assert!((exiting.progress - 0.5).abs() < 0.01);
        backend.dispatch(Msg::Open).unwrap();
        backend.capture_frame();
        let reopened = *samples.borrow().last().unwrap();
        assert_eq!(reopened.phase, VisibilityAnimationPhase::Entering);
        assert!((reopened.progress - exiting.progress).abs() < 0.01);
        backend.advance(Duration::from_millis(200));
        backend.dispatch(Msg::Close).unwrap();
        backend.advance(Duration::from_millis(200));
        let count = samples.borrow().len();
        backend.capture_frame();
        assert_eq!(
            samples.borrow().len(),
            count,
            "closed effects must stop painting"
        );
    }
}
#[test]
fn popover_closing_content_is_inert_while_trigger_remains_active() {
    let mut backend = backend(
        Kind::Popover,
        VisibilityAnimation::new()
            .enter(timing(0))
            .exit(timing(200)),
    );
    backend.dispatch(Msg::Open).unwrap();
    assert!(backend.focus_key(&Key::from("action")));
    enter(&mut backend);
    assert_eq!(backend.state().clicks, 1);
    backend.dispatch(Msg::Close).unwrap();
    enter(&mut backend);
    assert_eq!(backend.state().clicks, 1);
    assert!(backend.capture_frame().plain_text().contains("ACTION"));
    assert!(backend.focus_key(&Key::from("trigger")));
    enter(&mut backend);
    assert_eq!(backend.state().trigger_clicks, 2);
    backend.advance(Duration::from_millis(200));
    assert!(!backend.capture_frame().plain_text().contains("ACTION"));
}
#[test]
fn accordion_reflows_neighbors_and_reverses_without_snapping() {
    let mut backend = backend(
        Kind::Accordion,
        VisibilityAnimation::new()
            .enter(timing(200))
            .exit(timing(200)),
    );
    let after_row = |backend: &mut TestBackend<Host>| {
        backend
            .capture_frame()
            .plain_text()
            .lines()
            .position(|line| line.contains("AFTER"))
            .unwrap()
    };
    let closed = after_row(&mut backend);
    assert!(!backend.capture_frame().plain_text().contains("ACTION"));
    backend.dispatch(Msg::Open).unwrap();
    assert_eq!(after_row(&mut backend), closed);
    backend.advance(Duration::from_millis(100));
    let half = after_row(&mut backend);
    assert!(half > closed);
    backend.advance(Duration::from_millis(100));
    let full = after_row(&mut backend);
    assert!(full > half);
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(100));
    assert_eq!(after_row(&mut backend), half);
    backend.dispatch(Msg::Open).unwrap();
    assert_eq!(after_row(&mut backend), half);
    backend.advance(Duration::from_millis(200));
    assert_eq!(after_row(&mut backend), full);
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(200));
    assert_eq!(after_row(&mut backend), closed);
    assert!(!backend.capture_frame().plain_text().contains("ACTION"));
}
#[test]
fn toast_uses_custom_effects_and_removes_content_after_exit() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut backend = backend(Kind::Toast, recipe(samples.clone()));
    backend.dispatch(Msg::Open).unwrap();
    backend.advance(Duration::from_millis(100));
    backend.capture_frame();
    assert!((samples.borrow().last().unwrap().progress - 0.5).abs() < 0.01);
    backend.advance(Duration::from_millis(100));
    assert!(backend.capture_frame().plain_text().contains("ALPHA"));
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(100));
    backend.capture_frame();
    assert_eq!(
        samples.borrow().last().unwrap().phase,
        VisibilityAnimationPhase::Exiting
    );
    backend.advance(Duration::from_millis(100));
    assert!(!backend.capture_frame().plain_text().contains("ALPHA"));
}

#[test]
fn command_palette_retains_its_modal_after_component_removal() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut backend = backend(Kind::CommandPalette, recipe(samples.clone()));
    backend.dispatch(Msg::Open).unwrap();
    backend.advance(Duration::from_millis(200));
    assert!(backend.capture_frame().plain_text().contains("PALETTE"));
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(100));
    backend.capture_frame();
    assert_eq!(
        samples.borrow().last().unwrap().phase,
        VisibilityAnimationPhase::Exiting
    );
    backend.advance(Duration::from_millis(100));
    assert!(!backend.capture_frame().plain_text().contains("PALETTE"));
}

#[test]
fn accordion_custom_effects_and_zero_duration_visibility_are_supported() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut animated = backend(Kind::Accordion, recipe(samples.clone()));
    animated.dispatch(Msg::Open).unwrap();
    animated.advance(Duration::from_millis(100));
    animated.capture_frame();
    assert_eq!(
        samples.borrow().last().unwrap().phase,
        VisibilityAnimationPhase::Entering
    );
    animated.advance(Duration::from_millis(100));
    assert!(animated.focus_key(&Key::from("action")));
    enter(&mut animated);
    assert_eq!(animated.state().clicks, 1);
    animated.dispatch(Msg::Close).unwrap();
    enter(&mut animated);
    assert_eq!(animated.state().clicks, 1);
    animated.advance(Duration::from_millis(100));
    animated.capture_frame();
    assert_eq!(
        samples.borrow().last().unwrap().phase,
        VisibilityAnimationPhase::Exiting
    );
    for kind in [Kind::Popover, Kind::Accordion, Kind::Toast] {
        let mut instant = backend(
            kind,
            VisibilityAnimation::new().enter(timing(0)).exit(timing(0)),
        );
        instant.dispatch(Msg::Open).unwrap();
        assert!(
            instant
                .capture_frame()
                .plain_text()
                .contains(if matches!(kind, Kind::Toast) {
                    "ALPHA"
                } else {
                    "ACTION"
                })
        );
        instant.dispatch(Msg::Close).unwrap();
        assert!(
            !instant
                .capture_frame()
                .plain_text()
                .contains(if matches!(kind, Kind::Toast) {
                    "ALPHA"
                } else {
                    "ACTION"
                })
        );
    }
}

#[test]
fn popover_erased_title_and_border_reveal_the_application_instead_of_a_duplicate_popup() {
    let samples = Rc::new(RefCell::new(Vec::new()));
    let mut backend = backend(Kind::Popover, recipe(samples));
    let beneath = backend.capture_frame().cell(0, 1).symbol.clone();
    assert_eq!(beneath, "A", "AFTER is beneath the popup");
    backend.dispatch(Msg::Open).unwrap();
    assert_eq!(
        backend.capture_frame().cell(0, 1).symbol,
        beneath,
        "hidden border must not have been painted in the main tree"
    );
    backend.advance(Duration::from_millis(100));
    let entering = backend.capture_frame();
    assert_ne!(entering.cell(0, 1).symbol, beneath);
    assert_eq!(
        entering.cell(39, 1).symbol,
        " ",
        "erased right border restores the live backdrop"
    );
    backend.advance(Duration::from_millis(100));
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(100));
    assert_eq!(backend.capture_frame().cell(39, 1).symbol, " ");
}

#[test]
fn accordion_measures_wrapping_at_its_allocated_width() {
    let mut backend = backend(
        Kind::NarrowAccordion,
        VisibilityAnimation::new()
            .enter(timing(200))
            .exit(timing(200)),
    );
    let after_row = |backend: &mut TestBackend<Host>| {
        backend
            .capture_frame()
            .plain_text()
            .lines()
            .position(|line| line.contains("AFTER"))
            .unwrap()
    };
    let closed = after_row(&mut backend);
    backend.dispatch(Msg::Open).unwrap();
    backend.advance(Duration::from_millis(100));
    assert_eq!(after_row(&mut backend), 3);
    backend.advance(Duration::from_millis(100));
    assert_eq!(after_row(&mut backend), 5);
    backend.dispatch(Msg::Close).unwrap();
    backend.advance(Duration::from_millis(100));
    assert_eq!(after_row(&mut backend), 3);
    backend.advance(Duration::from_millis(100));
    assert_eq!(after_row(&mut backend), closed);
}
