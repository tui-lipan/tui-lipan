use tui_lipan::prelude::*;

struct CommandPaletteDemo;

#[derive(Default)]
struct State {
    show_palette: bool,
    active_doc: String,
    devtools_on: bool,
    wraps_enabled: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    TogglePalette,
    ClosePalette,
    OpenDoc(&'static str),
    ToggleDevtools,
    ToggleWrap,
    Editor(EditorMsg),
}

#[derive(Clone, Debug)]
enum EditorMsg {
    Save,
    SaveAll,
}

#[derive(Clone, PartialEq)]
struct EditorProps {
    wraps_enabled: bool,
    active_doc: String,
    on_event: Callback<EditorMsg>,
}

struct EditorPanel;

impl Component for EditorPanel {
    type Message = ();
    type Properties = EditorProps;
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        let on_save = ctx.props.on_event.clone();
        ctx.register_command(
            CommandEntry::builder("editor.save")
                .label("Editor: Save")
                .description("Save current file")
                .category("Editor")
                .keybinding("ctrl-s")
                .enabled(true)
                .handler(Callback::new(move |_| on_save.emit(EditorMsg::Save)))
                .build(),
        );

        let on_save_all = ctx.props.on_event.clone();
        ctx.register_command(
            CommandEntry::builder("editor.save-all")
                .label("Editor: Save all")
                .description("Save all open files")
                .category("Editor")
                .keybinding("ctrl-shift-s")
                .enabled(false)
                .handler(Callback::new(move |_| on_save_all.emit(EditorMsg::SaveAll)))
                .build(),
        );

        None
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Editor")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(format!(
                        "Active document: {}",
                        ctx.props.active_doc
                    )))
                    .child(Text::new(format!(
                        "Word wrap: {}",
                        if ctx.props.wraps_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    )))
                    .child(Text::new("Commands: p opens command palette"))
                    .child(Text::new(
                        "Use palette entries to run app + editor commands",
                    )),
            )
            .into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

impl Component for CommandPaletteDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            active_doc: "src/main.rs".to_string(),
            ..State::default()
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        let registry = ctx.command_registry();

        let link = ctx.link().clone();
        registry.register(
            CommandEntry::builder("app.open-main")
                .label("Open src/main.rs")
                .description("Switch active file to src/main.rs")
                .category("Application")
                .enabled(true)
                .handler(Callback::new(move |_| {
                    link.send(Msg::OpenDoc("src/main.rs"))
                }))
                .build(),
        );

        let link = ctx.link().clone();
        registry.register(
            CommandEntry::builder("app.open-lib")
                .label("Open src/lib.rs")
                .description("Switch active file to src/lib.rs")
                .category("Application")
                .enabled(true)
                .handler(Callback::new(move |_| {
                    link.send(Msg::OpenDoc("src/lib.rs"))
                }))
                .build(),
        );

        let link = ctx.link().clone();
        registry.register(
            CommandEntry::builder("app.toggle-wrap")
                .label("Toggle word wrap")
                .description("Enable or disable editor wrapping")
                .category("Application")
                .enabled(true)
                .handler(Callback::new(move |_| link.send(Msg::ToggleWrap)))
                .build(),
        );

        let link = ctx.link().clone();
        registry.register(
            CommandEntry::builder("app.toggle-devtools-local")
                .label("Toggle DevTools (demo)")
                .description("Toggle devtools and demo indicator")
                .category("Application")
                .enabled(true)
                .handler(Callback::new(move |_| link.send(Msg::ToggleDevtools)))
                .build(),
        );

        None
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TogglePalette => {
                ctx.state.show_palette = !ctx.state.show_palette;
                Update::full()
            }
            Msg::ClosePalette => {
                ctx.state.show_palette = false;
                Update::full()
            }
            Msg::OpenDoc(path) => {
                ctx.state.active_doc = path.to_string();
                ctx.state.show_palette = false;
                Update::full()
            }
            Msg::ToggleDevtools => {
                ctx.state.devtools_on = !ctx.state.devtools_on;
                ctx.toggle_devtools();
                ctx.state.show_palette = false;
                Update::full()
            }
            Msg::ToggleWrap => {
                ctx.state.wraps_enabled = !ctx.state.wraps_enabled;
                ctx.state.show_palette = false;
                Update::full()
            }
            Msg::Editor(EditorMsg::Save) => {
                ctx.toast().push(Toast::new("Saved file"));
                ctx.state.show_palette = false;
                Update::full()
            }
            Msg::Editor(EditorMsg::SaveAll) => {
                ctx.toast()
                    .push(Toast::new("Save all is disabled in this demo"));
                ctx.state.show_palette = false;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.code == KeyCode::Char('p') && key.mods == KeyMods::default() {
            ctx.link().send(Msg::TogglePalette);
            return KeyUpdate::handled(Update::full());
        }

        if key.code == KeyCode::Char('q') && key.mods == KeyMods::default() {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut root = VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new("p: open palette | q: quit").style(Style::new().fg(Color::DarkGray)))
            .child(
                Frame::new()
                    .title("App state")
                    .border(true)
                    .padding(1)
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new(format!("Active: {}", ctx.state.active_doc)))
                            .child(Text::new(format!(
                                "DevTools: {}",
                                if ctx.state.devtools_on { "on" } else { "off" }
                            )))
                            .child(Text::new(format!(
                                "Word wrap: {}",
                                if ctx.state.wraps_enabled {
                                    "enabled"
                                } else {
                                    "disabled"
                                }
                            ))),
                    ),
            )
            .child(child(
                || EditorPanel,
                EditorProps {
                    wraps_enabled: ctx.state.wraps_enabled,
                    active_doc: ctx.state.active_doc.clone(),
                    on_event: ctx.link().callback(Msg::Editor),
                },
            ));

        if ctx.state.show_palette {
            root = root.child(
                CommandPalette::new()
                    .title("Commands")
                    .show_disabled(true)
                    .on_close(ctx.link().callback(|_| Msg::ClosePalette)),
            );
        }

        root.into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Command Palette")
        .mount(CommandPaletteDemo)
        .run()
}
