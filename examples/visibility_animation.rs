//! One custom portal effect reused by Popover, Toast, and Accordion.
use std::{sync::Arc, time::Duration};
use tui_lipan::prelude::*;
use tui_lipan::{App, OverlayId, Result};
#[path = "support/portal_reveal.rs"]
mod portal_reveal;
use portal_reveal::PortalReveal;

fn animation() -> VisibilityAnimation {
    VisibilityAnimation::new()
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
        })
}
struct Demo;
#[derive(Default)]
struct State {
    popup: bool,
    details: bool,
    toast: Option<OverlayId>,
}
enum Msg {
    TogglePopup,
    ClosePopup,
    ToggleDetails,
    ShowToast,
    CloseToast,
}
impl Component for Demo {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TogglePopup => ctx.state.popup = !ctx.state.popup,
            Msg::ClosePopup => ctx.state.popup = false,
            Msg::ToggleDetails => ctx.state.details = !ctx.state.details,
            Msg::ShowToast => {
                if let Some(id) = ctx.state.toast {
                    ctx.toast().dismiss_immediately(id);
                }
                ctx.state.toast = Some(
                    ctx.toast().push(
                        Toast::new("The same portal effect animates this toast.")
                            .duration(30.0)
                            .width(Length::Px(40))
                            .animation(animation()),
                    ),
                );
            }
            Msg::CloseToast => {
                if let Some(id) = ctx.state.toast {
                    ctx.toast().dismiss(id);
                }
            }
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new().padding(2).gap(1)
            .style(Style::new().bg(Color::Rgb(18, 24, 34)).fg(Color::Rgb(220, 224, 230)))
            .child(Text::new("Shared visibility animations"))
            .child(Text::new("Open, close, or reverse any transition by toggling it again."))
            .child(Popover::new()
                .trigger(Button::new("Toggle popup").on_click(ctx.link().callback(|_| Msg::TogglePopup)).automation_id("popup"))
                .content(Frame::new().header_left("Popover").width(Length::Px(36)).height(Length::Px(5))
                    .child(Button::new("Close popup").on_click(ctx.link().callback(|_| Msg::ClosePopup)).automation_id("close-popup")))
                .open(ctx.state.popup).on_close(ctx.link().callback(|_| Msg::ClosePopup)).animation(animation()))
            .child(HStack::new().height(Length::Auto).gap(2)
                .child(Button::new("Show toast").on_click(ctx.link().callback(|_| Msg::ShowToast)).automation_id("toast"))
                .child(Button::new("Dismiss toast").on_click(ctx.link().callback(|_| Msg::CloseToast)).automation_id("close-toast")))
            .child(Button::new("Toggle details").on_click(ctx.link().callback(|_| Msg::ToggleDetails)).automation_id("details-toggle"))
            .child(Accordion::new().focusable(true).animation(animation())
                .on_toggle(ctx.link().callback(|_| Msg::ToggleDetails))
                .item(AccordionItem::new("Details", Text::new("The content stays full size while its height is clipped.\nClosing releases its input immediately.\nThe next row follows the changing height.\nThe same effect factory paints this section."))
                    .expanded(ctx.state.details)).automation_id("details"))
            .child(Text::new("This row moves as the Accordion opens and closes."))
            .child(Text::new("Live application content beneath the popup.\n".repeat(12))
                .style(Style::new().fg(Color::Rgb(100, 125, 150))))
            .into()
    }
}
fn main() -> Result<()> {
    App::new().mount(Demo).run()
}
