/// Stress test for ScrollView with many Frame+DocumentView children.
///
/// Simulates a chat/message-list UI (like opencode's message view) to measure
/// the layout and rendering cost of scrolling through hundreds of rich-text
/// messages.
///
/// Usage:
///   cargo run --example scroll_view_stress --features markdown
///
/// The FPS counter in the status bar shows real-time frame throughput.
use std::cell::Cell;
use std::sync::Arc;

use tui_lipan::prelude::*;

struct ScrollStressDemo;

#[derive(Default)]
struct State {
    offset: usize,
    fps: FpsCounter,
}

struct FpsCounter {
    frames: Cell<u32>,
    last_tick: Cell<std::time::Instant>,
    display: Cell<f64>,
}

impl Default for FpsCounter {
    fn default() -> Self {
        Self {
            frames: Cell::new(0),
            last_tick: Cell::new(std::time::Instant::now()),
            display: Cell::new(0.0),
        }
    }
}

impl FpsCounter {
    fn tick(&self) -> f64 {
        self.frames.set(self.frames.get() + 1);
        let elapsed = self.last_tick.get().elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            self.display.set(self.frames.get() as f64 / elapsed);
            self.frames.set(0);
            self.last_tick.set(std::time::Instant::now());
        }
        self.display.get()
    }
}

#[derive(Clone, Debug)]
enum Msg {
    Scroll(ScrollEvent),
}

const MESSAGE_COUNT: usize = 200;

fn generate_messages() -> Vec<(String, String)> {
    let bodies = [
        "Hello! This is a short message.",
        "Here's a message with some **bold text** and *italic text* to test markdown rendering \
         performance inside a scroll view with many children.",
        "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```\n\nCode blocks should \
         also render correctly and contribute to the layout cost.",
        "A longer message that contains multiple paragraphs.\n\nThe second paragraph has a \
         [link](https://example.com) and some `inline code` that the markdown formatter needs \
         to parse.\n\n- Item one\n- Item two\n- Item three",
        "Short reply.",
        "## Heading\n\nThis message starts with a heading and includes a table:\n\n\
         | Column A | Column B | Column C |\n\
         |----------|----------|----------|\n\
         | Value 1  | Value 2  | Value 3  |\n\
         | Value 4  | Value 5  | Value 6  |",
        "> This is a blockquote that spans multiple lines to test how the document view handles \
         quoted content with word wrapping enabled.\n\n\
         And a paragraph after the quote with some more text to fill the space.",
        "1. First ordered item\n2. Second ordered item\n3. Third ordered item with a longer \
         description that should wrap around in the document view",
    ];

    (0..MESSAGE_COUNT)
        .map(|i| {
            let author = match i % 5 {
                0 => "Alice",
                1 => "Bob",
                2 => "Charlie",
                3 => "Diana",
                _ => "Eve",
            };
            let body = bodies[i % bodies.len()].to_string();
            (format!("{author} (message #{i})"), body)
        })
        .collect()
}

impl Component for ScrollStressDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Scroll(event) => {
                ctx.state.offset = event.offset;
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
        let fps = ctx.state.fps.tick();
        let messages = generate_messages();

        let timeline = ScrollView::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .scrollbar(true)
            .gap(1)
            .padding(1)
            .offset(ctx.state.offset)
            .scroll_keys(ScrollKeymap::DEFAULT)
            .on_scroll(ctx.link().callback(Msg::Scroll))
            .children(
                messages
                    .iter()
                    .enumerate()
                    .map(|(i, (author, body))| render_message(i, author, body)),
            );

        Frame::new()
            .title("ScrollView Stress Test")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status(format!(
                "FPS: {fps:.1} | {MESSAGE_COUNT} messages | offset: {} | q to quit",
                ctx.state.offset,
            ))
            .child(timeline)
            .into()
    }
}

fn render_message(index: usize, _author: &str, body: &str) -> Element {
    let body: Arc<str> = Arc::from(body);
    Frame::new()
        .border(false)
        .padding((1, 0, 1, 3))
        .style(Style::new().bg(Color::Rgb(30, 30, 30)))
        .child(
            VStack::new().height(Length::Auto).gap(0).child(
                DocumentView::new(body)
                    .markdown()
                    .border(false)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .wrap(true)
                    .focusable(false)
                    .style(Style::new().fg(Color::White))
                    .height(Length::Auto),
            ),
        )
        .key(format!("msg-{index}"))
}

fn main() -> Result<()> {
    App::new()
        .title("ScrollView stress test")
        .mount(ScrollStressDemo)
        .run()
}
