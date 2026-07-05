use tui_lipan::prelude::*;

struct AutoWrapDemo;

#[derive(Default)]
struct State {
    draft: TextInput,
    inserted: u32,
}

#[derive(Clone, Debug)]
enum Msg {
    DraftChanged(InputEvent),
    Insert,
}

impl Component for AutoWrapDemo {
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
                let line = RichText::new()
                    .span(Span::new(format!("[{next:03}] {text}")).fg(Color::LightCyan));
                ctx.append_transcript_lines([line]);

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
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("Inline Transcript mode")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status("Enter appends to transcript | q/Esc quits | try resizing the terminal")
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
                        Button::filled("Append")
                            .on_click(ctx.link().callback(|_| Msg::Insert))
                            .focusable(true),
                    )
                    .child(Text::new(format!("Inserted lines: {}", ctx.state.inserted))),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .inline_transcript(8)
        .mount(AutoWrapDemo)
        .exit_view(|_component, ctx| {
            VStack::new()
                .gap(1)
                .child(Text::new("Inline transcript session ended"))
                .child(Text::new(format!("Inserted lines: {}", ctx.state.inserted)))
                .into()
        })
        .run()
}
