use tui_lipan::prelude::*;

#[derive(Clone, Copy)]
struct CommandItem {
    label: &'static str,
    hint: &'static str,
}

const COMMANDS: &[CommandItem] = &[
    CommandItem {
        label: "Build workspace",
        hint: "cargo build",
    },
    CommandItem {
        label: "Run tests",
        hint: "cargo test",
    },
    CommandItem {
        label: "Lint code",
        hint: "cargo clippy",
    },
    CommandItem {
        label: "Format code",
        hint: "cargo fmt",
    },
    CommandItem {
        label: "Run inline demo",
        hint: "cargo run --example inline",
    },
    CommandItem {
        label: "Open docs",
        hint: "cargo doc --open",
    },
    CommandItem {
        label: "Check dependencies",
        hint: "cargo tree --depth 1",
    },
    CommandItem {
        label: "Profile startup",
        hint: "cargo run --release",
    },
    CommandItem {
        label: "List examples",
        hint: "cargo run --example showcase",
    },
];

struct InlineListPicker;

#[derive(Default)]
struct State {
    query: TextInput,
    selected: usize,
    executed: u32,
    status: String,
    recent: Vec<String>,
}

#[derive(Clone, Debug)]
enum Msg {
    QueryChanged(InputEvent),
    Selected(ListEvent),
    ScrollTo(usize),
    Activate,
}

impl Component for InlineListPicker {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            status: "Pick a command and press Enter to insert it above".to_string(),
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::QueryChanged(event) => {
                ctx.state.query.set_text(event.value.as_ref());
                ctx.state.query.set_cursor_keep_anchor(event.cursor);
                ctx.state.query.set_anchor(event.anchor);
                ctx.state.selected = 0;
                Update::full()
            }
            Msg::Selected(event) => {
                ctx.state.selected = event.index;
                Update::full()
            }
            Msg::ScrollTo(index) => {
                ctx.state.selected = index;
                Update::full()
            }
            Msg::Activate => {
                let filtered = filtered_commands(ctx.state.query.text());
                if filtered.is_empty() {
                    ctx.state.status = "No command matches this query".to_string();
                    return Update::full();
                }

                let selected = ctx.state.selected.min(filtered.len().saturating_sub(1));
                let command = filtered[selected];
                let next = ctx.state.executed.saturating_add(1);

                ctx.state
                    .recent
                    .push(format!("[{next:03}] {} -> {}", command.label, command.hint));

                ctx.state.executed = next;
                ctx.state.status = format!("Queued: {}", command.hint);
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
        let filtered = filtered_commands(ctx.state.query.text());
        let selected = clamp_selected(ctx.state.selected, filtered.len());

        let items = filtered.iter().map(|item| {
            ListItem::new(item.label).description_spans([
                Span::new(" ").fg(Color::DarkGray),
                Span::new(item.hint).fg(Color::DarkGray),
            ])
        });

        Frame::new()
            .title("Inline list picker")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .status(format!(
                "Enter = insert above | m toggles mouse ({}) | q/Esc quits",
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
                        Input::new(ctx.state.query.text().to_string())
                            .cursor(ctx.state.query.cursor())
                            .anchor(ctx.state.query.anchor())
                            .placeholder("Filter commands")
                            .border(true)
                            .on_change(ctx.link().callback(Msg::QueryChanged))
                            .on_key(ctx.link().key_handler(|key| match key.code {
                                KeyCode::Enter => Some(Msg::Activate),
                                _ => None,
                            })),
                    )
                    .child(
                        List::new()
                            .title("Commands")
                            .border(true)
                            .scrollbar(true)
                            .scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            )
                            .show_scroll_indicators(true)
                            .selection_symbol(Some("➜ "))
                            .empty_text("No commands")
                            .items(items)
                            .selected(selected)
                            .on_select(ctx.link().callback(Msg::Selected))
                            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                            .on_activate(ctx.link().callback(|_| Msg::Activate))
                            .height(Length::Flex(1)),
                    )
                    .child(Text::new(format!(
                        "Runs: {} | {}",
                        ctx.state.executed, ctx.state.status
                    )))
                    .child(
                        Text::new(
                            ctx.state
                                .recent
                                .iter()
                                .rev()
                                .take(5)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                        .overflow(Overflow::Wrap),
                    ),
            )
            .into()
    }
}

fn filtered_commands(query: &str) -> Vec<CommandItem> {
    let needle = query.trim().to_lowercase();
    COMMANDS
        .iter()
        .copied()
        .filter(|item| {
            needle.is_empty()
                || item.label.to_lowercase().contains(&needle)
                || item.hint.to_lowercase().contains(&needle)
        })
        .collect()
}

fn clamp_selected(selected: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        selected.min(len.saturating_sub(1))
    }
}

fn main() -> Result<()> {
    App::new()
        .inline_ephemeral(12)
        .mouse(true)
        .mount(InlineListPicker)
        .run()
}
