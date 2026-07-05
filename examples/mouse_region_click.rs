//! MouseRegion click-capture demo.
//!
//! Run with: cargo run --example mouse_region_click

use tui_lipan::prelude::*;

struct MouseRegionClickDemo;

#[derive(Default)]
struct State {
    pass_region_clicks: u32,
    pass_button_clicks: u32,
    capture_region_clicks: u32,
    capture_button_clicks: u32,
    last_event: String,
}

#[derive(Clone, Debug)]
enum Msg {
    PassRegion(u16, u16),
    PassButton(u16, u16),
    CaptureRegion(u16, u16),
    CaptureButton(u16, u16),
}

impl Component for MouseRegionClickDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            last_event: "Click buttons and panel backgrounds. Right panel captures button clicks."
                .into(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::PassRegion(x, y) => {
                ctx.state.pass_region_clicks = ctx.state.pass_region_clicks.saturating_add(1);
                ctx.state.last_event = format!("Passthrough panel region click at ({x}, {y})");
            }
            Msg::PassButton(x, y) => {
                ctx.state.pass_button_clicks = ctx.state.pass_button_clicks.saturating_add(1);
                ctx.state.last_event = format!("Passthrough panel button click at ({x}, {y})");
            }
            Msg::CaptureRegion(x, y) => {
                ctx.state.capture_region_clicks = ctx.state.capture_region_clicks.saturating_add(1);
                ctx.state.last_event = format!("Capture panel region click at ({x}, {y})");
            }
            Msg::CaptureButton(x, y) => {
                ctx.state.capture_button_clicks = ctx.state.capture_button_clicks.saturating_add(1);
                ctx.state.last_event = format!("Capture panel button click at ({x}, {y})");
            }
        }
        Update::full()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("MouseRegion click capture")
            .status(format!("{} | q/esc: quit", ctx.state.last_event))
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                HStack::new()
                    .gap(1)
                    .child(self.passthrough_panel(ctx))
                    .child(self.capture_panel(ctx)),
            )
            .into()
    }
}

impl MouseRegionClickDemo {
    fn passthrough_panel(&self, ctx: &Context<Self>) -> Element {
        MouseRegion::new()
            .on_click(
                ctx.link()
                    .callback(|e: MouseEvent| Msg::PassRegion(e.x, e.y)),
            )
            .capture_click(false)
            .hover_style(Style::new().bg(Color::indexed(236)))
            .child(
                Frame::new()
                    .title("capture_click(false)")
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(234)))
                    .hover_style(Style::new().bg(Color::indexed(236)))
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Button clicks go to button first."))
                            .child(
                                Button::new(format!(
                                    "Button clicks: {}",
                                    ctx.state.pass_button_clicks
                                ))
                                .on_click(
                                    ctx.link()
                                        .callback(|e: MouseEvent| Msg::PassButton(e.x, e.y)),
                                ),
                            )
                            .child(Text::new(format!(
                                "Region clicks: {}",
                                ctx.state.pass_region_clicks
                            )))
                            .child(Text::new("Click empty panel area to trigger region click.")),
                    ),
            )
            .into()
    }

    fn capture_panel(&self, ctx: &Context<Self>) -> Element {
        MouseRegion::new()
            .on_click(
                ctx.link()
                    .callback(|e: MouseEvent| Msg::CaptureRegion(e.x, e.y)),
            )
            .capture_click(true)
            .hover_style(Style::new().bg(Color::indexed(236)))
            .child(
                Frame::new()
                    .title("capture_click(true)")
                    .border_style(BorderStyle::Rounded)
                    .style(Style::new().bg(Color::indexed(234)))
                    .hover_style(Style::new().bg(Color::indexed(236)))
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Region captures clicks over child button."))
                            .child(
                                Button::new(format!(
                                    "Button clicks: {}",
                                    ctx.state.capture_button_clicks
                                ))
                                .on_click(
                                    ctx.link()
                                        .callback(|e: MouseEvent| Msg::CaptureButton(e.x, e.y)),
                                ),
                            )
                            .child(Text::new(format!(
                                "Region clicks: {}",
                                ctx.state.capture_region_clicks
                            )))
                            .child(Text::new(
                                "Try clicking the button: region count increases.",
                            )),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new().mount(MouseRegionClickDemo).run()
}
