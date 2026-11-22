//! Alpha foreground backdrop repro.
//!
//! Run with:
//! cargo run --example alpha_foreground_backdrop
//!
//! The two rows inside the panel should look the same. If the first row is
//! visibly different from the second one, alpha foreground rendering is using
//! `App::terminal_bg(...)` instead of the already-painted parent background.

use tui_lipan::prelude::*;

const TERMINAL_FALLBACK_BG: Color = Color::Rgb(0x23, 0x23, 0x29);
const PANEL_BG: Color = Color::Rgb(0x15, 0x15, 0x19);

struct AlphaForegroundBackdrop;

impl Component for AlphaForegroundBackdrop {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let alpha_fg = Paint::rgba(255, 255, 255, 0x40);
        let expected = Style::new().fg(alpha_fg).bg(PANEL_BG);
        let inherited = Style::new().fg(alpha_fg);
        let muted = Style::new().fg(Color::rgb(0x9A, 0x9A, 0xA6));

        VStack::new()
            .padding(1)
            .gap(1)
            .style(Style::new().bg(TERMINAL_FALLBACK_BG))
            .child(
                Frame::new()
                    .title("Alpha foreground backdrop repro")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .padding(1)
                    .style(Style::new().bg(PANEL_BG))
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new(
                                "Expected: both rows below render with the same foreground color.",
                            ))
                            .child(
                                Text::new(
                                    "1. parent bg only: alpha fg should blend over this Frame bg",
                                )
                                .style(inherited),
                            )
                            .child(
                                Text::new(
                                    "2. explicit text bg: workaround/reference blend target",
                                )
                                .style(expected),
                            )
                            .child(
                                Text::new(
                                    "If row 1 differs, it probably blended against App::terminal_bg (#232329).",
                                )
                                .style(muted),
                            ),
                    ),
            )
            .child(
                Frame::new()
                    .title("Interactive hover check")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .padding(1)
                    .style(Style::new().bg(PANEL_BG))
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(
                                Text::new("Hover the button. Its alpha foreground should blend over the dark panel bg.")
                                    .style(muted),
                            )
                            .child(
                                Text::new(
                                    "This disables auto-contrast and clears the default focus accent so the raw hover blend stays visible.",
                                )
                                    .style(muted),
                            )
                            .child(
                                Button::filled("Hover me")
                                    .style(Style::new().bg(PANEL_BG).fg(Color::White))
                                    .focus_style(Style::default())
                                    .hover_style(
                                        Style::new()
                                            .fg(alpha_fg)
                                            .contrast_policy(ContrastPolicy::Off),
                                    ),
                            ),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Alpha Foreground Backdrop")
        .terminal_bg(Some(TERMINAL_FALLBACK_BG))
        .mount(AlphaForegroundBackdrop)
        .run()
}
