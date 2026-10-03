//! Interactive animation studio. Run `cargo run --example visibility_animation`.
//! D plays a guided tour; P/M/T/A toggle hosts; 1/2/3 select effects; S slows time.
use std::{sync::Arc, time::Duration};
use tui_lipan::prelude::*;
use tui_lipan::{App, OverlayId, Result};
#[path = "support/portal_reveal.rs"]
mod portal_reveal;
use portal_reveal::PortalReveal;

const BACKGROUND: Color = Color::Rgb(13, 18, 28);
const SURFACE: Color = Color::Rgb(21, 29, 43);
const TEXT: Color = Color::Rgb(220, 229, 241);
const MUTED: Color = Color::Rgb(119, 139, 164);
const CYAN: Color = Color::Rgb(93, 222, 213);
const AMBER: Color = Color::Rgb(244, 190, 97);
const VIOLET: Color = Color::Rgb(178, 155, 255);

#[derive(Clone, Copy, Default, PartialEq)]
enum Effect {
    #[default]
    Portal,
    Scan,
    Fade,
}
impl Effect {
    fn label(self) -> &'static str {
        match self {
            Self::Portal => "PORTAL",
            Self::Scan => "SCAN",
            Self::Fade => "FADE",
        }
    }
    fn color(self) -> Color {
        match self {
            Self::Portal => CYAN,
            Self::Scan => AMBER,
            Self::Fade => VIOLET,
        }
    }
    fn animation(self, slow: bool) -> VisibilityAnimation {
        let scale = if slow { 2 } else { 1 };
        let recipe = VisibilityAnimation::new()
            .enter(TransitionConfig {
                duration: Duration::from_millis(500 * scale),
                easing: Easing::EaseOutQuad,
            })
            .exit(TransitionConfig {
                duration: Duration::from_millis(450 * scale),
                easing: Easing::EaseInQuad,
            });
        match self {
            Self::Portal => recipe.effect(|ctx| {
                VisualEffect::Custom(Arc::new(PortalReveal {
                    progress: ctx.progress,
                }))
            }),
            Self::Scan => {
                recipe.effect(|ctx| VisualEffect::Custom(Arc::new(ScanReveal(ctx.progress))))
            }
            Self::Fade => recipe,
        }
    }
}
#[derive(Debug)]
struct ScanReveal(f32);
impl CellEffect for ScanReveal {
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
        let rows = f32::from(ctx.bounds.h) * self.0;
        if f32::from(ctx.y - ctx.bounds.y) >= rows {
            *cell = backdrop.clone();
        }
    }
}

struct Studio;
#[derive(Default)]
struct State {
    effect: Effect,
    slow: bool,
    modal: bool,
    popup: bool,
    details: bool,
    toast: Option<OverlayId>,
    tour: bool,
    generation: u64,
}
#[derive(Clone, Copy)]
enum Host {
    Modal,
    Popover,
    Toast,
    Accordion,
}
#[derive(Clone, Copy)]
enum Msg {
    Choose(Effect),
    Slow,
    Toggle(Host),
    CloseModal,
    ClosePopup,
    DismissToast,
    StartTour,
    Tour(u64, u8),
    Quit,
}

