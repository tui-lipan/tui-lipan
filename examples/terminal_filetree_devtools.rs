use std::sync::Arc;
use tui_lipan::prelude::*;

struct DevToolsApp;

struct State {
    root: Arc<str>,
    selected_path: Arc<str>,
    terminal_status: ManagedTerminalStatus,
    git_refresh_token: u64,
    changed_only: bool,
}

#[derive(Clone)]
enum Msg {
    FileSelected(FileTreeEvent),
    TerminalStatus(ManagedTerminalStatus),
    RefreshGit,
    ToggleChangedOnly,
    Quit,
}

impl Component for DevToolsApp {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let root: Arc<str> = match std::env::current_dir() {
            Ok(path) => Arc::<str>::from(path.to_string_lossy().to_string()),
            Err(_) => Arc::<str>::from("."),
        };
        Self::State {
            selected_path: root.clone(),
            root,
            terminal_status: ManagedTerminalStatus::Starting,
            git_refresh_token: 0,
            changed_only: false,
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.link().send(Msg::Quit);
            return KeyUpdate::handled(Update::full());
        }

        if !key.mods.ctrl
            && !key.mods.alt
            && !key.mods.super_key
            && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        {
            ctx.link().send(Msg::RefreshGit);
            return KeyUpdate::handled(Update::full());
        }

        if !key.mods.ctrl
            && !key.mods.alt
            && !key.mods.super_key
            && matches!(key.code, KeyCode::Char('g') | KeyCode::Char('G'))
        {
            ctx.link().send(Msg::ToggleChangedOnly);
            return KeyUpdate::handled(Update::full());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::FileSelected(event) => {
                ctx.state.selected_path = event.path;
                Update::full()
            }
            Msg::TerminalStatus(status) => {
                ctx.state.terminal_status = status;
                Update::full()
            }
            Msg::RefreshGit => {
                ctx.state.git_refresh_token = ctx.state.git_refresh_token.saturating_add(1);
                Update::full()
            }
            Msg::ToggleChangedOnly => {
                ctx.state.changed_only = !ctx.state.changed_only;
                Update::full()
            }
            Msg::Quit => {
                ctx.quit();
                Update::none()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let file_tree = FileTree::new(ctx.state.root.clone())
            .git_status(true)
            .git_changed_only(ctx.state.changed_only)
            .git_diff_stats(ctx.state.changed_only)
            .icon_style(FileIconStyle::NerdFontColored)
            .indent_style(IndentStyle::None)
            .show_arrows(false)
            .git_refresh_token(ctx.state.git_refresh_token)
            .focusable(true)
            .explorer(true)
            .explorer_prefix(">")
            .on_select(ctx.link().callback(Msg::FileSelected));

        // Create a status text from the terminal status
        let status_text = match &ctx.state.terminal_status {
            ManagedTerminalStatus::Starting => "starting terminal...".to_string(),
            ManagedTerminalStatus::Ready => "ready".to_string(),
            ManagedTerminalStatus::Exited(code) => format!("exited (code: {code})"),
            ManagedTerminalStatus::Error(msg) => format!("error: {msg}"),
        };

        // Simple usage: ManagedTerminal handles all PTY management internally
        let terminal = ManagedTerminal::new()
            .config(
                TerminalPtyConfig::default()
                    .cwd(
                        std::env::current_dir()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    )
                    .term("xterm-256color"),
            )
            .scrollback(2000)
            .on_status(ctx.link().callback(Msg::TerminalStatus));

        VStack::new()
            .child(
                HStack::new()
                    .gap(1)
                    .height(Length::Flex(1))
                    .child(
                        Frame::new()
                            .title(if ctx.state.changed_only {
                                "Changed files"
                            } else {
                                "Files"
                            })
                            .border(true)
                            .padding(0)
                            .width(Length::Flex(1))
                            .height(Length::Flex(1))
                            .child(file_tree),
                    )
                    .child(
                        Frame::new()
                            .title("Terminal")
                            .status(status_text)
                            .border(true)
                            .padding(0)
                            .width(Length::Flex(2))
                            .height(Length::Flex(1))
                            .child(terminal),
                    ),
            )
            .child(
                Frame::new()
                    .border(true)
                    .height(Length::Auto)
                    .padding((0, 1))
                    .child(Text::new(format!(
                        "Selected: {} | G: toggle git changes | R: refresh git | Ctrl+Q: quit",
                        ctx.state.selected_path
                    ))),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Devtools: FileTree + PTY Terminal")
        .mount(DevToolsApp)
        .run()
}
