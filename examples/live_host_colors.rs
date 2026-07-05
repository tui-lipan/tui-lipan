//! Live host terminal color refresh and system theme demo.
//!
//! Run with: cargo run --example live_host_colors
//!
//! The runner refreshes on startup, focus gained, and `r`. The app-wide theme is
//! driven by `App::system_theme()`; the lower panel also shows how to build
//! app-owned tokens from `Theme::from_host_colors(...)`.

use tui_lipan::prelude::*;

struct LiveHostColors;

impl Component for LiveHostColors {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('r') | KeyCode::Char('R') => {
                ctx.request_host_terminal_color_refresh();
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let colors = ctx.host_terminal_colors();
        let generation = ctx.host_terminal_color_generation();
        let theme = ctx.theme();
        let app_owned_theme = theme_from_host(colors);

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Frame::new()
                    .title("Live Host Colors")
                    .status("r refresh - q/Esc quit")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new(status_line(colors, generation)))
                            .child(
                                Text::new(
                                    "App::system_theme() owns the active ctx.theme(); no ThemeProvider is needed.",
                                )
                                .style(theme.muted),
                            )
                            .child(palette_preview(colors)),
                    ),
            )
            .child(
                Frame::new()
                    .title("App-wide System Theme")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Primary text follows the host foreground."))
                            .child(
                                Text::new("Muted text uses the active app-wide system theme.")
                                    .style(theme.muted),
                            )
                            .child(
                                HStack::new()
                                    .gap(1)
                                    .child(Button::new("Accent"))
                                    .child(Button::outlined("Outlined")),
                            ),
                    ),
            )
            .child(
                ThemeProvider::new(app_owned_theme.clone()).child(
                    Frame::new()
                        .title("App-owned Tokens")
                        .border(true)
                        .border_style(BorderStyle::Rounded)
                        .padding(1)
                        .child(
                            VStack::new()
                                .gap(1)
                                .child(
                                    Text::new(
                                        "This panel uses Theme::from_host_colors(...) in a ThemeProvider.",
                                    )
                                    .style(app_owned_theme.muted),
                                )
                                .child(
                                    HStack::new()
                                        .gap(1)
                                        .child(Button::new("Derived"))
                                        .child(Button::outlined("Scoped")),
                                ),
                        ),
                ),
            )
            .into()
    }
}

fn theme_from_host(colors: Option<HostTerminalColors>) -> Theme {
    colors
        .map(Theme::from_host_colors)
        .unwrap_or_else(Theme::one_dark)
}

fn status_line(colors: Option<HostTerminalColors>, generation: u64) -> String {
    match colors {
        Some(colors) => format!(
            "host colors generation {generation} - fg {:?} - bg {:?}",
            colors.fg, colors.bg
        ),
        None => "host colors unavailable - terminal did not answer the startup probe".to_string(),
    }
}

fn palette_preview(colors: Option<HostTerminalColors>) -> Element {
    let Some(colors) = colors else {
        return Text::new("Press r to retry the host terminal color probe.")
            .style(Style::new().dim())
            .into();
    };

    HStack::new()
        .gap(1)
        .child(swatch("fg", colors.fg))
        .child(swatch("bg", colors.bg))
        .child(swatch("accent", colors.ansi[4]))
        .child(swatch("success", colors.ansi[2]))
        .child(swatch("warning", colors.ansi[3]))
        .into()
}

fn swatch(label: &'static str, color: Color) -> Element {
    Text::new(format!(" {label} "))
        .style(Style::new().fg(color).bold())
        .into()
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Live Host Colors")
        .theme(Theme::one_dark())
        .system_theme()
        .mount(LiveHostColors)
        .run()
}
