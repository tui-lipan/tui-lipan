use tui_lipan::prelude::*;
struct InputsDemo;

#[derive(Default)]
struct State {
    username: TextInput,
    password: TextInput,
    search: TextInput,
    bio: TextEditor,
    logs: TextEditor,
}

#[derive(Clone, Debug)]
enum Msg {
    UsernameChanged(InputEvent),
    PasswordChanged(InputEvent),
    SearchChanged(InputEvent),
    BioChanged(TextAreaEvent),
    LogsChanged(TextAreaEvent),
    ScrollTo(usize),
}

impl Component for InputsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let username = TextInput::new("admin");
        let bio = TextEditor::new("I love building TUIs with lipan!\nIt's so much fun.");
        let logs = TextEditor::new(
            "System started...\nReady for input.\nTry typing something!\nVery long log line to demonstrate horizontal scrolling: 0123456789012345678901234567890123456789",
        );
        State {
            username,
            password: TextInput::new(""),
            search: TextInput::new(""),
            bio,
            logs,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        rsx! {
            Frame {
                title: "Inputs & TextAreas Demo",
                padding: 1,
                VStack {
                    gap: 1,
                    HStack {
                        gap: 2,
                        VStack {
                            width: Length::Flex(1),
                            gap: 1,
                            Text {
                                content: "Single-line Inputs",
                                style: Style::default().bold().fg(Color::Yellow),
                            },
                            VStack {
                                gap: 0,
                                Text { content: "Basic Username:" },
                                Input::bound(&ctx.state.username)
                                    .placeholder("Enter username...")
                                    .caret_shape(CaretShape::Bar)
                                    .on_change(ctx.link().callback(Msg::UsernameChanged)),
                            },
                            VStack {
                                gap: 0,
                                Text { content: "Password (Masked):" },
                                Input::bound(&ctx.state.password)
                                    .placeholder("Enter password...")
                                    .mask(Some('*'))
                                    .on_change(ctx.link().callback(Msg::PasswordChanged)),
                            },
                            VStack {
                                gap: 0,
                                Text { content: "Search with Prefix/Suffix:" },
                                Input::bound(&ctx.state.search)
                                    .prefix("🔍 ")
                                    .suffix(format!(" {} chars", ctx.state.search.text().len()))
                                    .placeholder("Type to search...")
                                    .on_change(ctx.link().callback(Msg::SearchChanged)),
                            },
                        },
                        VStack {
                            width: Length::Flex(1),
                            gap: 1,
                            Text {
                                content: "Multi-line Bio",
                                style: Style::default().bold().fg(Color::Yellow),
                            },
                            TextArea::bound(&ctx.state.bio)
                                .border(true)
                                .scrollbar(true)
                                .caret_shape(CaretShape::Underline)
                                .on_change(ctx.link().callback(Msg::BioChanged))
                                .on_scroll_to(ctx.link().callback(Msg::ScrollTo)),
                        },
                    },
                    Divider::horizontal(),
                    Text {
                        content: "Logs / Large Text (Line Numbers enabled)",
                        style: Style::default().bold().fg(Color::Yellow),
                    },
                    TextArea::bound(&ctx.state.logs)
                        .line_numbers(true)
                        .min_line_number_width(3)
                        .height(Length::Flex(1))
                        .border(true)
                        .wrap(false)
                        .scrollbar_config(ScrollbarConfig::new().variant(ScrollbarVariant::Integrated))
                        .h_scrollbar(true)
                        .h_scrollbar_variant(ScrollbarVariant::Integrated)
                        .caret_shape(CaretShape::Bar)
                        .on_change(ctx.link().callback(Msg::LogsChanged))
                        .on_scroll_to(ctx.link().callback(Msg::ScrollTo)),
                    StatusBar {
                        left: Text::new(
                            format!(
                                "q: quit | User: {} | Bio: {} chars", ctx.state.username.text(), ctx.state
                                .bio.text().len()
                            ),
                        ),
                    },
                },
            }
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::UsernameChanged(ev) => {
                ev.apply_to(&mut ctx.state.username);
                Update::full()
            }
            Msg::PasswordChanged(ev) => {
                ev.apply_to(&mut ctx.state.password);
                Update::full()
            }
            Msg::SearchChanged(ev) => {
                ev.apply_to(&mut ctx.state.search);
                Update::full()
            }
            Msg::BioChanged(ev) => {
                ev.apply_to(&mut ctx.state.bio);
                Update::full()
            }
            Msg::LogsChanged(ev) => {
                ev.apply_to(&mut ctx.state.logs);
                Update::full()
            }
            Msg::ScrollTo(_offset) => {
                // Framework handles manual scrolling automatically via NodeKind state
                Update::none()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.is(KeyCode::Char('q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Inputs Demo")
        .mount(InputsDemo)
        .run()
}