impl Component for Studio {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        if let Msg::Tour(generation, step) = msg {
            return tour_step(ctx, generation, step);
        }
        ctx.state.tour = false;
        ctx.state.generation = ctx.state.generation.wrapping_add(1);
        match msg {
            Msg::Choose(effect) => ctx.state.effect = effect,
            Msg::Slow => ctx.state.slow = !ctx.state.slow,
            Msg::Toggle(host) => toggle_host(ctx, host),
            Msg::CloseModal => ctx.state.modal = false,
            Msg::ClosePopup => ctx.state.popup = false,
            Msg::DismissToast => dismiss_toast(ctx),
            Msg::StartTour => {
                ctx.state.tour = true;
                return tour_step(ctx, ctx.state.generation, 0);
            }
            Msg::Quit => ctx.quit(),
            Msg::Tour(_, _) => {}
        }
        Update::full()
    }
    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        const KEYS: [(char, Msg); 10] = [
            ('1', Msg::Choose(Effect::Portal)),
            ('2', Msg::Choose(Effect::Scan)),
            ('3', Msg::Choose(Effect::Fade)),
            ('s', Msg::Slow),
            ('m', Msg::Toggle(Host::Modal)),
            ('p', Msg::Toggle(Host::Popover)),
            ('t', Msg::Toggle(Host::Toast)),
            ('a', Msg::Toggle(Host::Accordion)),
            ('d', Msg::StartTour),
            ('q', Msg::Quit),
        ];
        let KeyCode::Char(character) = key.code else {
            return KeyUpdate::unhandled(Update::none());
        };
        if key.mods != KeyMods::NONE {
            return KeyUpdate::unhandled(Update::none());
        }
        let Some((_, msg)) = KEYS.iter().find(|(shortcut, _)| *shortcut == character) else {
            return KeyUpdate::unhandled(Update::none());
        };
        KeyUpdate::handled(self.update(*msg, ctx))
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let accent = ctx.state.effect.color();
        let left = VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(modal_card(ctx, accent))
            .child(popover_card(ctx, accent));
        let right = VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(toast_card(ctx, accent))
            .child(accordion_card(ctx, accent));
        let cards: Element = if ctx.viewport().w < 72 {
            VStack::new()
                .height(Length::Auto)
                .gap(1)
                .child(left)
                .child(right)
                .into()
        } else {
            HStack::new()
                .height(Length::Auto)
                .align(Align::Start)
                .gap(2)
                .child(left)
                .child(right)
                .into()
        };
        let page = VStack::new().padding(1).gap(1)
            .style(Style::new().bg(BACKGROUND).fg(TEXT))
            .child(VStack::new().height(Length::Auto)
                .child(Text::new(format!("◈  ANIMATION STUDIO   /   {}", ctx.state.effect.label())).style(Style::new().fg(accent).bold()))
                .child(Text::new("One recipe. Four hosts. Watch the content underneath.").style(Style::new().fg(MUTED))))
            .child(toolbar(ctx, accent))
            .child(cards)
            .child(Text::new(if ctx.state.tour { "▶ Guided tour playing · any control interrupts" } else { "D demo   M modal   P popover   T toast   A details   S slow   Q quit" }).style(Style::new().fg(MUTED)))
            .child(Text::new("Toggle twice mid-flight to reverse. Closing content releases input immediately.").style(Style::new().fg(MUTED)));
        let mut root = ZStack::new().child(
            ScrollView::new()
                .style(Style::new().bg(BACKGROUND).fg(TEXT))
                .child(page),
        );
        if ctx.state.modal {
            let modal: Element = Modal::new().title("◈  A window through the screen")
                .width(Length::Px(52)).padding(1)
                .frame_style(Style::new().bg(SURFACE).fg(accent))
                .title_style(Style::new().fg(accent).bold())
                .backdrop_style(Style::new().transform_bg(ColorTransform::dim(0.25)))
                .animation(ctx.state.effect.animation(ctx.state.slow))
                .on_close(ctx.link().callback(|_| Msg::CloseModal))
                .child(VStack::new().height(Length::Auto).gap(1)
                    .child(Text::new("The title, border, and body share the reveal.").style(Style::new().fg(TEXT)))
                    .child(Text::new("Close me and watch the cards reappear.\nPress M during the exit to reverse it.").style(Style::new().fg(MUTED)))
                    .child(action("Close window", "close-modal", Msg::CloseModal, ctx, accent)))
                .into();
            root = root.child(modal.key("studio-modal"));
        }
        root.into()
    }
}
fn toggle_host(ctx: &mut Context<Studio>, host: Host) {
    match host {
        Host::Modal => ctx.state.modal = !ctx.state.modal,
        Host::Popover => ctx.state.popup = !ctx.state.popup,
        Host::Accordion => ctx.state.details = !ctx.state.details,
        Host::Toast => {
            if ctx.state.toast.is_some() {
                dismiss_toast(ctx);
            } else {
                show_toast(ctx);
            }
        }
    }
}
fn show_toast(ctx: &mut Context<Studio>) {
    dismiss_toast(ctx);
    let recipe = ctx.state.effect.animation(ctx.state.slow);
    ctx.state.toast = Some(
        ctx.toast().push(
            Toast::new("Saved · the same recipe, a different host")
                .title(Some("✓  Looking good"))
                .duration(30.0)
                .width(Length::Px(46))
                .animation(recipe),
        ),
    );
}
fn dismiss_toast(ctx: &mut Context<Studio>) {
    if let Some(id) = ctx.state.toast.take() {
        ctx.toast().dismiss(id);
    }
}
fn tour_step(ctx: &mut Context<Studio>, generation: u64, step: u8) -> Update {
    if !ctx.state.tour || generation != ctx.state.generation {
        return Update::none();
    }
    match step {
        0 => {
            ctx.state.modal = true;
            ctx.state.popup = false;
            ctx.state.details = false;
            dismiss_toast(ctx);
        }
        1 => {
            ctx.state.modal = false;
            ctx.state.popup = true;
        }
        2 => {
            ctx.state.popup = false;
            show_toast(ctx);
        }
        3 => {
            dismiss_toast(ctx);
            ctx.state.details = true;
        }
        4 => ctx.state.details = false,
        5 => ctx.state.details = true,
        6 => ctx.state.details = false,
        _ => {
            ctx.state.tour = false;
            return Update::full();
        }
    }
    let delay = if step == 4 {
        180
    } else if ctx.state.slow {
        1400
    } else {
        900
    };
    Update::with_command(Command::after(Duration::from_millis(delay), move |link| {
        link.send(Msg::Tour(generation, step + 1))
    }))
}
fn action(
    label: &str,
    id: &'static str,
    msg: Msg,
    ctx: &Context<Studio>,
    accent: Color,
) -> Element {
    Button::new(label)
        .variant(ButtonVariant::Filled)
        .padding((0, 1))
        .style(Style::new().fg(BACKGROUND).bg(accent))
        .on_click(ctx.link().callback(move |_| msg))
        .automation_id(id)
}
fn card(title: &str, accent: Color, content: impl Into<Element>) -> Frame {
    Frame::new()
        .header_left(title)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().fg(accent).bg(SURFACE))
        .padding((1, 2))
        .height(Length::Auto)
        .width(Length::Flex(1))
        .child(content)
}
fn caption(text: &str) -> Text {
    Text::new(text).style(Style::new().fg(MUTED))
}
fn toolbar(ctx: &Context<Studio>, accent: Color) -> Element {
    let mut row = HStack::new().height(Length::Auto).gap(1);
    for (effect, label, id) in [
        (Effect::Portal, "1 Portal", "portal"),
        (Effect::Scan, "2 Scan", "scan"),
        (Effect::Fade, "3 Fade", "fade"),
    ] {
        let color = if ctx.state.effect == effect {
            accent
        } else {
            MUTED
        };
        row = row.child(action(label, id, Msg::Choose(effect), ctx, color));
    }
    row.child(action(
        if ctx.state.slow {
            "S Slow ●"
        } else {
            "S Slow ○"
        },
        "slow",
        Msg::Slow,
        ctx,
        MUTED,
    ))
    .child(action("D Play demo", "demo", Msg::StartTour, ctx, accent))
    .into()
}
fn modal_card(ctx: &Context<Studio>, accent: Color) -> Element {
    card(
        "01  MODAL",
        accent,
        VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(caption("A complete dialog, title and border included."))
            .child(action(
                "M  Open window",
                "modal",
                Msg::Toggle(Host::Modal),
                ctx,
                accent,
            )),
    )
    .key("modal-card")
}
fn popover_card(ctx: &Context<Studio>, accent: Color) -> Element {
    let popup = card(
        "A little room above the page",
        accent,
        VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(Text::new("Live cards return as this closes.").style(Style::new().fg(TEXT)))
            .child(action(
                "Close popover",
                "close-popup",
                Msg::ClosePopup,
                ctx,
                accent,
            )),
    )
    .width(Length::Px(38));
    card(
        "02  POPOVER",
        accent,
        VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(caption("An anchored layer with a live backdrop."))
            .child(
                Popover::new()
                    .trigger(action(
                        "P  Toggle popover",
                        "popup",
                        Msg::Toggle(Host::Popover),
                        ctx,
                        accent,
                    ))
                    .content(popup)
                    .open(ctx.state.popup)
                    .on_close(ctx.link().callback(|_| Msg::ClosePopup))
                    .animation(ctx.state.effect.animation(ctx.state.slow)),
            ),
    )
    .key("popover-card")
}
fn toast_card(ctx: &Context<Studio>, accent: Color) -> Element {
    card(
        "03  TOAST",
        accent,
        VStack::new()
            .height(Length::Auto)
            .gap(1)
            .child(caption("Small feedback. The same enter/exit recipe."))
            .child(
                HStack::new()
                    .height(Length::Auto)
                    .gap(1)
                    .child(action(
                        "T  Toggle toast",
                        "toast",
                        Msg::Toggle(Host::Toast),
                        ctx,
                        accent,
                    ))
                    .child(action(
                        "Dismiss",
                        "close-toast",
                        Msg::DismissToast,
                        ctx,
                        MUTED,
                    )),
            ),
    )
    .key("toast-card")
}
fn accordion_card(ctx: &Context<Studio>, accent: Color) -> Element {
    card("04  ACCORDION", accent, VStack::new().height(Length::Auto).gap(1)
        .child(action("A  Toggle details", "details-toggle", Msg::Toggle(Host::Accordion), ctx, accent))
        .child(Accordion::new().border(false).content_border(false).content_padding(0).header_padding(0)
            .header_style(Style::new().fg(TEXT)).content_style(Style::new().fg(TEXT))
            .animation(ctx.state.effect.animation(ctx.state.slow))
            .item(AccordionItem::new("Project details", Text::new("● Natural size stays steady\n● The visible height changes\n● Closing releases its scope\n● Reopening reverses smoothly"))
                .expanded(ctx.state.details)))
        .child(Text::new("↑ This row follows the animated height").style(Style::new().fg(accent))))
        .key("accordion-card")
}
fn main() -> Result<()> {
    App::new().mount(Studio).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;
    fn key(backend: &mut TestBackend<Studio>, character: char) {
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char(character),
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.pump().unwrap();
    }
    #[test]
    fn keyboard_controls_open_close_and_select_effects() {
        let mut backend = TestBackend::new(Studio);
        backend.render();
        key(&mut backend, '2');
        assert!(backend.state().effect == Effect::Scan);
        key(&mut backend, 's');
        assert!(backend.state().slow);
        key(&mut backend, 'm');
        assert!(backend.state().modal);
        backend.advance(Duration::from_millis(1000));
        assert!(
            backend
                .capture_frame()
                .plain_text()
                .contains("A window through")
        );
        key(&mut backend, 'm');
        assert!(!backend.state().modal);
        backend.advance(Duration::from_millis(900));
        assert!(
            !backend
                .capture_frame()
                .plain_text()
                .contains("A window through")
        );
        key(&mut backend, 'a');
        assert!(backend.state().details);
        key(&mut backend, 't');
        assert!(backend.state().toast.is_some());
        key(&mut backend, 't');
        assert!(backend.state().toast.is_none());
    }
    #[test]
    fn guided_tour_finishes_and_manual_controls_cancel_pending_steps() {
        let mut backend = TestBackend::new(Studio);
        backend.render();
        key(&mut backend, 'd');
        for ms in [900, 900, 900, 900, 180, 900, 900] {
            backend.advance(Duration::from_millis(ms));
            backend.pump().unwrap();
        }
        assert!(!backend.state().tour);
        assert!(!backend.state().modal && !backend.state().popup && !backend.state().details);
        key(&mut backend, 'd');
        key(&mut backend, '2');
        backend.advance(Duration::from_millis(900));
        assert!(!backend.state().tour);
        assert!(backend.state().modal && !backend.state().popup);
    }
}
