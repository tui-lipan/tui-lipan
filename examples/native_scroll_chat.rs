use std::thread;
use std::time::Duration;

use tui_lipan::prelude::*;

struct NativeScrollChat;

#[derive(Default)]
struct State {
    draft: TextInput,
    next_request_id: u64,
    committed_blocks: u64,
    active_request_id: Option<u64>,
    /// The latest uncommitted fragment still being streamed.
    active_tail: String,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    DraftChanged(InputEvent),
    Submit,
    StreamChunk { request_id: u64, chunk: String },
    StreamDone { request_id: u64 },
}

impl Component for NativeScrollChat {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            status: "Type a prompt and press Enter. Terminal scrollback stays native.".to_string(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::DraftChanged(event) => {
                ctx.state.draft.set_text(event.value.as_ref());
                ctx.state.draft.set_cursor_keep_anchor(event.cursor);
                ctx.state.draft.set_anchor(event.anchor);
                Update::full()
            }
            Msg::Submit => {
                if ctx.state.active_request_id.is_some() {
                    ctx.state.status =
                        "Wait for the current response to finish before sending another."
                            .to_string();
                    return Update::full();
                }

                let prompt = ctx.state.draft.text().trim().to_string();
                if prompt.is_empty() {
                    ctx.state.status = "Type something first.".to_string();
                    return Update::full();
                }

                let request_id = ctx.state.next_request_id.saturating_add(1);
                ctx.state.next_request_id = request_id;
                ctx.state.active_request_id = Some(request_id);
                ctx.state.active_tail.clear();
                ctx.state.draft.clear();
                ctx.state.status =
                    "Streaming - paragraphs commit into scrollback as they arrive.".to_string();

                // Commit the user's message immediately.
                ctx.append_transcript_element(user_message_card(request_id, &prompt));
                ctx.state.committed_blocks = ctx.state.committed_blocks.saturating_add(1);

                let cmd = ctx.link().command(move |link| {
                    for chunk in scripted_response_chunks(&prompt) {
                        thread::sleep(Duration::from_millis(120));
                        link.send(Msg::StreamChunk { request_id, chunk });
                    }
                    thread::sleep(Duration::from_millis(80));
                    link.send(Msg::StreamDone { request_id });
                });
                Update::with_command(cmd)
            }
            Msg::StreamChunk { request_id, chunk } => {
                if ctx.state.active_request_id != Some(request_id) {
                    return Update::none();
                }

                ctx.state.active_tail.push_str(&chunk);

                // Progressive commit: flush completed paragraphs (delimited
                // by blank lines) into transcript history so the live
                // viewport only holds the current in-progress paragraph.
                flush_completed_paragraphs(ctx);

                Update::full()
            }
            Msg::StreamDone { request_id } => {
                if ctx.state.active_request_id != Some(request_id) {
                    return Update::none();
                }

                // Commit whatever remains in the tail.
                let tail = std::mem::take(&mut ctx.state.active_tail);
                if !tail.trim().is_empty() {
                    ctx.append_transcript_element(assistant_paragraph(&tail));
                    ctx.state.committed_blocks = ctx.state.committed_blocks.saturating_add(1);
                }

                ctx.state.active_request_id = None;
                ctx.state.status =
                    "Response committed. Use terminal scrolling to review prior messages."
                        .to_string();
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
        let streaming_preview: Element = if ctx.state.active_request_id.is_some() {
            let tail_text = if ctx.state.active_tail.is_empty() {
                "Waiting for the first chunk...".to_string()
            } else {
                ctx.state.active_tail.clone()
            };
            VStack::new()
                .gap(0)
                .child(
                    Spinner::new()
                        .label("Streaming")
                        .style(Style::new().fg(Color::LightMagenta)),
                )
                .child(
                    Text::new(tail_text)
                        .overflow(Overflow::Wrap)
                        .style(Style::new().fg(Color::LightCyan)),
                )
                .into()
        } else {
            Text::new("Enter a prompt below.")
                .overflow(Overflow::Wrap)
                .style(Style::new().fg(Color::Gray))
                .into()
        };

        VStack::new()
            .gap(1)
            .child(streaming_preview)
            .child(
                Text::new(ctx.state.status.clone())
                    .overflow(Overflow::Wrap)
                    .style(Style::new().fg(Color::Gray)),
            )
            .child(
                Input::new(ctx.state.draft.text().to_string())
                    .cursor(ctx.state.draft.cursor())
                    .anchor(ctx.state.draft.anchor())
                    .placeholder("Ask something...")
                    .border(true)
                    .on_change(ctx.link().callback(Msg::DraftChanged))
                    .on_key(ctx.link().key_handler(|key| match key.code {
                        KeyCode::Enter => Some(Msg::Submit),
                        _ => None,
                    })),
            )
            .into()
    }
}

/// Flush all completed paragraphs (split on `\n\n`) from the active tail
/// into transcript history, leaving only the trailing incomplete fragment
/// in the live viewport.
fn flush_completed_paragraphs(ctx: &mut Context<NativeScrollChat>) {
    while let Some(split_pos) = ctx.state.active_tail.find("\n\n") {
        let paragraph: String = ctx.state.active_tail.drain(..split_pos).collect();
        // Consume the delimiter itself.
        ctx.state.active_tail.drain(.."\n\n".len());

        if !paragraph.trim().is_empty() {
            ctx.append_transcript_element(assistant_paragraph(&paragraph));
            ctx.state.committed_blocks = ctx.state.committed_blocks.saturating_add(1);
        }
    }
}

fn user_message_card(request_id: u64, prompt: &str) -> Element {
    Frame::new()
        .title(format!("You #{request_id}"))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .style(Style::new().fg(Color::LightGreen))
        .child(Text::new(prompt.to_string()).overflow(Overflow::Wrap))
        .into()
}

fn assistant_paragraph(text: &str) -> Element {
    Frame::new()
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .style(Style::new().fg(Color::LightBlue))
        .child(Text::new(text.to_string()).overflow(Overflow::Wrap))
        .into()
}

fn scripted_response_chunks(prompt: &str) -> Vec<String> {
    vec![
        format!("Thinking about \"{prompt}\".\n\n"),
        "This response is rendered live inside the inline viewport first. ".to_string(),
        "When a paragraph boundary is reached, that block commits into native terminal scrollback.\n\n"
            .to_string(),
        "That means previous paragraphs are no longer re-rendered by the app; ".to_string(),
        "they stay in normal shell history just like Claude Code or Gemini CLI.\n\n".to_string(),
        "Each paragraph commits independently, ".to_string(),
        "keeping the live viewport compact.".to_string(),
    ]
}

fn main() -> Result<()> {
    App::new()
        .inline_transcript(8)
        .mount(NativeScrollChat)
        .exit_view(|_component, ctx| {
            VStack::new()
                .gap(1)
                .child(Text::new("Native scroll chat ended"))
                .child(Text::new(format!(
                    "Committed blocks: {}",
                    ctx.state.committed_blocks
                )))
                .into()
        })
        .run()
}
