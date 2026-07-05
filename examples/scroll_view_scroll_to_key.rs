use std::time::Duration;

use tui_lipan::prelude::*;

struct ScrollToKeyDemo;

struct MessageItem {
    id: usize,
    author: &'static str,
    title: &'static str,
    body: &'static str,
}

const MESSAGES: &[MessageItem] = &[
    MessageItem {
        id: 1,
        author: "Mila",
        title: "Release checklist",
        body: "Docs are updated. We still need an example for scroll_to_key before the release note is ready.",
    },
    MessageItem {
        id: 2,
        author: "Kai",
        title: "Search UX",
        body: "When users search through long threads, jumping directly to the matching message feels much better than manual scrolling.",
    },
    MessageItem {
        id: 3,
        author: "Rin",
        title: "Design note",
        body: "Let's keep the demo simple: type a word like release, search, telemetry, or invoice and the scroll view should jump there.",
    },
    MessageItem {
        id: 4,
        author: "Noah",
        title: "Telemetry event",
        body: "The background worker now reports a compact telemetry summary every five minutes.",
    },
    MessageItem {
        id: 5,
        author: "Ivy",
        title: "Invoice follow-up",
        body: "Finance confirmed the invoice batch landed, but one customer record still needs a manual retry.",
    },
    MessageItem {
        id: 6,
        author: "Omar",
        title: "Message rendering",
        body: "We should key each message frame directly so search jumps land on the exact timeline row.",
    },
    MessageItem {
        id: 7,
        author: "Lena",
        title: "Search result focus",
        body: "After the first match is found we can keep the input focused and still scroll the message timeline to the result.",
    },
    MessageItem {
        id: 8,
        author: "Jules",
        title: "Performance check",
        body: "A direct jump is cheap because the scroll view already knows child layout rects after reconciliation.",
    },
    MessageItem {
        id: 9,
        author: "Sana",
        title: "Deployment note",
        body: "Staging deployment passed, and the release candidate is waiting for sign-off from QA.",
    },
    MessageItem {
        id: 10,
        author: "Theo",
        title: "Support inbox",
        body: "A support message mentioned invoice search again, so this example should cover that wording too.",
    },
    MessageItem {
        id: 11,
        author: "Ava",
        title: "Window sizing",
        body: "The example should behave nicely on small terminals, so keep the layout to a single column.",
    },
    MessageItem {
        id: 12,
        author: "Ezra",
        title: "Final review",
        body: "Once this ships, the docs can point to a tiny example that demonstrates controlled scrolling by key.",
    },
];

#[derive(Default)]
struct State {
    query: TextInput,
}

#[derive(Clone, Debug)]
enum Msg {
    QueryChanged(InputEvent),
}

impl Component for ScrollToKeyDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::QueryChanged(event) => {
                ctx.state.query.set_text(event.value.as_ref());
                ctx.state.query.set_cursor_keep_anchor(event.cursor);
                ctx.state.query.set_anchor(event.anchor);
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
        let query = ctx.state.query.text();
        let jump_target = first_match_key(query);
        let status = match jump_target.as_deref() {
            Some(key) => {
                let title = MESSAGES
                    .iter()
                    .find(|message| message_key(message.id) == key)
                    .map(|message| message.title)
                    .unwrap_or("unknown");
                format!("Jumping to first match: {title}")
            }
            None if query.trim().is_empty() => {
                "Type part of a title, author, or body to jump to a message".to_string()
            }
            None => format!("No messages match \"{query}\""),
        };

        let mut timeline = ScrollView::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .scrollbar(true)
            .show_scroll_indicators(true)
            .padding(1)
            .gap(1)
            .scroll_keys(ScrollKeymap::DEFAULT)
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(180),
                easing: Easing::EaseOutQuad,
            })
            .children(MESSAGES.iter().map(render_message));

        if let Some(target) = jump_target {
            timeline = timeline.scroll_to_key(target);
        }

        Frame::new()
            .title("ScrollView scroll_to_key")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status("Type to jump | q or Esc quits")
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Input::new(query.to_string())
                            .cursor(ctx.state.query.cursor())
                            .anchor(ctx.state.query.anchor())
                            .placeholder("Search messages: release, search, telemetry, invoice...")
                            .border(true)
                            .on_change(ctx.link().callback(Msg::QueryChanged)),
                    )
                    .child(Text::new(status).style(Style::new().fg(Color::DarkGray)))
                    .child(timeline),
            )
            .into()
    }
}

fn message_key(id: usize) -> String {
    format!("message-{id}")
}

fn first_match_key(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    let query = query.to_ascii_lowercase();
    MESSAGES.iter().find_map(|message| {
        let haystack = format!(
            "{} {} {}",
            message.author.to_ascii_lowercase(),
            message.title.to_ascii_lowercase(),
            message.body.to_ascii_lowercase(),
        );
        haystack.contains(&query).then(|| message_key(message.id))
    })
}

fn render_message(message: &MessageItem) -> Element {
    Element::from(
        Frame::new()
            .title(message.title)
            .border(true)
            .border_style(BorderStyle::Plain)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(format!("From {}", message.author))
                            .style(Style::new().fg(Color::LightBlue).bold()),
                    )
                    .child(Text::new(message.body).style(Style::new().fg(Color::White))),
            ),
    )
    .key(message_key(message.id))
}

fn main() -> Result<()> {
    App::new()
        .title("ScrollView scroll_to_key example")
        .mount(ScrollToKeyDemo)
        .run()
}
