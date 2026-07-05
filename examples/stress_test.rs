use std::sync::Arc;
use std::time::{Duration, Instant};

use tui_lipan::prelude::*;

struct StressTest {
    last_tick: Instant,
    fps: f64,
    render_count: usize,
}

#[derive(Default)]
struct State {
    text: String,
    items: Arc<[ListItem]>,
    selected: usize,
    show_fps: bool,
}

#[derive(Clone)]
enum Msg {
    Tick,
    Text(String),
    Select(usize),
    ToggleFps,
}

impl Component for StressTest {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            text: String::from(
                "Type here to test cursor blink optimization.\n\nWhen not typing, the app should be static (no renders).",
            ),
            items: (0..1000)
                .map(|i| ListItem::new(format!("Item {}", i)))
                .collect::<Vec<_>>()
                .into(),
            selected: 0,
            show_fps: true,
        }
    }

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        // Start tick loop for FPS display (optional, toggle with 'f')
        Some(Command::spawn(move |link| {
            link.send(Msg::Tick);
        }))
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Tick => {
                let now = Instant::now();
                let delta = now.duration_since(self.last_tick);
                self.last_tick = now;
                self.fps = 1.0 / delta.as_secs_f64();
                self.render_count += 1;

                // Only continue ticking if FPS display is enabled
                if ctx.state.show_fps {
                    Update::with_command(Command::spawn(move |link| {
                        std::thread::sleep(Duration::from_millis(16));
                        link.send(Msg::Tick);
                    }))
                } else {
                    Update::none()
                }
            }
            Msg::Text(s) => {
                ctx.state.text = s;
                Update::full()
            }
            Msg::Select(idx) => {
                ctx.state.selected = idx;
                Update::full()
            }
            Msg::ToggleFps => {
                ctx.state.show_fps = !ctx.state.show_fps;
                if ctx.state.show_fps {
                    // Restart tick loop
                    Update::with_command(Command::spawn(move |link| {
                        std::thread::sleep(Duration::from_millis(16));
                        link.send(Msg::Tick);
                    }))
                } else {
                    Update::full()
                }
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if matches!(key.code, KeyCode::Char('f') | KeyCode::Char('F')) {
            ctx.link().send(Msg::ToggleFps);
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let fps_text = if ctx.state.show_fps {
            format!(
                "FPS: {:.2} | Renders: {} | Press 'f' to toggle FPS",
                self.fps, self.render_count
            )
        } else {
            format!(
                "FPS: (paused) | Renders: {} | Press 'f' to toggle FPS",
                self.render_count
            )
        };

        rsx! {
            HStack {
                gap: 1,
                padding: 1,
                VStack {
                    width: Length::Flex(1),
                    border: true,
                    List {
                        items_arc: ctx.state.items.clone(),
                        selected: ctx.state.selected,
                        on_select: ctx.link().callback(|e: ListEvent| Msg::Select(e.index)),
                        scrollbar: true,
                        item_hover_style: Style::new().bg(Color::DarkGray),
                        show_scroll_indicators: true,
                        height: Length::Flex(1),
                    },
                },
                VStack {
                    width: Length::Flex(1),
                    border: true,
                    Text { content: fps_text },
                    TextArea {
                        value: ctx.state.text.clone(),
                        on_change: ctx.link().callback(|e: TextAreaEvent| Msg::Text(e.value.to_string())),
                        height: Length::Flex(1),
                        border: true,
                        line_numbers: true,
                    },
                },
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Stress Test - Performance Optimization Demo")
        .mount(StressTest {
            last_tick: Instant::now(),
            fps: 0.0,
            render_count: 0,
        })
        .run()
}
