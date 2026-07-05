//! Theme hot-reload demo.
//!
//! Run with:
//! cargo run --example theme_hot_reload --features theme-reload

#[cfg(feature = "theme-reload")]
use std::path::PathBuf;
#[cfg(feature = "theme-reload")]
use std::time::Duration;

#[cfg(feature = "theme-reload")]
use tui_lipan::prelude::*;

#[cfg(feature = "theme-reload")]
const THEME_PATH: &str = "examples/assets/theme.toml";

#[cfg(feature = "theme-reload")]
struct ThemeHotReload;

#[cfg(feature = "theme-reload")]
struct State {
    current_theme: Theme,
    watcher: Option<ThemeWatcher>,
    theme_path: PathBuf,
}

#[cfg(feature = "theme-reload")]
impl Default for State {
    fn default() -> Self {
        Self {
            current_theme: Theme::one_dark(),
            watcher: None,
            theme_path: PathBuf::from(THEME_PATH),
        }
    }
}

#[cfg(feature = "theme-reload")]
#[derive(Clone)]
enum Msg {
    Tick,
    ThemeError(String),
    Quit,
}

#[cfg(feature = "theme-reload")]
impl Component for ThemeHotReload {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        let fallback = Theme::one_dark();
        let theme_path = ctx.state.theme_path.clone();

        match load_theme_from_toml(&theme_path, fallback.clone()) {
            Ok(theme) => {
                ctx.state.current_theme = theme;
            }
            Err(err) => {
                ctx.toast().push(Toast::new(format!(
                    "Theme load failed for {}: {err}",
                    theme_path.display()
                )));
                ctx.state.current_theme = fallback.clone();
            }
        }

        match ThemeWatcher::new(theme_path.clone(), fallback) {
            Ok(watcher) => {
                ctx.state.watcher = Some(watcher);
            }
            Err(err) => {
                ctx.toast().push(Toast::new(format!(
                    "Theme watcher failed for {}: {err}",
                    theme_path.display()
                )));
                ctx.state.watcher = None;
            }
        }

        Some(schedule_tick())
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.code == KeyCode::Esc
            || (key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')))
        {
            ctx.link().send(Msg::Quit);
            return KeyUpdate::handled(Update::none());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Tick => {
                if let Some(watcher) = ctx.state.watcher.as_ref() {
                    let mut newest_theme = None;
                    while let Some(theme) = watcher.try_recv() {
                        newest_theme = Some(theme);
                    }

                    while let Some(err) = watcher.try_recv_error() {
                        ctx.link().send(Msg::ThemeError(err));
                    }

                    if let Some(theme) = newest_theme {
                        ctx.state.current_theme = theme;
                        return Update::with_command(schedule_tick());
                    }
                }

                Update::command_only(schedule_tick())
            }
            Msg::ThemeError(err) => {
                ctx.toast().push(
                    Toast::new(format!("Theme reload failed: {err}"))
                        .title(Some("Theme Reload Error")),
                );
                Update::none()
            }
            Msg::Quit => {
                ctx.quit();
                Update::none()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = ctx.state.current_theme.clone();

        ThemeProvider::new(theme.clone())
            .child(
                VStack::new()
                    .padding(1)
                    .gap(1)
                    .child(
                        Frame::new()
                            .title("Theme Hot Reload")
                            .status("Ctrl+Q / Esc to quit")
                            .border(true)
                            .border_style(BorderStyle::Rounded)
                            .padding(1)
                            .child(
                                VStack::new()
                                    .gap(1)
                                    .child(Text::new(format!(
                                        "Watching: {}",
                                        ctx.state.theme_path.display()
                                    )))
                                    .child(
                                        Text::new("Edit the TOML file and save. New styles apply automatically.")
                                            .style(theme.muted),
                                    )
                                    .child(
                                        HStack::new()
                                            .gap(1)
                                            .child(Button::new("Filled").variant(ButtonVariant::Filled))
                                            .child(
                                                Button::new("Outlined")
                                                    .variant(ButtonVariant::Outlined),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        Frame::new()
                            .title("Status Palette")
                            .border(true)
                            .border_style(BorderStyle::Rounded)
                            .padding(1)
                            .child(
                                HStack::new()
                                    .gap(2)
                                    .child(Text::new("Success").style(Style::new().fg(theme.status.success)))
                                    .child(Text::new("Warning").style(Style::new().fg(theme.status.warning)))
                                    .child(Text::new("Error").style(Style::new().fg(theme.status.error)))
                                    .child(Text::new("Info").style(Style::new().fg(theme.status.info))),
                            ),
                    )
                    .child(
                        Frame::new()
                            .title("Git Status Palette")
                            .border(true)
                            .border_style(BorderStyle::Rounded)
                            .padding(1)
                            .child(
                                HStack::new()
                                    .gap(2)
                                    .child(
                                        Text::new("Modified")
                                            .style(Style::new().fg(theme.git_status.modified)),
                                    )
                                    .child(Text::new("Added").style(Style::new().fg(theme.git_status.added)))
                                    .child(
                                        Text::new("Deleted")
                                            .style(Style::new().fg(theme.git_status.deleted)),
                                    )
                                    .child(
                                        Text::new("Untracked")
                                            .style(Style::new().fg(theme.git_status.untracked)),
                                    ),
                            ),
                    ),
            )
            .into()
    }
}

#[cfg(feature = "theme-reload")]
fn schedule_tick() -> Command {
    Command::spawn(move |link| {
        std::thread::sleep(Duration::from_millis(150));
        link.send(Msg::Tick);
    })
}

#[cfg(feature = "theme-reload")]
fn main() -> Result<()> {
    let terminal_bg = query_host_colors().map(|colors| colors.bg);

    App::new()
        .title("tui-lipan - Theme Hot Reload")
        .terminal_bg(terminal_bg)
        .toast_placement(ToastPlacement::BottomEnd)
        .mount(ThemeHotReload)
        .run()
}

#[cfg(not(feature = "theme-reload"))]
fn main() {
    eprintln!("Run with: cargo run --example theme_hot_reload --features theme-reload");
}
