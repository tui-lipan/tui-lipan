//! An application-defined portal animation for a modal, including retained close and reversal.
use std::sync::Arc;
use std::time::Duration;

use tui_lipan::prelude::*;
use tui_lipan::{App, Result};

#[path = "support/portal_reveal.rs"]
mod portal_reveal;
use portal_reveal::PortalReveal;

struct Demo;
#[derive(Default)]
struct State {
    open: bool,
}
enum Msg {
    Toggle,
}
impl Component for Demo {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, _: Msg, ctx: &mut Context<Self>) -> Update {
        ctx.state.open = !ctx.state.open;
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut root = ZStack::new().child(
            VStack::new()
                .style(
                    Style::new()
                        .bg(Color::Rgb(18, 24, 34))
                        .fg(Color::Rgb(220, 224, 230)),
                )
                .padding(2)
                .gap(1)
                .child(Text::new("Custom modal animation"))
                .child(Text::new(
                    "The portal reveals the live content beneath the dialog.",
                ))
                .child(
                    Button::new("Open picker")
                        .on_click(ctx.link().callback(|_| Msg::Toggle))
                        .automation_id("open"),
                )
                .child(
                    Text::new("Live application content beneath the picker.\n".repeat(12))
                        .style(Style::new().fg(Color::Rgb(100, 125, 150))),
                ),
        );
        if ctx.state.open {
            let animation = VisibilityAnimation::new()
                .enter(TransitionConfig {
                    duration: Duration::from_millis(400),
                    easing: Easing::EaseOutQuad,
                })
                .exit(TransitionConfig {
                    duration: Duration::from_millis(300),
                    easing: Easing::EaseInQuad,
                })
                .effect(|context| {
                    VisualEffect::Custom(Arc::new(PortalReveal {
                        progress: context.progress,
                    }))
                });
            let modal: Element = Modal::new()
                .title("Picker")
                .width(Length::Px(44))
                .animation(animation)
                .backdrop_style(Style::new().transform_bg(ColorTransform::dim(0.35)))
                .on_close(ctx.link().callback(|_| Msg::Toggle))
                .child(
                    VStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(Text::new("Any VisualEffect can use lifecycle progress."))
                        .child(
                            Button::new("Close")
                                .on_click(ctx.link().callback(|_| Msg::Toggle))
                                .automation_id("close"),
                        ),
                )
                .into();
            root = root.child(modal.key("picker"));
        }
        root.into()
    }
}
fn main() -> Result<()> {
    App::new().mount(Demo).run()
}
