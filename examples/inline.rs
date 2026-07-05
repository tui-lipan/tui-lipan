use tui_lipan::prelude::*;

struct InlineDemo;

#[derive(Default)]
struct State {
    draft: TextInput,
    inserted: u32,
    entries: Vec<String>,
}

#[derive(Clone, Debug)]
enum Msg {
    DraftChanged(InputEvent),
    Insert,
}

impl Component for InlineDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::DraftChanged(event) => {
                ctx.state.draft.set_text(event.value.as_ref());
                ctx.state.draft.set_cursor_keep_anchor(event.cursor);
                ctx.state.draft.set_anchor(event.anchor);
                Update::full()
            }
            Msg::Insert => {
                let mut text = ctx.state.draft.text().trim().to_string();
                if text.is_empty() {
                    text = "generated log entry".to_string();
                }

                let next = ctx.state.inserted.saturating_add(1);
                ctx.state.entries.push(format!("[{next:03}] {text}"));

                ctx.state.inserted = next;
                ctx.state.draft.clear();
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                ctx.toggle_mouse_capture();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Inline mode")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status(format!(
                "Enter records a line | m toggles mouse ({}) | q/Esc quits",
                if ctx.mouse_capture_enabled() {
                    "on"
                } else {
                    "off"
                }
            ))
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Input::new(ctx.state.draft.text().to_string())
                            .cursor(ctx.state.draft.cursor())
                            .anchor(ctx.state.draft.anchor())
                            .placeholder("Type text and press Enter")
                            .border(true)
                            .on_change(ctx.link().callback(Msg::DraftChanged))
                            .on_key(ctx.link().key_handler(|key| match key.code {
                                KeyCode::Enter => Some(Msg::Insert),
                                _ => None,
                            })),
                    )
                    .child(
                        Button::filled("Insert")
                            .on_click(ctx.link().callback(|_| Msg::Insert))
                            .focusable(true),
                    )
                    .child(
                        Text::new(
                            ctx.state
                                .entries
                                .iter()
                                .rev()
                                .take(4)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                        .overflow(Overflow::Wrap),
                    )
                    .child(Text::new(format!("Inserted lines: {}", ctx.state.inserted))),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .inline_ephemeral(8)
        .mouse(true)
        .mount(InlineDemo)
        .exit_view(|_component, ctx| {
            VStack::new()
                .gap(1)
                .child(Text::new("Inline session ended"))
                .child(Text::new(format!("Inserted lines: {}", ctx.state.inserted)))
                .into()
        })
        .run()
}
